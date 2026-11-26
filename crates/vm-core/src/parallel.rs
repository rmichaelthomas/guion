//! One pool of threads for the element-wise array kernels, matrix multiplication and the
//! parallel CSV read.
//!
//! `run_on_pool` runs a work-pulling closure on the caller and on the pool's threads.
//! A kernel hands `for_each_chunk_mut` its output buffer; disjoint chunks of it are filled on
//! the threads of a persistent pool. The result is the one a single thread would give: each chunk reports the
//! first failure inside it in index order, and the failure of the lowest-offset chunk wins.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex, OnceLock};

/// Elements below which one thread does the work. Measured on an Apple M1 Pro (a loop of
/// `array_add(array_mul(a, b), c)`): 2^16 beat one thread from 66,000 elements up (206 against
/// 307 ms at 66,000; 160 against 258 ms at 100,000), where 2^18 and 2^20 left those sizes on
/// one thread.
pub(crate) const PARALLEL_MIN_ELEMENTS: usize = 1 << 16;

/// The smallest chunk handed to a worker, so a chunk always outweighs the cost of fetching it.
const MIN_CHUNK: usize = 1 << 16;

#[cfg(test)]
thread_local! {
    static FORCED_THREADS: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
    static OFF_THREAD_CHUNKS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static LAST_JOINING: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static NO_NEW_THREADS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Runs `body` with `threads_for_elements` answering `threads` on this thread.
#[cfg(test)]
pub(crate) fn with_forced_threads<R>(threads: usize, body: impl FnOnce() -> R) -> R {
    let before = FORCED_THREADS.with(|forced| forced.replace(Some(threads)));
    let result = body();
    FORCED_THREADS.with(|forced| forced.set(before));
    result
}

/// How many chunks calls made from this thread have had run by other threads.
#[cfg(test)]
pub(crate) fn off_thread_chunks() -> usize {
    OFF_THREAD_CHUNKS.with(|chunks| chunks.get())
}

/// How many pool workers the latest call from this thread asked to join.
#[cfg(test)]
fn last_joining() -> usize {
    LAST_JOINING.with(|joining| joining.get())
}

/// 1 below the threshold and on wasm32; otherwise `available_parallelism`.
pub(crate) fn threads_for_elements(len: usize) -> usize {
    #[cfg(test)]
    if let Some(forced) = FORCED_THREADS.with(|forced| forced.get()) {
        return forced;
    }
    if len < PARALLEL_MIN_ELEMENTS {
        return 1;
    }
    available_threads()
}

#[cfg(target_arch = "wasm32")]
fn available_threads() -> usize {
    1
}

#[cfg(not(target_arch = "wasm32"))]
fn available_threads() -> usize {
    std::thread::available_parallelism().map_or(1, |count| count.get())
}

#[cfg(test)]
fn refuse_spawn() -> bool {
    NO_NEW_THREADS.with(|refuse| refuse.get())
}

/// Runs `body` with the pool's workers unavailable to calls made from this thread, as if the
/// operating system had refused to start them: the caller does all the work.
#[cfg(test)]
pub(crate) fn with_no_pool_workers<R>(body: impl FnOnce() -> R) -> R {
    let before = NO_NEW_THREADS.with(|refuse| refuse.replace(true));
    let result = body();
    NO_NEW_THREADS.with(|refuse| refuse.set(before));
    result
}

#[cfg(not(test))]
fn refuse_spawn() -> bool {
    false
}

/// A job on loan to the pool: the address of a closure that stays alive until every worker
/// that claimed it has finished.
#[derive(Clone, Copy)]
struct Job(*const (dyn Fn() + Sync));

// SAFETY: the closure behind the pointer is `Sync`, and `run_on_pool` keeps it alive until
// every worker that took the job has finished with it.
unsafe impl Send for Job {}

struct State {
    job: Option<Job>,
    /// Workers that may still claim the current job.
    unclaimed: usize,
    /// Workers that claimed the current job and have not finished it.
    pending: usize,
    panicked: bool,
}

struct Pool {
    state: Mutex<State>,
    wake: Condvar,
    done: Condvar,
    /// How many workers started, which bounds how many a job can use.
    workers: AtomicUsize,
    /// Held by the one job in flight; a second caller does its own work instead of waiting.
    busy: AtomicBool,
}

/// The pool starts once and its threads live for the process. Starting threads per call
/// would cost more than small kernels save, and every short-lived thread would strand the
/// small-object chunk the `guion` binary's allocator hands it.
fn pool() -> &'static Pool {
    static POOL: OnceLock<&'static Pool> = OnceLock::new();
    POOL.get_or_init(|| {
        let pool: &'static Pool = Box::leak(Box::new(Pool {
            state: Mutex::new(State {
                job: None,
                unclaimed: 0,
                pending: 0,
                panicked: false,
            }),
            wake: Condvar::new(),
            done: Condvar::new(),
            workers: AtomicUsize::new(0),
            busy: AtomicBool::new(false),
        }));
        // A thread the operating system refuses to start only means fewer workers.
        for _ in 1..available_threads() {
            let started = std::thread::Builder::new()
                .name("guion-array".into())
                .spawn(move || serve(pool));
            if started.is_err() {
                break;
            }
            pool.workers.fetch_add(1, Ordering::Relaxed);
        }
        pool
    })
}

