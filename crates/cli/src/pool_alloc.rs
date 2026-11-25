//! Small-object pool allocator for the `guion` binary.
//!
//! Guion values are built from many short-lived small `Shared`/`String`/`Vec`
//! allocations. This allocator serves requests of up to `MAX_SMALL` bytes from
//! thread-local size-class free lists carved out of large chunks, and forwards
//! everything else to the system allocator. Chunks are never returned to the
//! operating system, so a block may safely be freed by a thread other than the
//! one that allocated it. A thread whose free list grows past `SPILL_LIMIT`
//! hands the excess to a shared list so producer/consumer threads cannot grow
//! memory without bound. A thread that exits hands its free lists and the unused rest of its
//! bump region to the shared state, so the next thread reuses them instead of taking a new chunk.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::UnsafeCell;
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// Bytes this allocator holds from the system: every small-object chunk (chunks are never
/// returned) plus the large blocks currently allocated. Updated once per chunk or large block,
/// so small allocations stay free of shared atomics.
static HELD_BYTES: AtomicUsize = AtomicUsize::new(0);

/// The memory the program holds, for the runtime's allocation limit.
pub fn held_bytes() -> usize {
    HELD_BYTES.load(Ordering::Relaxed)
}

fn note_large(block: *mut u8, bytes: usize) -> *mut u8 {
    if !block.is_null() {
        HELD_BYTES.fetch_add(bytes, Ordering::Relaxed);
    }
    block
}

const GRANULE: usize = 16;
const MAX_SMALL: usize = 512;
const CLASSES: usize = MAX_SMALL / GRANULE;
const CHUNK_BYTES: usize = 256 * 1024;
const SPILL_LIMIT: usize = 16 * 1024;
/// The smallest unused region worth handing on: it must hold its two-word header.
const MIN_REGION: usize = 2 * GRANULE;
/// The free-list length a thread reports once its lists have been handed on. Every later free
/// on that thread exceeds `SPILL_LIMIT` and goes straight to the shared lists.
const RETIRED_COUNT: usize = usize::MAX / 2;

struct Local {
    heads: [*mut u8; CLASSES],
    counts: [usize; CLASSES],
    bump: *mut u8,
    bump_end: *mut u8,
    /// The exit guard has been registered (set before registering, so registering, which may
    /// allocate, does not recurse).
    guarded: bool,
    /// The thread's lists have been handed on; it only uses the shared state from now on.
    retired: bool,
}

impl Local {
    const EMPTY: Local = Local {
        heads: [ptr::null_mut(); CLASSES],
        counts: [0; CLASSES],
        bump: ptr::null_mut(),
        bump_end: ptr::null_mut(),
        guarded: false,
        retired: false,
    };
}

thread_local! {
    // No destructor: the storage stays valid for the whole of thread teardown, so an allocation
    // or free made by a later destructor can still reach it.
    static LOCAL: UnsafeCell<Local> = const { UnsafeCell::new(Local::EMPTY) };
    static EXIT_GUARD: ExitGuard = const { ExitGuard };
}

/// Hands the thread's memory to the shared state when the thread exits.
struct ExitGuard;

impl Drop for ExitGuard {
    fn drop(&mut self) {
        // SAFETY: this is the exiting thread's own storage and no allocator call is in progress
        // on it; destructors run between allocator calls.
        LOCAL.with(|local| unsafe { retire(&mut *local.get()) });
    }
}

/// Registers the exit guard. Registering a thread-local destructor can allocate, so this runs
/// while no borrow of the lists is live.
#[cold]
fn register_exit_guard() {
    let _ = EXIT_GUARD.try_with(|_| ());
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
// same pointer inherits the lists as the old one left them. The owner never registers an exit
// guard, so its lists are never handed on and a reused pointer never meets a retired `Local`.
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
unsafe impl Sync for OwnedLocal {}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
static OWNED: OwnedLocal = OwnedLocal(UnsafeCell::new(Local::EMPTY));

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
static OWNER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

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
    let local = thread_lists();
    // SAFETY: `thread_lists` returns this thread's storage, which outlives every allocator call
    // on the thread, and only this thread reaches it. The allocator never re-enters itself
    // while this borrow is live.
    action(unsafe { &mut *local })
}

/// This thread's lists, registering the exit guard on first use. `LOCAL` has no destructor, so
/// the returned pointer stays valid for the rest of the thread, teardown included. The guard is
/// registered while no borrow of the lists is live, because registering may allocate.
#[inline(never)]
fn thread_lists() -> *mut Local {
    let local = LOCAL.with(|local| local.get());
    // SAFETY: only this thread reaches its storage, and no other borrow of it is live here.
    if !unsafe { (*local).guarded } {
        unsafe { (*local).guarded = true };
        register_exit_guard();
    }
    local
}

struct SharedState {
    heads: [*mut u8; CLASSES],
    /// A stack of unused bump regions left by exited threads. Each region starts with a header
    /// of two words: the next region and the address one past its end.
    regions: *mut u8,
}

