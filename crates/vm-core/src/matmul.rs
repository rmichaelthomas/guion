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
const KC: usize = 256;
/// Products (n*m*k) below which one thread does the work.
const PARALLEL_MIN: usize = 1 << 21;

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
    let packed_b = pack_b(b, k, m, mpad);
    let threads = threads.clamp(1, n.div_ceil(MR));
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
        std::thread::scope(|scope| {
            for _ in 0..threads {
                scope.spawn(work);
            }
        });
    }
}

/// Packs `B` as, per `KC` block of rows, panels of `NR` columns, each stored `[p][NR]` and
/// zero-padded past column `m`. The block starting at row `pc` begins at `pc * mpad`.
fn pack_b(b: &[f64], k: usize, m: usize, mpad: usize) -> Vec<f64> {
    let mut packed = vec![0.0f64; k * mpad];
    for pc in (0..k).step_by(KC) {
        let kc = KC.min(k - pc);
        let block = &mut packed[pc * mpad..(pc + kc) * mpad];
        for (panel, out) in block.chunks_exact_mut(kc * NR).enumerate() {
            let j0 = panel * NR;
            let width = NR.min(m - j0);
            for p in 0..kc {
                let source = &b[(pc + p) * m + j0..(pc + p) * m + j0 + width];
                out[p * NR..p * NR + width].copy_from_slice(source);
            }
        }
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

    fn assert_close(got: &[f64], want: &[f64], k: usize, what: &str) {
        for (index, (g, w)) in got.iter().zip(want).enumerate() {
            let scale = (k as f64).max(1.0);
            assert!(
                (g - w).abs() <= 1e-12 * scale,
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
                assert_close(&got, &want, k, &format!("{n}x{k} * {k}x{m} {isa:?}"));
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
}