/// A worker: claims a slot of the job in flight, runs it, reports back, and waits again.
fn serve(pool: &'static Pool) {
    let mut state = pool.state.lock().unwrap_or_else(|e| e.into_inner());
    loop {
        state = pool
            .wake
            .wait_while(state, |state| state.unclaimed == 0)
            .unwrap_or_else(|e| e.into_inner());
        state.unclaimed -= 1;
        let job = state.job.expect("a job is on loan while slots are open");
        drop(state);
        // SAFETY: `run_on_pool` does not return, and so does not free the closure, until
        // `pending` reaches zero, and this worker counts in `pending` until it reports below.
        let outcome =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe { (*job.0)() }));
        state = pool.state.lock().unwrap_or_else(|e| e.into_inner());
        state.panicked |= outcome.is_err();
        state.pending -= 1;
        if state.pending == 0 {
            pool.done.notify_all();
        }
    }
}

/// Runs `worker` on the calling thread and on up to `threads - 1` pool threads, and returns
/// when every invocation has returned. The closure pulls its work from a shared queue or
/// counter, so how many threads join never changes the result. If the pool is busy (a nested
/// call, or another caller's job) or has no workers, `worker` runs once, on the caller only.
/// A panic in any invocation is re-raised on the caller.
pub fn run_on_pool(threads: usize, worker: &(dyn Fn() + Sync)) {
    let pool = pool();
    let available = if refuse_spawn() {
        0
    } else {
        pool.workers.load(Ordering::Relaxed)
    };
    let joining = threads.saturating_sub(1).min(available);
    #[cfg(test)]
    LAST_JOINING.with(|last| last.set(joining));
    if joining == 0 || pool.busy.swap(true, Ordering::Acquire) {
        return worker();
    }
    // SAFETY: only the lifetime is erased. The closure is not dropped or moved before the wait
    // below sees `pending == 0` and clears the loan, and that wait runs even if this thread's
    // own share panics.
    let job = Job(unsafe {
        std::mem::transmute::<*const (dyn Fn() + Sync + '_), *const (dyn Fn() + Sync + 'static)>(
            worker,
        )
    });
    {
        let mut state = pool.state.lock().unwrap_or_else(|e| e.into_inner());
        state.job = Some(job);
        state.unclaimed = joining;
        state.pending = joining;
        state.panicked = false;
    }
    pool.wake.notify_all();
    let own = std::panic::catch_unwind(std::panic::AssertUnwindSafe(worker));
    let mut state = pool.state.lock().unwrap_or_else(|e| e.into_inner());
    state = pool
        .done
        .wait_while(state, |state| state.pending > 0)
        .unwrap_or_else(|e| e.into_inner());
    state.job = None;
    let helper_panicked = state.panicked;
    drop(state);
    pool.busy.store(false, Ordering::Release);
    if let Err(payload) = own {
        std::panic::resume_unwind(payload);
    }
    assert!(!helper_panicked, "an array worker thread panicked");
}

