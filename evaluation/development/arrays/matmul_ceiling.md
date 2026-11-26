# Matrix multiply against the machine's ceiling

Machine: Apple M1 Pro, 10 cores (8 performance, 2 efficiency), 16 GB. Guion uses the NEON
instructions only (no SME, no Apple matrix unit, no Accelerate). All numbers are `f64`, `n x n`
times `n x n`, from a release build.

## How it was measured

- Kernel only: `kernel_ceiling` in `crates/vm-core/src/matmul.rs` (ignored by default) runs the
  8 x 6 micro-kernel alone on packed panels that sit in L1, on one core, and measures the core
  clock with a chain of dependent adds. Run it with
  `cargo test --release -p guion-vm-core kernel_ceiling -- --ignored --nocapture`.
- End to end, in process: `end_to_end_throughput` in the same file times `matmul_f64` (packing,
  multiply, write-back) at 128 to 2048, median of 61 runs (9 at 2048), one thread and all
  threads. Run to run it varies by about 2 to 5 percent, so a change counts only if it clears that.
- End to end, whole program: `matmul_compute.py` (512 x 512, 15 trials, per-multiply cost by
  subtracting a 1-multiply run from a 21-multiply run). It includes the result array allocation
  and the 262,144-element sum on both sides and varies by 10 percent or more from run to run
  (Guion per-trial values from 0.75 to 1.8 ms), so it cannot resolve changes of a few percent.

## The ceiling

| | Value |
|---|---|
| Core clock (measured, one P-core) | 3.19 to 3.20 GHz |
| Theoretical peak per core (16 flops per cycle) | 51.1 GFLOPS |
| Micro-kernel alone, one core | 49.1 GFLOPS (96% of peak) |
| Eight P-cores at the kernel rate | 393 GFLOPS |
| NumPy 512 x 512 (Accelerate), per-multiply | 0.64 ms, 419 GFLOPS |

NumPy's figure is above eight cores of NEON because Accelerate uses Apple's matrix unit on this
chip, which is undocumented and which Guion does not use.

## Before the changes in this round

| n | threads | median | GFLOPS |
|---|---|---|---|
| 128 | 1 | 0.121 ms | 34.8 |
| 256 | 1 | 0.783 ms | 42.9 |
| 256 | 10 | 0.255 ms | 132 |
| 512 | 1 | 6.09 ms | 44.1 |
| 512 | 10 | 1.150 ms | 233 |
| 1024 | 10 | 7.26 ms | 295 |
| 2048 | 10 | 58 ms | 296 |

`matmul_compute.py`: Guion 1.298 ms per multiply, NumPy 0.641 ms, ratio 2.02.

Where the time goes at 512 x 512 on 10 threads (temporary timers, removed): packing `B` on one
thread took 0.34 ms of a 1.15 ms multiply. It was slow because each of the 44,000 row pieces of 6
values went through a general copy call. The multiply itself was within a few percent of the
kernel rate per core (single thread 44 of 49 GFLOPS).

## Attempts

Medians of three runs of `end_to_end_throughput` unless stated, 10 threads.

| Change | 512 | 1024 | Result |
|---|---|---|---|
| None (above) | 1.150 ms | 7.26 ms | |
| 1. Pack `B` on the pool, one panel per queue pull | 1.12 ms | 7.15 ms | small gain; a panel is about 1 microsecond so the queue dominated |
| 1b. Fixed-length copy for full panels, 4 panels per queue pull | 1.04 ms | 6.74 ms | kept: 9% at 512, 7% at 1024 |
| 1c. Share the packing only when packed `B` is 2^19 values or more | same | same | kept: sharing was neutral at 512 and 3% faster at 1024, and slower at 256 |
| 2. `KC` 128 | 1.07 ms | 7.55 ms | rejected, slower |
| 2. `KC` 384 | 1.02 to 1.10 ms | 6.63 to 6.74 ms | rejected, inside the noise |
| 2. `KC` 512 | 1.01 to 1.18 ms | 6.56 ms | rejected, inside the noise (2048 slower: 56.7 vs 52.5 ms) |
| 2. Rows per worker chunk, 1x to 8x the thread count | 1.09 to 1.47 ms | 7.15 to 9.6 ms | rejected, the existing 4x is best or tied |
| 2. `MC` 64 and 192 | not run | not run | `MC` only caps the chunk height, which is 16 to 32 rows at these sizes on 10 threads, so it has no effect below about 5000 |
| 3. Prefetch next `A` and `B` data in the kernel, 8 and 2 steps ahead | 1.02 ms | 6.67 ms | rejected, inside the noise (kernel alone 48.5 vs 49.1 GFLOPS) |
| 4. Lower `PARALLEL_MIN` | | | rejected: 128^3 is 107 us on one thread and 91 to 105 us on two or more; 144^3 is better on 4 threads (90 us) than on 10 (118 us) or one (144 us) |

Other observations: eight threads match ten at 512 and 1024 (the two efficiency cores add
nothing), and at 144 to 224 four to eight threads beat ten.

## After

In process, 10 threads (median of three runs):

| n | median | GFLOPS | share of eight cores at the kernel rate |
|---|---|---|---|
| 256 | 0.236 ms | 142 | 36% |
| 512 | 1.04 ms | 258 | 66% |
| 1024 | 6.74 ms | 319 | 81% |
| 2048 | 52.8 ms | 325 | 83% |

One thread at 512 and above: 45 to 46 GFLOPS, 93% of the kernel alone.

`matmul_compute.py` (two runs): Guion 1.295 and 1.244 ms per multiply, NumPy 0.718 and 0.623 ms;
ratio 1.80 and 2.00 (before this round: 1.298 ms, ratio 2.02). The whole-program figure is
about 0.2 ms above the in-process 1.04 ms (it also allocates the result array, sums it, and
starts each multiply from idle workers; the split was not measured), and its noise hides the 0.1 ms saved, so the ratio to NumPy there is
about 2 either way.

## What remains

At 512 the in-process multiply reaches 66% of eight cores at the kernel rate and Guion runs at
roughly half of NumPy's rate. The rest of the gap to NumPy is Accelerate's use of Apple's
undocumented matrix unit on M1 to M3; Guion uses only documented instructions. Within Guion's
own ceiling the remaining loss is the start and end of each parallel multiply (waking workers,
uneven last chunks), which matters most at the smaller sizes.
