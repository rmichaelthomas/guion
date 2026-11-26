//! Packed, register-tiled `f64` matrix multiplication (GotoBLAS structure).
//!
//! `C = A * B` for row-major `A` (n by k), `B` (k by m) and `C` (n by m). `B` is packed once
//! into panels of `NR` columns; row chunks of `A` are packed into panels of `MR` rows; a
//! micro-kernel computes an `MR` by `NR` tile with fused multiply-add. Every output element is
//! summed in the same order whatever the thread count or instruction set: `KC`-sized blocks of
//! the inner dimension in order, each block accumulated sequentially with fused multiply-add
//! starting from zero, then added to `C`. Results are therefore bit-identical across runs,
//! thread counts and the NEON, AVX2 and scalar kernels.

use std::{mem::size_of, sync::Mutex};

pub(crate) const MR: usize = 8;
pub(crate) const NR: usize = 6;
/// Rows of `A` packed at once per worker chunk, and the inner-dimension block length.
const MC: usize = 128;
pub(crate) const KC: usize = 256;
/// Products (n*m*k) below which one thread does the work: 128^3 is faster on one thread and
/// 160^3 on several (M1 Pro), so the cut sits between them.
const PARALLEL_MIN: usize = (1 << 21) + (1 << 20);
/// Packed-`B` size (values) from which packing is shared across the workers; below it the
/// hand-off costs more than it saves (512^3 measured no faster shared, 1024^3 about 3% faster).
const PARALLEL_PACK_MIN: usize = 1 << 19;
/// Panels of `B` a worker packs per queue pull (a single panel is about a microsecond, so
/// pulling one at a time is dominated by the queue).
const PACK_GROUP: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Isa {
    /// The portable kernel; unused outside tests on aarch64, where NEON is always present.
    #[cfg_attr(target_arch = "aarch64", allow(dead_code))]
    Scalar,
    #[cfg(target_arch = "aarch64")]
    Neon,
    #[cfg(target_arch = "x86_64")]
    Avx2,
}

/// The fastest kernel this machine supports.
pub(crate) fn native_isa() -> Isa {
    #[cfg(target_arch = "aarch64")]
    {
        Isa::Neon
    }
    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx2") && is_x86_feature_detected!("fma") {
            Isa::Avx2
        } else {
            Isa::Scalar
        }
    }
    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
    {
        Isa::Scalar
    }
}

/// Workers for a product of the given size.
pub(crate) fn threads_for((n, k, m): (usize, usize, usize)) -> usize {
    if n.saturating_mul(k).saturating_mul(m) < PARALLEL_MIN {
        return 1;
    }
    let available = std::thread::available_parallelism().map_or(1, |count| count.get());
    available.min(n.div_ceil(MR)).max(1)
}

fn chunk_rows(n: usize, threads: usize) -> usize {
    let target = n.div_ceil(threads * 4).next_multiple_of(MR);
    target.clamp(MR, MC)
}

/// Bytes of packing buffers `matmul_f64` allocates: the packed `B` and one packed `A` chunk
/// per worker.
pub(crate) fn scratch_bytes((n, k, m): (usize, usize, usize), threads: usize) -> usize {
    let packed_b = k.saturating_mul(m.next_multiple_of(NR));
    let packed_a = threads
        .saturating_mul(chunk_rows(n, threads.max(1)).min(n.next_multiple_of(MR)))
        .saturating_mul(k.min(KC));
    packed_b
        .saturating_add(packed_a)
        .saturating_mul(size_of::<f64>())
}

