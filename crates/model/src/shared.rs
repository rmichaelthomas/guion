//! Reference counting for values.
//!
//! A count is plain while one thread holds the value, and atomic once the value has been
//! shared with other threads, as in Lean 4. Most programs never share a value between
//! threads, so on server CPUs, where atomic read-modify-write instructions are slow, this
//! keeps clones and drops as cheap as ordinary loads and stores.
//!
//! The count's sign records the mode: positive counts belong to the thread that made the
//! value, negative counts are shared and updated atomically, and zero marks a value that
//! lives for the rest of the process and is never counted. Before a value is reachable from
//! another thread, the thread holding it calls `share_across_threads`, which switches it and
//! everything it holds to shared counts. A shared value's contents are therefore always
//! shared too. When a shared value is held in only one place again, taking it for writing
//! returns it to plain counting. Debug builds check that plain counts are only updated by
//! the thread that owns them.

use std::{
    marker::PhantomData,
    ptr::NonNull,
    sync::atomic::{fence, AtomicIsize, Ordering},
};

/// The count in a shared allocation's header.
pub(crate) struct HybridCount {
    count: AtomicIsize,
    #[cfg(debug_assertions)]
    owner: std::sync::atomic::AtomicUsize,
}

#[cfg(debug_assertions)]
fn current_thread_token() -> usize {
    thread_local!(static TOKEN: u8 = const { 0 });
    TOKEN.with(|token| token as *const u8 as usize)
}

impl HybridCount {
    pub(crate) fn new() -> Self {
        Self {
            count: AtomicIsize::new(1),
            #[cfg(debug_assertions)]
            owner: std::sync::atomic::AtomicUsize::new(current_thread_token()),
        }
    }

