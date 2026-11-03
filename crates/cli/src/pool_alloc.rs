//! Small-object pool allocator for the `guion` binary.
//!
//! Guion values are built from many short-lived small `Arc`/`String`/`Vec`
//! allocations. This allocator serves requests of up to `MAX_SMALL` bytes from
//! thread-local size-class free lists carved out of large chunks, and forwards
//! everything else to the system allocator. Chunks are never returned to the
//! operating system, so a block may safely be freed by a thread other than the
//! one that allocated it. A thread whose free list grows past `SPILL_LIMIT`
//! hands the excess to a shared list so producer/consumer threads cannot grow
//! memory without bound.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::UnsafeCell;
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

const GRANULE: usize = 16;
const MAX_SMALL: usize = 512;
const CLASSES: usize = MAX_SMALL / GRANULE;
const CHUNK_BYTES: usize = 256 * 1024;
const SPILL_LIMIT: usize = 16 * 1024;

struct Local {
    heads: [*mut u8; CLASSES],
    counts: [usize; CLASSES],
    bump: *mut u8,
    bump_end: *mut u8,
}

thread_local! {
    static LOCAL: UnsafeCell<Local> = const {
        UnsafeCell::new(Local {
            heads: [ptr::null_mut(); CLASSES],
            counts: [0; CLASSES],
            bump: ptr::null_mut(),
            bump_end: ptr::null_mut(),
        })
    };
}

/// The free lists of the thread that claimed them with its thread pointer. On macOS every
/// `thread_local!` access is a call into dyld, which costs a measurable share of an
/// allocation-heavy run; the claiming thread (the main thread, which allocates first)
/// reaches its lists with one register read instead.
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
struct OwnedLocal(UnsafeCell<Local>);

// SAFETY: only the live thread whose thread pointer is stored in `OWNER` touches the lists,
// and a thread pointer identifies one live thread. A thread pointer is reused only after its
// thread has exited, and thread teardown and creation synchronise, so a new thread with the
// same pointer inherits the lists as the old one left them.
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
unsafe impl Sync for OwnedLocal {}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
static OWNED: OwnedLocal = OwnedLocal(UnsafeCell::new(Local {
    heads: [ptr::null_mut(); CLASSES],
    counts: [0; CLASSES],
    bump: ptr::null_mut(),
    bump_end: ptr::null_mut(),
}));

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
static OWNER: AtomicUsize = AtomicUsize::new(0);

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[inline(always)]
fn thread_pointer() -> usize {
    let pointer: usize;
    // SAFETY: reading the thread pointer register has no side effects.
    unsafe {
        std::arch::asm!("mrs {}, tpidrro_el0", out(reg) pointer, options(nomem, nostack, preserves_flags))
    };
    pointer
}

/// Run `action` on the calling thread's free lists.
#[inline(always)]
fn with_local<R>(action: impl FnOnce(&mut Local) -> R) -> R {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        let thread = thread_pointer();
        let owner = OWNER.load(Ordering::Relaxed);
        if owner == thread
            || (owner == 0
                && OWNER
                    .compare_exchange(0, thread, Ordering::Relaxed, Ordering::Relaxed)
                    .is_ok())
        {
            // SAFETY: as for `OwnedLocal`; the allocator never re-enters itself.
            return action(unsafe { &mut *OWNED.0.get() });
        }
    }
    // SAFETY: the allocator never re-enters itself while this borrow is live.
    LOCAL.with(|local| action(unsafe { &mut *local.get() }))
}

struct Shared {
    lock: AtomicBool,
    heads: UnsafeCell<[*mut u8; CLASSES]>,
}

// SAFETY: `heads` is only touched while `lock` is held.
unsafe impl Sync for Shared {}

static SHARED: Shared = Shared {
    lock: AtomicBool::new(false),
    heads: UnsafeCell::new([ptr::null_mut(); CLASSES]),
};

impl Shared {
    fn with<R>(&self, action: impl FnOnce(&mut [*mut u8; CLASSES]) -> R) -> R {
        while self
            .lock
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            std::hint::spin_loop();
        }
        // SAFETY: the spin lock gives exclusive access.
        let result = action(unsafe { &mut *self.heads.get() });
        self.lock.store(false, Ordering::Release);
        result
    }
}

pub struct PoolAllocator;