struct Shared {
    lock: AtomicBool,
    state: UnsafeCell<SharedState>,
}

// SAFETY: `state` is only touched while `lock` is held.
unsafe impl Sync for Shared {}

static SHARED: Shared = Shared {
    lock: AtomicBool::new(false),
    state: UnsafeCell::new(SharedState {
        heads: [ptr::null_mut(); CLASSES],
        regions: ptr::null_mut(),
    }),
};

impl Shared {
    fn with<R>(&self, action: impl FnOnce(&mut SharedState) -> R) -> R {
        while self
            .lock
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            std::hint::spin_loop();
        }
        // SAFETY: the spin lock gives exclusive access.
        let result = action(unsafe { &mut *self.state.get() });
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
    if local.retired {
        // A retired thread keeps nothing: the one block just freed goes to the shared list.
        let block = local.heads[class];
        local.heads[class] = ptr::null_mut();
        local.counts[class] = RETIRED_COUNT;
        SHARED.with(|shared| {
            unsafe { block.cast::<*mut u8>().write(shared.heads[class]) };
            shared.heads[class] = block;
        });
        return;
    }
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
    SHARED.with(|shared| {
        unsafe { tail.cast::<*mut u8>().write(shared.heads[class]) };
        shared.heads[class] = detached;
    });
}

/// Writes the header of an unused region `[start, end)` and pushes it on the shared stack.
/// The caller holds the shared lock and has checked the region is at least `MIN_REGION` long.
unsafe fn push_region(shared: &mut SharedState, start: *mut u8, end: *mut u8) {
    let header = start.cast::<*mut u8>();
    unsafe {
        header.write(shared.regions);
        header.add(1).write(end);
    }
    shared.regions = start;
}

/// Pops a shared region of at least `size` bytes, dropping any smaller ones on the way (each is
/// under `MAX_SMALL`). Returns its start and end.
unsafe fn pop_region(shared: &mut SharedState, size: usize) -> Option<(*mut u8, *mut u8)> {
    while !shared.regions.is_null() {
        let start = shared.regions;
        let header = start.cast::<*mut u8>();
        let (next, end) = unsafe { (header.read(), header.add(1).read()) };
        shared.regions = next;
        if (end as usize) - (start as usize) >= size {
            return Some((start, end));
        }
    }
    None
}

/// Hands the exiting thread's free lists and unused bump region to the shared state, and sends
/// every later allocation or free on this thread straight there.
unsafe fn retire(local: &mut Local) {
    local.retired = true;
    let mut lists = [(ptr::null_mut::<u8>(), ptr::null_mut::<u8>()); CLASSES];
    for class in 0..CLASSES {
        let head = local.heads[class];
        if !head.is_null() {
            let mut tail = head;
            loop {
                let next = unsafe { tail.cast::<*mut u8>().read() };
                if next.is_null() {
                    break;
                }
                tail = next;
            }
            lists[class] = (head, tail);
        }
        local.heads[class] = ptr::null_mut();
        local.counts[class] = RETIRED_COUNT;
    }
    let (start, end) = (local.bump, local.bump_end);
    local.bump = ptr::null_mut();
    local.bump_end = ptr::null_mut();
    SHARED.with(|shared| {
        for (class, &(head, tail)) in lists.iter().enumerate() {
            if !head.is_null() {
                unsafe { tail.cast::<*mut u8>().write(shared.heads[class]) };
                shared.heads[class] = head;
            }
        }
        if (end as usize) - (start as usize) >= MIN_REGION {
            unsafe { push_region(shared, start, end) };
        }
    });
}

/// An allocation for a retired thread: a shared free block, else a shared region, else a new
/// chunk held as a shared region.
#[cold]
unsafe fn shared_alloc(class: usize) -> *mut u8 {
    let size = (class + 1) * GRANULE;
    let fresh = SHARED.with(|shared| {
        let block = shared.heads[class];
        if !block.is_null() {
            shared.heads[class] = unsafe { block.cast::<*mut u8>().read() };
            return Some(block);
        }
        if let Some((start, end)) = unsafe { pop_region(shared, size) } {
            let rest = unsafe { start.add(size) };
            if (end as usize) - (rest as usize) >= MIN_REGION {
                unsafe { push_region(shared, rest, end) };
            }
            return Some(start);
        }
        None
    });
    if fresh.is_some() {
        return fresh.unwrap_or(ptr::null_mut());
    }
    let chunk = unsafe { System.alloc(Layout::from_size_align_unchecked(CHUNK_BYTES, GRANULE)) };
    if chunk.is_null() {
        return ptr::null_mut();
    }
    HELD_BYTES.fetch_add(CHUNK_BYTES, Ordering::Relaxed);
    SHARED.with(|shared| unsafe {
        push_region(shared, chunk.add(size), chunk.add(CHUNK_BYTES));
    });
    chunk
}