    /// A count for a value that is never freed, such as one held in a process-wide cache.
    pub(crate) fn immortal() -> Self {
        Self {
            count: AtomicIsize::new(0),
            #[cfg(debug_assertions)]
            owner: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    #[inline]
    fn check_owner(&self) {
        #[cfg(debug_assertions)]
        assert_eq!(
            self.owner.load(Ordering::Relaxed),
            current_thread_token(),
            "a value was reference-counted on a thread it was not shared with"
        );
    }

    #[inline]
    pub(crate) fn increment(&self) {
        let count = self.count.load(Ordering::Relaxed);
        if count > 0 {
            self.check_owner();
            if count == isize::MAX {
                std::process::abort();
            }
            self.count.store(count + 1, Ordering::Relaxed);
        } else if count < 0 && self.count.fetch_sub(1, Ordering::Relaxed) == isize::MIN + 1 {
            std::process::abort();
        }
    }

    /// Release one reference; true when it was the last and the allocation must be freed.
    #[inline]
    pub(crate) fn decrement(&self) -> bool {
        let count = self.count.load(Ordering::Relaxed);
        if count > 0 {
            self.check_owner();
            if count == 1 {
                return true;
            }
            self.count.store(count - 1, Ordering::Relaxed);
            false
        } else if count < 0 {
            if self.count.fetch_add(1, Ordering::Release) != -1 {
                return false;
            }
            fence(Ordering::Acquire);
            true
        } else {
            false
        }
    }

    /// Whether the caller's reference is the only one. The caller must hold that reference
    /// mutably: a shared value held in one place is then returned to plain counting, since no
    /// other thread can reach it.
    #[inline]
    pub(crate) fn claim_unique(&self) -> bool {
        match self.count.load(Ordering::Acquire) {
            1 => {
                self.check_owner();
                true
            }
            -1 => {
                #[cfg(debug_assertions)]
                self.owner.store(current_thread_token(), Ordering::Relaxed);
                self.count.store(1, Ordering::Relaxed);
                true
            }
            _ => false,
        }
    }

    /// References held, or `usize::MAX` for a value that is never freed.
    pub(crate) fn get(&self) -> usize {
        match self.count.load(Ordering::Acquire) {
            0 => usize::MAX,
            count => count.unsigned_abs(),
        }
    }

    /// Switch to shared counting; true when the count was plain, so the caller must also
    /// share the contents.
    pub(crate) fn share(&self) -> bool {
        let count = self.count.load(Ordering::Relaxed);
        if count <= 0 {
            return false;
        }
        self.check_owner();
        self.count.store(-count, Ordering::Relaxed);
        true
    }
}

/// Values whose contents can be prepared to be read from other threads.
pub trait ShareAcrossThreads {
    /// Switch every reference count this value holds, at any depth, to atomic counting.
    /// Call it on the thread that holds the value, before another thread can reach it.
    fn share_across_threads(&self);
}

/// A reference-counted pointer, like `Arc`, with the counting described in this module.
pub struct Shared<T>(NonNull<SharedBox<T>>, PhantomData<SharedBox<T>>);

struct SharedBox<T> {
    count: HybridCount,
    value: T,
}

// SAFETY: plain counts are updated only by the thread that made the value, since a value is
// shared (and its counts made atomic) before another thread can reach it; the contents are
// read through `&T` and written only while the count is one, as in `Arc`.
unsafe impl<T: Send + Sync> Send for Shared<T> {}
// SAFETY: as above.
unsafe impl<T: Send + Sync> Sync for Shared<T> {}

impl<T> Shared<T> {
    #[inline]
    pub fn new(value: T) -> Self {
        let boxed = Box::new(SharedBox {
            count: HybridCount::new(),
            value,
        });
        Self(NonNull::from(Box::leak(boxed)), PhantomData)
    }

    /// A value that is never freed and never counted, for process-wide caches.
    pub fn immortal(value: T) -> Self {
        let boxed = Box::new(SharedBox {
            count: HybridCount::immortal(),
            value,
        });
        Self(NonNull::from(Box::leak(boxed)), PhantomData)
    }

    #[inline]
    fn inner(&self) -> &SharedBox<T> {
        // SAFETY: the box lives as long as any `Shared` pointing at it.
        unsafe { self.0.as_ref() }
    }

    pub fn ptr_eq(left: &Self, right: &Self) -> bool {
        left.0 == right.0
    }

    pub fn as_ptr(this: &Self) -> *const T {
        &this.inner().value
    }

    pub fn strong_count(this: &Self) -> usize {
        this.inner().count.get()
    }

    #[inline]
    pub fn get_mut(this: &mut Self) -> Option<&mut T> {
        if this.inner().count.claim_unique() {
            // SAFETY: this is the only reference, and it is borrowed mutably.
            Some(unsafe { &mut this.0.as_mut().value })
        } else {
            None
        }
    }

    /// The contents, if this is the only reference; otherwise this reference back.
    pub fn try_unwrap(this: Self) -> Result<T, Self> {
        if !this.inner().count.claim_unique() {
            return Err(this);
        }
        let this = std::mem::ManuallyDrop::new(this);
        // SAFETY: this was the only reference, and it is not dropped again.
        let boxed = unsafe { Box::from_raw(this.0.as_ptr()) };
        Ok(boxed.value)
    }

    #[inline(never)]
    fn free(&mut self) {
        // SAFETY: the last reference was just released; the box came from `Box::leak`.
        drop(unsafe { Box::from_raw(self.0.as_ptr()) });
    }
}

impl<T: Clone> Shared<T> {
    /// The contents for writing, copying them first when another reference shares them.
    #[inline]
    pub fn make_mut(this: &mut Self) -> &mut T {
        if !this.inner().count.claim_unique() {
            *this = Self::new(this.inner().value.clone());
        }
        // SAFETY: this is now the only reference, and it is borrowed mutably.
        unsafe { &mut this.0.as_mut().value }
    }

    pub fn unwrap_or_clone(this: Self) -> T {
        Self::try_unwrap(this).unwrap_or_else(|shared| shared.inner().value.clone())
    }
}

impl<T: ShareAcrossThreads> ShareAcrossThreads for Shared<T> {
    fn share_across_threads(&self) {
        if self.inner().count.share() {
            self.inner().value.share_across_threads();
        }
    }
}

impl<T> Clone for Shared<T> {
    #[inline]
    fn clone(&self) -> Self {
        self.inner().count.increment();
        Self(self.0, PhantomData)
    }
}

impl<T> Drop for Shared<T> {
    #[inline]
    fn drop(&mut self) {
        if self.inner().count.decrement() {
            self.free();
        }
    }
}

impl<T> std::ops::Deref for Shared<T> {
    type Target = T;
    #[inline]
    fn deref(&self) -> &T {
        &self.inner().value
    }
}

impl<T> AsRef<T> for Shared<T> {
    fn as_ref(&self) -> &T {
        self
    }
}

impl<T> std::borrow::Borrow<T> for Shared<T> {
    fn borrow(&self) -> &T {
        self
    }
}

impl<T> From<T> for Shared<T> {
    fn from(value: T) -> Self {
        Self::new(value)
    }
}

impl<T: Default> Default for Shared<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for Shared<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(&**self, f)
    }
}