/// `c = a * b`, with `c` of length `n * m` (overwritten). `threads` is clamped to the work.
pub(crate) fn matmul_f64(
    a: &[f64],
    b: &[f64],
    c: &mut [f64],
    (n, k, m): (usize, usize, usize),
    threads: usize,
    isa: Isa,
) {
    assert_eq!(a.len(), n * k);
    assert_eq!(b.len(), k * m);
    assert_eq!(c.len(), n * m);
    c.fill(0.0);
    if n == 0 || m == 0 || k == 0 {
        return;
    }
    let mpad = m.next_multiple_of(NR);
    let threads = threads.clamp(1, n.div_ceil(MR));
    let packed_b = pack_b(b, k, m, mpad, threads);
    let rows = chunk_rows(n, threads);
    let queue = Mutex::new(c.chunks_mut(rows * m).enumerate());
    let work = || {
        let mut packed_a = vec![0.0f64; rows.min(n.next_multiple_of(MR)) * k.min(KC)];
        loop {
            let next = queue.lock().unwrap_or_else(|e| e.into_inner()).next();
            let Some((index, chunk)) = next else { return };
            let r0 = index * rows;
            multiply_chunk(a, &packed_b, chunk, &mut packed_a, (r0, k, m, mpad), isa);
        }
    };
    if threads == 1 {
        work();
    } else {
        // The workers pull row chunks from one queue, so the calling thread works too and a
        // pool with fewer free workers (or none, or busy) only means fewer threads, never a
        // missing chunk.
        crate::parallel::run_on_pool(threads, &work);
    }
}

/// Packs `B` as, per `KC` block of rows, panels of `NR` columns, each stored `[p][NR]` and
/// zero-padded past column `m`. The block starting at row `pc` begins at `pc * mpad`. The
/// panels are independent, so the workers pull them from one queue.
fn pack_b(b: &[f64], k: usize, m: usize, mpad: usize, threads: usize) -> Vec<f64> {
    let mut packed = vec![0.0f64; k * mpad];
    let shared = packed.len() >= PARALLEL_PACK_MIN;
    let groups = packed
        .chunks_mut(KC * mpad)
        .enumerate()
        .flat_map(|(block, rows)| {
            let (pc, kc) = (block * KC, rows.len() / mpad);
            rows.chunks_mut(PACK_GROUP * kc * NR)
                .enumerate()
                .map(move |(index, out)| (pc, kc, index * PACK_GROUP * NR, out))
        });
    let queue = Mutex::new(groups);
    let work = || loop {
        let next = queue.lock().unwrap_or_else(|e| e.into_inner()).next();
        let Some((pc, kc, first, out)) = next else {
            return;
        };
        for (panel, out) in out.chunks_exact_mut(kc * NR).enumerate() {
            let j0 = first + panel * NR;
            let width = NR.min(m - j0);
            if width == NR {
                // A full panel copies `NR` values per row; the fixed length compiles to plain moves.
                for (p, row) in out.chunks_exact_mut(NR).enumerate() {
                    let start = (pc + p) * m + j0;
                    row.copy_from_slice(&b[start..start + NR]);
                }
            } else {
                for p in 0..kc {
                    let source = &b[(pc + p) * m + j0..(pc + p) * m + j0 + width];
                    out[p * NR..p * NR + width].copy_from_slice(source);
                }
            }
        }
    };
    if threads <= 1 || !shared {
        work();
    } else {
        crate::parallel::run_on_pool(threads, &work);
    }
    packed
}

/// Packs rows `r0..r0 + rows` of `A`, columns `pc..pc + kc`, as panels of `MR` rows stored
/// `[p][MR]`, zero-padded past the last row.
fn pack_a(
    a: &[f64],
    out: &mut [f64],
    (r0, rows): (usize, usize),
    (pc, kc, k): (usize, usize, usize),
) {
    for (panel, out) in out
        .chunks_exact_mut(kc * MR)
        .take(rows.div_ceil(MR))
        .enumerate()
    {
        let i0 = panel * MR;
        let height = MR.min(rows - i0);
        for i in 0..MR {
            if i < height {
                let row = &a[(r0 + i0 + i) * k + pc..(r0 + i0 + i) * k + pc + kc];
                for (p, value) in row.iter().enumerate() {
                    out[p * MR + i] = *value;
                }
            } else {
                for p in 0..kc {
                    out[p * MR + i] = 0.0;
                }
            }
        }
    }
}