#[inline]
fn class_of(size: usize) -> usize {
    size.max(1).div_ceil(GRANULE) - 1
}

#[inline]
fn is_small(layout: &Layout) -> bool {
    layout.size() <= MAX_SMALL && layout.align() <= GRANULE
}

/// Every free block stores the address of the next free block in its first word.
#[inline]
unsafe fn push(local: &mut Local, class: usize, block: *mut u8) {
    unsafe { block.cast::<*mut u8>().write(local.heads[class]) };
    local.heads[class] = block;
    local.counts[class] += 1;
    if local.counts[class] > SPILL_LIMIT {
        unsafe { spill(local, class) };
    }
}

#[cold]
unsafe fn spill(local: &mut Local, class: usize) {
    let keep = SPILL_LIMIT / 2;
    let mut cursor = local.heads[class];
    for _ in 1..keep {
        cursor = unsafe { cursor.cast::<*mut u8>().read() };
    }
    let detached = unsafe { cursor.cast::<*mut u8>().read() };
    unsafe { cursor.cast::<*mut u8>().write(ptr::null_mut()) };
    local.counts[class] = keep;
    let mut tail = detached;
    loop {
        let next = unsafe { tail.cast::<*mut u8>().read() };
        if next.is_null() {
            break;
        }
        tail = next;
    }
    SHARED.with(|heads| {
        unsafe { tail.cast::<*mut u8>().write(heads[class]) };
        heads[class] = detached;
    });
}

#[cold]
unsafe fn refill(local: &mut Local, class: usize) -> *mut u8 {
    let adopted = SHARED.with(|heads| std::mem::replace(&mut heads[class], ptr::null_mut()));
    if !adopted.is_null() {
        let mut count = 0usize;
        let mut cursor = adopted;
        while !cursor.is_null() {
            count += 1;
            cursor = unsafe { cursor.cast::<*mut u8>().read() };
        }
        local.heads[class] = unsafe { adopted.cast::<*mut u8>().read() };
        local.counts[class] = count - 1;
        return adopted;
    }
    let size = (class + 1) * GRANULE;
    if (local.bump_end as usize) - (local.bump as usize) < size {
        let chunk =
            unsafe { System.alloc(Layout::from_size_align_unchecked(CHUNK_BYTES, GRANULE)) };
        if chunk.is_null() {
            return ptr::null_mut();
        }
        local.bump = chunk;
        local.bump_end = unsafe { chunk.add(CHUNK_BYTES) };
    }
    let block = local.bump;
    local.bump = unsafe { block.add(size) };
    block
}

unsafe impl GlobalAlloc for PoolAllocator {
    #[inline]
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if !is_small(&layout) {
            return unsafe { System.alloc(layout) };
        }
        let class = class_of(layout.size());
        with_local(|local| {
            let head = local.heads[class];
            if head.is_null() {
                unsafe { refill(local, class) }
            } else {
                local.heads[class] = unsafe { head.cast::<*mut u8>().read() };
                local.counts[class] -= 1;
                head
            }
        })
    }

    #[inline]
    unsafe fn dealloc(&self, block: *mut u8, layout: Layout) {
        if !is_small(&layout) {
            return unsafe { System.dealloc(block, layout) };
        }
        let class = class_of(layout.size());
        // SAFETY: `block` is a free block of this class.
        with_local(|local| unsafe { push(local, class, block) });
    }

    #[inline]
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if !is_small(&layout) {
            return unsafe { System.alloc_zeroed(layout) };
        }
        let block = unsafe { self.alloc(layout) };
        if !block.is_null() {
            unsafe { ptr::write_bytes(block, 0, layout.size()) };
        }
        block
    }

    #[inline]
    unsafe fn realloc(&self, block: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new_layout = unsafe { Layout::from_size_align_unchecked(new_size, layout.align()) };
        match (is_small(&layout), is_small(&new_layout)) {
            (false, false) => unsafe { System.realloc(block, layout, new_size) },
            (true, true) if class_of(layout.size()) == class_of(new_size) => block,
            _ => {
                let replacement = unsafe { self.alloc(new_layout) };
                if !replacement.is_null() {
                    unsafe {
                        ptr::copy_nonoverlapping(block, replacement, layout.size().min(new_size));
                        self.dealloc(block, layout);
                    }
                }
                replacement
            }
        }
    }
}