/// Runs `work(offset, chunk)` over disjoint chunks of `out` on the pool's threads and the
/// calling thread, and returns the result of the lowest-offset chunk that failed. No workers
/// (none started, or the pool busy) means the caller does every chunk.
pub(crate) fn for_each_chunk_mut<T: Send, E: Send>(
    out: &mut [T],
    threads: usize,
    work: impl Fn(usize, &mut [T]) -> Result<(), E> + Sync,
) -> Result<(), E> {
    let len = out.len();
    if threads <= 1 || len <= MIN_CHUNK {
        return work(0, out);
    }
    let size = len.div_ceil(threads * 4).max(MIN_CHUNK);
    let queue = Mutex::new(out.chunks_mut(size).enumerate());
    let failure: Mutex<Option<(usize, E)>> = Mutex::new(None);
    // The lowest offset known to have failed. A chunk past it cannot change the answer, so it is
    // not run; a chunk before it still is, because it might fail at a lower offset.
    let stop_at = AtomicUsize::new(usize::MAX);
    #[cfg(test)]
    let (caller, off_thread) = (std::thread::current().id(), AtomicUsize::new(0));
    let worker = || loop {
        let next = queue.lock().unwrap_or_else(|e| e.into_inner()).next();
        let Some((index, chunk)) = next else { return };
        let offset = index * size;
        if offset > stop_at.load(Ordering::Acquire) {
            return;
        }
        #[cfg(test)]
        if std::thread::current().id() != caller {
            off_thread.fetch_add(1, Ordering::Relaxed);
        }
        if let Err(error) = work(offset, chunk) {
            let mut slot = failure.lock().unwrap_or_else(|e| e.into_inner());
            if slot.as_ref().is_none_or(|(known, _)| offset < *known) {
                *slot = Some((offset, error));
                stop_at.store(offset, Ordering::Release);
            }
        }
    };
    // More workers than chunks would only be woken to find the queue empty.
    run_on_pool(threads.min(len.div_ceil(size)), &worker);
    #[cfg(test)]
    OFF_THREAD_CHUNKS.with(|chunks| chunks.set(chunks.get() + off_thread.load(Ordering::Relaxed)));
    match failure.into_inner().unwrap_or_else(|e| e.into_inner()) {
        Some((_, error)) => Err(error),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn for_each_chunk_mut_reports_the_lowest_failing_offset() {
        let mut data = vec![0u32; 1_000_000];
        for _ in 0..20 {
            let result = for_each_chunk_mut(&mut data, 8, |offset, chunk| {
                for (i, slot) in chunk.iter_mut().enumerate() {
                    let index = offset + i;
                    if index == 3 || index == 900_000 {
                        return Err(index);
                    }
                    *slot = 1;
                }
                Ok(())
            });
            assert_eq!(result, Err(3));
        }
    }

    #[test]
    fn every_element_is_visited_once_and_a_refused_spawn_still_finishes() {
        for refuse in [false, true] {
            NO_NEW_THREADS.with(|flag| flag.set(refuse));
            let mut data = vec![0u32; 700_001];
            let result: Result<(), ()> = for_each_chunk_mut(&mut data, 8, |offset, chunk| {
                for (i, slot) in chunk.iter_mut().enumerate() {
                    *slot += (offset + i) as u32 + 1;
                }
                Ok(())
            });
            NO_NEW_THREADS.with(|flag| flag.set(false));
            assert_eq!(result, Ok(()));
            assert!(data.iter().enumerate().all(|(i, v)| *v == i as u32 + 1));
        }
    }

    #[test]
    fn the_pool_serves_many_calls_and_concurrent_callers() {
        let callers: Vec<_> = (0..4)
            .map(|_| {
                std::thread::spawn(|| {
                    for _ in 0..200 {
                        let mut data = vec![0u8; 300_000];
                        let result: Result<(), ()> =
                            for_each_chunk_mut(&mut data, 4, |_, chunk| {
                                chunk.fill(7);
                                Ok(())
                            });
                        assert_eq!(result, Ok(()));
                        assert!(data.iter().all(|byte| *byte == 7));
                    }
                })
            })
            .collect();
        for caller in callers {
            caller.join().expect("no panic");
        }
    }

    #[test]
    fn a_panicking_chunk_reaches_the_caller_and_leaves_the_pool_usable() {
        let mut data = vec![0u8; 300_000];
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _: Result<(), ()> = for_each_chunk_mut(&mut data, 4, |offset, _| {
                assert!(offset != 65536, "boom");
                Ok(())
            });
        }));
        assert!(outcome.is_err());
        let result: Result<(), ()> = for_each_chunk_mut(&mut data, 4, |_, chunk| {
            chunk.fill(1);
            Ok(())
        });
        assert_eq!(result, Ok(()));
        assert!(data.iter().all(|byte| *byte == 1));
    }

    #[test]
    fn a_two_chunk_call_wakes_at_most_one_worker() {
        let mut data = vec![0u8; MIN_CHUNK + 1000];
        let result: Result<(), ()> = for_each_chunk_mut(&mut data, 8, |_, chunk| {
            chunk.fill(1);
            Ok(())
        });
        assert_eq!(result, Ok(()));
        assert!(last_joining() <= 1, "woke {}", last_joining());
        let mut data = vec![0u8; 8 * 4 * MIN_CHUNK];
        let _: Result<(), ()> = for_each_chunk_mut(&mut data, 8, |_, _| Ok(()));
        assert!(last_joining() > 1 || available_threads() <= 2);
    }

    #[test]
    fn run_on_pool_uses_pool_threads() {
        if available_threads() < 2 {
            return;
        }
        // Another test may hold the pool; a busy pool runs the work on the caller alone, so
        // try again until a call really is shared.
        let mut most = 0;
        for _ in 0..50 {
            let ids = Mutex::new(std::collections::HashSet::new());
            run_on_pool(4, &|| {
                ids.lock().unwrap().insert(std::thread::current().id());
                let start = std::time::Instant::now();
                while ids.lock().unwrap().len() < 4 && start.elapsed().as_millis() < 100 {
                    std::thread::yield_now();
                }
            });
            most = most.max(ids.lock().unwrap().len());
            if most >= 2 {
                break;
            }
        }
        assert!(most >= 2, "only {most} distinct threads ran");
    }

    #[test]
    fn nested_run_on_pool_runs_on_caller() {
        if available_threads() < 2 {
            return;
        }
        for _ in 0..50 {
            let outer = Mutex::new(std::collections::HashSet::new());
            let inner: Mutex<Vec<(std::thread::ThreadId, std::collections::HashSet<_>)>> =
                Mutex::new(Vec::new());
            run_on_pool(4, &|| {
                outer.lock().unwrap().insert(std::thread::current().id());
                let start = std::time::Instant::now();
                while outer.lock().unwrap().len() < 2 && start.elapsed().as_millis() < 100 {
                    std::thread::yield_now();
                }
                let seen = Mutex::new(std::collections::HashSet::new());
                run_on_pool(4, &|| {
                    seen.lock().unwrap().insert(std::thread::current().id());
                });
                inner
                    .lock()
                    .unwrap()
                    .push((std::thread::current().id(), seen.into_inner().unwrap()));
            });
            // Only when the outer call was really shared did it hold the pool throughout.
            if outer.lock().unwrap().len() >= 2 {
                let inner = inner.lock().unwrap();
                assert!(!inner.is_empty());
                for (thread, seen) in inner.iter() {
                    assert_eq!(seen.len(), 1);
                    assert!(seen.contains(thread));
                }
                return;
            }
        }
        panic!("the pool was never free for a shared call");
    }

    #[test]
    fn failed_chunk_stops_remaining_work() {
        const CHUNKS: usize = 400;
        let run = |refuse: bool| {
            let started = AtomicUsize::new(0);
            let failed = AtomicBool::new(false);
            let mut data = vec![0u8; CHUNKS * MIN_CHUNK];
            let mut body = || {
                for_each_chunk_mut(&mut data, 100, |offset, _| {
                    started.fetch_add(1, Ordering::SeqCst);
                    if offset == 0 {
                        failed.store(true, Ordering::SeqCst);
                        return Err(offset);
                    }
                    // Every other chunk starts only once chunk 0 has failed.
                    let start = std::time::Instant::now();
                    while !failed.load(Ordering::SeqCst) && start.elapsed().as_secs() < 2 {
                        std::thread::yield_now();
                    }
                    Ok(())
                })
            };
            let result = if refuse {
                with_no_pool_workers(&mut body)
            } else {
                body()
            };
            (result, started.load(Ordering::SeqCst))
        };
        // The caller alone takes chunk 0 first and fails it: nothing else may start.
        assert_eq!(run(true), (Err(0), 1));
        // With workers, only chunks already taken when the failure lands may start.
        let (result, started) = run(false);
        assert_eq!(result, Err(0));
        assert!(started < CHUNKS / 2, "{started} of {CHUNKS} chunks started");
    }

    #[test]
    fn refused_spawn_still_finishes() {
        let mut data = vec![0u32; 700_001];
        let result: Result<(), ()> = with_no_pool_workers(|| {
            for_each_chunk_mut(&mut data, 8, |offset, chunk| {
                for (i, slot) in chunk.iter_mut().enumerate() {
                    *slot += (offset + i) as u32 + 1;
                }
                Ok(())
            })
        });
        assert_eq!(result, Ok(()));
        assert_eq!(last_joining(), 0);
        assert!(data.iter().enumerate().all(|(i, v)| *v == i as u32 + 1));
    }

    #[test]
    fn small_inputs_use_one_thread() {
        assert_eq!(threads_for_elements(PARALLEL_MIN_ELEMENTS - 1), 1);
    }
}