/// Multiplies the rows of `A` starting at `r0` that `chunk` (rows of `C`) covers.
fn multiply_chunk(
    a: &[f64],
    packed_b: &[f64],
    chunk: &mut [f64],
    packed_a: &mut [f64],
    (r0, k, m, mpad): (usize, usize, usize, usize),
    isa: Isa,
) {
    let rows = chunk.len() / m;
    let mut tile = [0.0f64; MR * NR];
    for pc in (0..k).step_by(KC) {
        let kc = KC.min(k - pc);
        pack_a(a, packed_a, (r0, rows), (pc, kc, k));
        let block = &packed_b[pc * mpad..(pc + kc) * mpad];
        for (jp, b_panel) in block.chunks_exact(kc * NR).enumerate() {
            let j0 = jp * NR;
            let width = NR.min(m - j0);
            for (ip, a_panel) in packed_a
                .chunks_exact(kc * MR)
                .take(rows.div_ceil(MR))
                .enumerate()
            {
                let i0 = ip * MR;
                let height = MR.min(rows - i0);
                micro_kernel(isa, kc, a_panel, b_panel, &mut tile);
                for i in 0..height {
                    let row = &mut chunk[(i0 + i) * m + j0..(i0 + i) * m + j0 + width];
                    for (j, cell) in row.iter_mut().enumerate() {
                        *cell += tile[j * MR + i];
                    }
                }
            }
        }
    }
}

/// `tile[j * MR + i] = sum over p of a[p * MR + i] * b[p * NR + j]`, fused, in order of `p`.
#[inline]
fn micro_kernel(isa: Isa, kc: usize, a: &[f64], b: &[f64], tile: &mut [f64; MR * NR]) {
    assert!(a.len() >= kc * MR && b.len() >= kc * NR);
    match isa {
        Isa::Scalar => micro_scalar(kc, a, b, tile),
        #[cfg(target_arch = "aarch64")]
        // SAFETY: the slice lengths were checked above and NEON is baseline on aarch64.
        Isa::Neon => unsafe { micro_neon(kc, a.as_ptr(), b.as_ptr(), tile.as_mut_ptr()) },
        #[cfg(target_arch = "x86_64")]
        Isa::Avx2 => {
            assert!(is_x86_feature_detected!("avx2") && is_x86_feature_detected!("fma"));
            // SAFETY: the slice lengths were checked above and AVX2 and FMA were just detected.
            unsafe { micro_avx2(kc, a.as_ptr(), b.as_ptr(), tile.as_mut_ptr()) }
        }
    }
}

fn micro_scalar(kc: usize, a: &[f64], b: &[f64], tile: &mut [f64; MR * NR]) {
    let mut acc = [0.0f64; MR * NR];
    for p in 0..kc {
        let (a, b) = (&a[p * MR..p * MR + MR], &b[p * NR..p * NR + NR]);
        for j in 0..NR {
            for i in 0..MR {
                acc[j * MR + i] = a[i].mul_add(b[j], acc[j * MR + i]);
            }
        }
    }
    *tile = acc;
}

/// # Safety
/// `a` must be readable for `kc * MR` values, `b` for `kc * NR`, `tile` writable for `MR * NR`.
#[cfg(target_arch = "aarch64")]
unsafe fn micro_neon(kc: usize, a: *const f64, b: *const f64, tile: *mut f64) {
    use std::arch::aarch64::*;
    let mut acc = [[vdupq_n_f64(0.0); MR / 2]; NR];
    for p in 0..kc {
        let (ap, bp) = (a.add(p * MR), b.add(p * NR));
        let av = [
            vld1q_f64(ap),
            vld1q_f64(ap.add(2)),
            vld1q_f64(ap.add(4)),
            vld1q_f64(ap.add(6)),
        ];
        let bv = [vld1q_f64(bp), vld1q_f64(bp.add(2)), vld1q_f64(bp.add(4))];
        macro_rules! column {
            ($j:literal, $reg:expr, $lane:literal) => {
                for (cell, a) in acc[$j].iter_mut().zip(av) {
                    *cell = vfmaq_laneq_f64::<$lane>(*cell, a, bv[$reg]);
                }
            };
        }
        column!(0, 0, 0);
        column!(1, 0, 1);
        column!(2, 1, 0);
        column!(3, 1, 1);
        column!(4, 2, 0);
        column!(5, 2, 1);
    }
    for (j, column) in acc.iter().enumerate() {
        for (half, value) in column.iter().enumerate() {
            vst1q_f64(tile.add(j * MR + half * 2), *value);
        }
    }
}