impl<T: std::fmt::Display> std::fmt::Display for Shared<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&**self, f)
    }
}

impl<T: PartialEq> PartialEq for Shared<T> {
    fn eq(&self, other: &Self) -> bool {
        **self == **other
    }
}

impl<T: Eq> Eq for Shared<T> {}

impl<T: PartialOrd> PartialOrd for Shared<T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        (**self).partial_cmp(&**other)
    }
}

impl<T: Ord> Ord for Shared<T> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (**self).cmp(&**other)
    }
}

impl<T: std::hash::Hash> std::hash::Hash for Shared<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        (**self).hash(state)
    }
}

impl<T: serde::Serialize> serde::Serialize for Shared<T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        (**self).serialize(serializer)
    }
}

impl<T> ShareAcrossThreads for Vec<T>
where
    T: ShareAcrossThreads,
{
    fn share_across_threads(&self) {
        for item in self {
            item.share_across_threads();
        }
    }
}

impl ShareAcrossThreads for u8 {
    fn share_across_threads(&self) {}
}

impl ShareAcrossThreads for String {
    fn share_across_threads(&self) {}
}

/// These tests also run under Miri (`cargo +nightly miri test -p guion-model -- shared::tests`).
#[cfg(test)]
mod tests {
    use super::{ShareAcrossThreads, Shared};

    #[test]
    fn counts_plainly_while_held_by_one_thread() {
        let list = Shared::new(vec![1u8, 2, 3]);
        let copy = list.clone();
        assert_eq!(Shared::strong_count(&list), 2);
        assert!(Shared::ptr_eq(&list, &copy));
        drop(copy);
        assert_eq!(Shared::strong_count(&list), 1);
        assert_eq!(Shared::try_unwrap(list), Ok(vec![1, 2, 3]));
    }

    #[test]
    fn make_mut_copies_only_when_shared() {
        let mut list = Shared::new(vec![1u8]);
        let before = Shared::as_ptr(&list);
        Shared::make_mut(&mut list).push(2);
        assert_eq!(Shared::as_ptr(&list), before);
        let copy = list.clone();
        Shared::make_mut(&mut list).push(3);
        assert_eq!(*copy, vec![1, 2]);
        assert_eq!(*list, vec![1, 2, 3]);
        assert!(Shared::get_mut(&mut list).is_some());
    }

    #[test]
    fn shared_values_cross_threads_and_return_to_plain_counting() {
        let inner = Shared::new(vec![7u8; 4]);
        let mut outer = Shared::new(vec![inner.clone(), inner]);
        outer.share_across_threads();
        let workers: Vec<_> = (0..4)
            .map(|_| {
                let outer = outer.clone();
                std::thread::spawn(move || {
                    let copies: Vec<_> = (0..50).map(|_| outer[0].clone()).collect();
                    copies.iter().all(|copy| **copy == [7; 4])
                })
            })
            .collect();
        for worker in workers {
            assert!(worker.join().unwrap());
        }
        assert_eq!(Shared::strong_count(&outer), 1);
        assert_eq!(Shared::strong_count(&outer[0]), 2);
        // Held in one place again, the outer value is writable and counts plainly.
        let items = Shared::get_mut(&mut outer).expect("only reference");
        items.push(Shared::new(vec![1]));
        let copy = outer.clone();
        assert_eq!(Shared::strong_count(&copy), 2);
    }

    // Immortal values are leaked by design, which Miri reports.
    #[test]
    #[cfg_attr(miri, ignore)]
    fn immortal_values_are_never_counted_or_written() {
        let mut name = Shared::immortal(String::from("Row"));
        let copy = name.clone();
        assert_eq!(Shared::strong_count(&name), usize::MAX);
        assert!(Shared::get_mut(&mut name).is_none());
        Shared::make_mut(&mut name).push('s');
        assert_eq!(*name, "Rows");
        assert_eq!(*copy, "Row");
        let copies: Vec<_> = (0..2)
            .map(|_| {
                let copy = copy.clone();
                std::thread::spawn(move || copy.len()).join().unwrap()
            })
            .collect();
        assert_eq!(copies, [3, 3]);
    }

    #[cfg(debug_assertions)]
    #[test]
    #[cfg_attr(miri, ignore)]
    #[should_panic(expected = "not shared with")]
    fn debug_builds_catch_values_that_cross_threads_unshared() {
        let list = Shared::new(vec![1u8]);
        let copy = list.clone();
        let result = std::thread::spawn(move || drop(copy)).join();
        std::panic::resume_unwind(result.unwrap_err());
    }
}