#[cold]
unsafe fn refill(local: &mut Local, class: usize) -> *mut u8 {
    if local.retired {
        return unsafe { shared_alloc(class) };
    }
    let adopted =
        SHARED.with(|shared| std::mem::replace(&mut shared.heads[class], ptr::null_mut()));
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
        // Prefer the unused region a finished thread left over to a new chunk.
        if let Some((start, end)) = SHARED.with(|shared| unsafe { pop_region(shared, size) }) {
            local.bump = start;
            local.bump_end = end;
        } else {
            let chunk =
                unsafe { System.alloc(Layout::from_size_align_unchecked(CHUNK_BYTES, GRANULE)) };
            if chunk.is_null() {
                return ptr::null_mut();
            }
            HELD_BYTES.fetch_add(CHUNK_BYTES, Ordering::Relaxed);
            local.bump = chunk;
            local.bump_end = unsafe { chunk.add(CHUNK_BYTES) };
        }
    }
    let block = local.bump;
    local.bump = unsafe { block.add(size) };
    block
}

unsafe impl GlobalAlloc for PoolAllocator {
    #[inline]
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if !is_small(&layout) {
            return note_large(unsafe { System.alloc(layout) }, layout.size());
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
            HELD_BYTES.fetch_sub(layout.size(), Ordering::Relaxed);
            return unsafe { System.dealloc(block, layout) };
        }
        let class = class_of(layout.size());
        // SAFETY: `block` is a free block of this class.
        with_local(|local| unsafe { push(local, class, block) });
    }

    #[inline]
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if !is_small(&layout) {
            return note_large(unsafe { System.alloc_zeroed(layout) }, layout.size());
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
            (false, false) => {
                let moved = unsafe { System.realloc(block, layout, new_size) };
                if !moved.is_null() {
                    HELD_BYTES.fetch_add(new_size, Ordering::Relaxed);
                    HELD_BYTES.fetch_sub(layout.size(), Ordering::Relaxed);
                }
                moved
            }
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

#[cfg(test)]
mod tests {
    use super::{held_bytes, CHUNK_BYTES};

    /// `held_bytes` is process-wide and the other tests allocate at the same time, so each test
    /// here re-runs itself alone in a child process and measures there.
    fn isolated(name: &str) -> bool {
        if std::env::var_os("GUION_POOL_ALLOC_ISOLATED").is_some() {
            return true;
        }
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", name, "--test-threads=1", "--nocapture"])
            .env("GUION_POOL_ALLOC_ISOLATED", "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        print!("{}", String::from_utf8_lossy(&output.stdout));
        false
    }

    #[test]
    fn a_finished_threads_chunk_is_reused() {
        if !isolated("pool_alloc::tests::a_finished_threads_chunk_is_reused") {
            return;
        }
        // Warm up so the first thread's own chunk is already counted.
        std::thread::spawn(|| drop(std::hint::black_box(vec![1u8; 64])))
            .join()
            .unwrap();
        for keep in [false, true] {
            let before = held_bytes();
            let mut kept = Vec::new();
            for _ in 0..200 {
                let block = std::thread::spawn(|| std::hint::black_box(vec![7u8; 64]))
                    .join()
                    .unwrap();
                if keep {
                    kept.push(block);
                }
            }
            let grown = held_bytes().saturating_sub(before);
            println!("keep={keep}: held bytes grew by {grown}");
            // Kept blocks stay live, so only the chunks, not the blocks, are bounded: 200 kept
            // 64-byte blocks fit in one chunk.
            assert!(
                grown <= 4 * CHUNK_BYTES,
                "keep={keep}: 200 finished threads grew held bytes by {grown}"
            );
            assert!(kept.iter().all(|block| block.iter().all(|&byte| byte == 7)));
        }
    }

    #[test]
    fn memory_freed_on_another_thread_after_its_owner_exited_is_reusable() {
        if !isolated(
            "pool_alloc::tests::memory_freed_on_another_thread_after_its_owner_exited_is_reusable",
        ) {
            return;
        }
        let made = std::thread::spawn(|| {
            (0..1_000u64)
                .map(|i| vec![i; 1 + (i % 30) as usize])
                .collect::<Vec<_>>()
        })
        .join()
        .unwrap();
        // The thread that made these blocks is gone; free them here and from a third thread,
        // then allocate again and check nothing was damaged.
        let (here, there) = made.split_at(500);
        let there = there.to_vec();
        std::thread::spawn(move || drop(there)).join().unwrap();
        for (i, block) in here.iter().enumerate() {
            assert!(block.iter().all(|&value| value == i as u64));
        }
        drop(made);
        let before = held_bytes();
        for round in 0..50u64 {
            let again: Vec<Vec<u64>> = (0..1_000u64)
                .map(|i| vec![i + round; 1 + (i % 30) as usize])
                .collect();
            for (i, block) in again.iter().enumerate() {
                assert!(block.iter().all(|&value| value == i as u64 + round));
            }
        }
        assert!(held_bytes().saturating_sub(before) <= 4 * CHUNK_BYTES);
    }
}