/// # Safety
/// As `micro_neon`, and the CPU must support AVX2 and FMA.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,fma")]
unsafe fn micro_avx2(kc: usize, a: *const f64, b: *const f64, tile: *mut f64) {
    use std::arch::x86_64::*;
    let mut acc = [[_mm256_setzero_pd(); MR / 4]; NR];
    for p in 0..kc {
        let (ap, bp) = (a.add(p * MR), b.add(p * NR));
        let av = [_mm256_loadu_pd(ap), _mm256_loadu_pd(ap.add(4))];
        for (j, column) in acc.iter_mut().enumerate() {
            let scale = _mm256_broadcast_sd(&*bp.add(j));
            for (cell, a) in column.iter_mut().zip(av) {
                *cell = _mm256_fmadd_pd(a, scale, *cell);
            }
        }
    }
    for (j, column) in acc.iter().enumerate() {
        for (half, value) in column.iter().enumerate() {
            _mm256_storeu_pd(tile.add(j * MR + half * 4), *value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fill(len: usize, seed: u64) -> Vec<f64> {
        let mut state = seed;
        (0..len)
            .map(|_| {
                state = state
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                ((state >> 33) as f64 / (1u64 << 31) as f64) * 2.0 - 1.0
            })
            .collect()
    }

    fn reference(a: &[f64], b: &[f64], n: usize, k: usize, m: usize) -> Vec<f64> {
        let mut c = vec![0.0; n * m];
        for i in 0..n {
            for j in 0..m {
                let mut sum = 0.0;
                for p in 0..k {
                    sum += a[i * k + p] * b[p * m + j];
                }
                c[i * m + j] = sum;
            }
        }
        c
    }

    fn run(n: usize, k: usize, m: usize, threads: usize, isa: Isa) -> (Vec<f64>, Vec<f64>) {
        let a = fill(n * k, 1);
        let b = fill(k * m, 2);
        let mut c = vec![0.0; n * m];
        matmul_f64(&a, &b, &mut c, (n, k, m), threads, isa);
        (c, reference(&a, &b, n, k, m))
    }

    /// Agreement to 1e-12 relative to the value (with a floor of 1, so a sum that cancels to
    /// nearly zero is judged against the size of its terms rather than against itself).
    fn assert_close(got: &[f64], want: &[f64], what: &str) {
        for (index, (g, w)) in got.iter().zip(want).enumerate() {
            assert!(
                (g - w).abs() <= 1e-12 * w.abs().max(1.0),
                "{what}: element {index}: {g} vs {w}"
            );
        }
    }

    const SHAPES: &[(usize, usize, usize)] = &[
        (1, 1, 1),
        (1, 9, 7),
        (9, 7, 1),
        (7, 13, 5),
        (8, 6, 6),
        (9, 257, 7),
        (33, 300, 50),
        (513, 17, 511),
        (130, 513, 13),
    ];

    #[test]
    fn matches_the_triple_loop_on_awkward_shapes() {
        for &(n, k, m) in SHAPES {
            for isa in [Isa::Scalar, native_isa()] {
                let (got, want) = run(n, k, m, 1, isa);
                assert_close(&got, &want, &format!("{n}x{k} * {k}x{m} {isa:?}"));
            }
        }
    }

    #[test]
    fn zero_inner_dimension_gives_zeros() {
        let (got, _) = run(3, 0, 4, 1, native_isa());
        assert_eq!(got, vec![0.0; 12]);
    }

    #[test]
    fn output_is_bit_identical_across_threads_runs_and_kernels() {
        for &(n, k, m) in &[(7, 13, 5), (70, 300, 50), (257, 129, 100)] {
            let (first, _) = run(n, k, m, 1, native_isa());
            for threads in [1, 2, 8] {
                for _ in 0..2 {
                    let (again, _) = run(n, k, m, threads, native_isa());
                    assert_eq!(first, again, "{n}x{k}x{m} with {threads} threads");
                }
            }
            let (scalar, _) = run(n, k, m, 2, Isa::Scalar);
            assert_eq!(first, scalar, "{n}x{k}x{m} scalar fallback");
        }
    }

    #[test]
    fn a_refused_thread_leaves_the_work_to_the_threads_that_started() {
        let (n, k, m) = (70, 300, 50);
        let (expected, _) = run(n, k, m, 1, native_isa());
        // No pool worker is available: the calling thread does all of it.
        let (got, _) = crate::parallel::with_no_pool_workers(|| run(n, k, m, 8, native_isa()));
        assert_eq!(got, expected);
    }

    #[test]
    fn matmul_inside_pool_worker_completes() {
        if std::thread::available_parallelism().map_or(1, |count| count.get()) < 2 {
            return;
        }
        let (n, k, m) = (200, 200, 200);
        let (expected, _) = run(n, k, m, 1, native_isa());
        // Another test may hold the pool, which runs the outer call on the caller alone; try
        // again until a matmul really ran on a pool worker.
        for _ in 0..50 {
            let (sender, receiver) = std::sync::mpsc::channel();
            let expected = expected.clone();
            std::thread::spawn(move || {
                let caller = std::thread::current().id();
                let off_caller = Mutex::new(Vec::new());
                let gate = std::sync::atomic::AtomicUsize::new(0);
                crate::parallel::run_on_pool(4, &|| {
                    // Hold every invocation until two have started, so a worker joins in.
                    gate.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    let start = std::time::Instant::now();
                    while gate.load(std::sync::atomic::Ordering::SeqCst) < 2
                        && start.elapsed().as_millis() < 100
                    {
                        std::thread::yield_now();
                    }
                    let (got, _) = run(n, k, m, 4, native_isa());
                    if std::thread::current().id() != caller {
                        off_caller.lock().unwrap().push(got);
                    }
                });
                let _ = sender.send(off_caller.into_inner().unwrap());
            });
            let results = receiver
                .recv_timeout(std::time::Duration::from_secs(10))
                .expect("a matmul inside a pool worker finished within 10 s");
            if !results.is_empty() {
                for got in results {
                    assert_eq!(got, expected);
                }
                return;
            }
        }
        panic!("no matmul ever ran on a pool worker");
    }

    #[test]
    fn packing_b_on_several_workers_gives_the_same_panels() {
        // Large enough that the workers share the packing.
        let (k, m): (usize, usize) = (300, 1999);
        assert!(k * m.next_multiple_of(NR) >= PARALLEL_PACK_MIN);
        let b = fill(k * m, 5);
        let mpad = m.next_multiple_of(NR);
        assert_eq!(pack_b(&b, k, m, mpad, 1), pack_b(&b, k, m, mpad, 4));
    }

    #[test]
    fn small_products_stay_on_one_thread() {
        // Measured on M1 Pro: 128^3 is faster on one thread, 160^3 and up on several.
        assert_eq!(threads_for((128, 128, 128)), 1);
        let cores = std::thread::available_parallelism().map_or(1, |count| count.get());
        assert_eq!(threads_for((160, 160, 160)) > 1, cores > 1);
    }

    /// Reports which kernel `native_isa` picks. Setting `GUION_EXPECT_ISA` (for example
    /// `Avx2`) makes the test fail when the machine picks something else, so an emulated or
    /// remote run can prove the SIMD path really executed.
    #[test]
    fn reports_the_kernel_path() {
        let isa = native_isa();
        eprintln!("matmul kernel path: {isa:?}");
        if let Ok(expected) = std::env::var("GUION_EXPECT_ISA") {
            assert_eq!(format!("{isa:?}"), expected);
        }
    }

    /// Kernel-only throughput: the micro-kernel on packed panels that sit in L1, one core.
    /// Run with `cargo test --release -p guion-vm-core kernel_ceiling -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn kernel_ceiling() {
        let kc = KC;
        let a = fill(kc * MR, 3);
        let b = fill(kc * NR, 4);
        let mut tile = [0.0f64; MR * NR];
        let calls = 400_000usize;
        let isa = native_isa();
        let mut best = f64::MAX;
        for _ in 0..7 {
            let start = std::time::Instant::now();
            for _ in 0..calls {
                micro_kernel(
                    isa,
                    kc,
                    std::hint::black_box(&a),
                    std::hint::black_box(&b),
                    &mut tile,
                );
                std::hint::black_box(&tile);
            }
            best = best.min(start.elapsed().as_secs_f64());
        }
        let flops = (calls * kc * MR * NR * 2) as f64;
        let gflops = flops / best / 1e9;
        eprintln!("kernel {isa:?}: {gflops:.2} GFLOPS on one core (best of 7)");
        #[cfg(target_arch = "aarch64")]
        {
            // Core clock from a chain of dependent integer adds (one cycle each); the peak is
            // 4 pipes x 2 lanes x 2 flops = 16 flops per cycle.
            let iterations = 200_000_000u64;
            let mut best = f64::MAX;
            for _ in 0..5 {
                let (mut count, mut left) = (0u64, iterations);
                let start = std::time::Instant::now();
                // SAFETY: register-only arithmetic.
                unsafe {
                    std::arch::asm!(
                        "2:",
                        "add {c}, {c}, #1", "add {c}, {c}, #1", "add {c}, {c}, #1",
                        "add {c}, {c}, #1", "add {c}, {c}, #1", "add {c}, {c}, #1",
                        "add {c}, {c}, #1", "add {c}, {c}, #1",
                        "subs {n}, {n}, #1",
                        "b.ne 2b",
                        c = inout(reg) count, n = inout(reg) left,
                        options(nostack, nomem)
                    );
                }
                std::hint::black_box((count, left));
                best = best.min(start.elapsed().as_secs_f64());
            }
            let ghz = iterations as f64 * 8.0 / best / 1e9;
            eprintln!(
                "core clock {ghz:.2} GHz; peak {:.1} GFLOPS; kernel is {:.0}% of peak",
                ghz * 16.0,
                gflops / (ghz * 16.0) * 100.0
            );
        }
    }

    /// End-to-end throughput by size and thread count, median of many runs.
    #[test]
    #[ignore]
    fn end_to_end_throughput() {
        let cores = std::thread::available_parallelism().map_or(1, |count| count.get());
        for size in [128usize, 256, 512, 1024, 2048] {
            let (n, k, m) = (size, size, size);
            let a = fill(n * k, 1);
            let b = fill(k * m, 2);
            let mut c = vec![0.0; n * m];
            for threads in [1, threads_for((n, k, m)).min(cores)] {
                let mut times = Vec::new();
                let runs = if size >= 2048 { 9 } else { 61 };
                for _ in 0..runs + 2 {
                    let start = std::time::Instant::now();
                    matmul_f64(&a, &b, &mut c, (n, k, m), threads, native_isa());
                    times.push(start.elapsed().as_secs_f64());
                }
                times.sort_by(|x, y| x.partial_cmp(y).unwrap());
                let median = times[times.len() / 2];
                let gflops = 2.0 * (n * k * m) as f64 / median / 1e9;
                eprintln!(
                    "{size}^3 threads {threads}: median {:.3} ms, min {:.3} ms, {gflops:.1} GFLOPS",
                    median * 1e3,
                    times[0] * 1e3
                );
                if threads_for((n, k, m)).min(cores) == 1 {
                    break;
                }
            }
        }
    }
}
