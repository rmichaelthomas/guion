# Speed report

Date: 2026-11-12. Branch `perf/speed`. Artifact format 64. Verification tables: `frames-verification.md` and `arrays-verification.md` at the repository root (written by `scripts/verify-frames.sh` and `scripts/verify-arrays.sh`).

This batch fixed the slow spots that the earlier [data frames](2026-11-12-data-frames-report.md) and [native arrays](2026-11-11-native-arrays-report.md) reports found.

## What landed

| Commit | Change |
|---|---|
| ae485c5 | Calls from interpreted code into a machine-code function no longer rescan the program |
| c071ffd, 8cdf52c | Native `csv_read_columns` (artifact format 64) and a `frames` reader built on it |
| 4c19507, db55344 | Array kernels reuse unshared buffers; `set a = array_fn(a, ...)` updates in place; the self-hosted compiler emits `ArraySetLocal` |
| 5d961e9 | Branch-free `array_filter` |
| 8d73df4 | Packed, register-tiled matrix multiplication for `f64` and `f32`: NEON on aarch64, AVX2 with FMA on x86_64 (runtime detection), a scalar fallback, and threads above 2^21 multiply-adds |

## How each fix works

- **Helper calls (ae485c5).** Every time interpreted code called a function that had been compiled to machine code, the runtime rebuilt a table of the program's text and byte constants by scanning every instruction of every function. The cost grew with the size of the program (here, the `frames` package plus the standard library), not with the helper, so a one-line helper called a million times cost about 7 µs a call. The table is now built once when the program loads and shared. A call now costs about 0.2 µs. The remaining cost is the fixed set-up of entering machine code.
- **CSV reading (c071ffd, 8cdf52c).** A new builtin, `csv_read_columns(path, kinds)`, reads a file in 64 KiB blocks in native code, infers each column's kind (integer, float, boolean or text), keeps a missing-value mask, and builds text columns with a first-seen dictionary. The `frames` reader now calls it once instead of running a loop per value. Three behaviours changed with it: streaming CSV and line readers no longer count the whole file against `max_file_bytes` (a single record still does, and `read_text` still fails on a large file); empty text is written as `""` so it reads back as empty text rather than missing; and the `R_ALLOCATION_LIMIT` message names the `allocation_limit_bytes` setting to raise. A column that is entirely missing is now `text`, not `i64`. An artifact below format 64 that uses the builtin is rejected at decode.
- **Element-wise maths (4c19507, db55344).** Arithmetic, negation, `abs`, `clip` and float math now write into an operand's buffer when nothing else holds it, instead of allocating a new 80 MB result for 10M `f64`. The VM also releases a local just before `set a = array_fn(a, ...)` stores back into it, so that update is in place; a `let b = a` copy keeps the count above one, so `b` is never touched. Finiteness of float results is accumulated without a branch per element and searched only when a check fails, so the loop vectorises and errors keep the same element index. The self-hosted compiler now emits `ArraySetLocal` for `set a = array_set(a, i, v)`, matching the Rust compiler.
- **Filter (5d961e9).** Profiling showed nearly all of a filter step inside `array_filter`'s `if flag { push }`, which mispredicts on a random mask. It now writes every element to the next slot and advances the slot by the flag. A filter step over seven 1M-row columns dropped from about 26 ms to about 7 ms. No `frames` change was needed; group-by and join were already inside the target after the earlier tasks.

## Measurements

One Apple M1 Pro, macOS 27.0.1, release build, CPython 3.14.7, pandas 3.1.1 with NumPy 2.5.4 for frames and NumPy 2.5.1 for arrays. Each kit ran its full default protocol: 15 interleaved trials, shuffled order, end-to-end wall time, every Guion output matching the reference to 1e-9. "Before" columns are the earlier reports' numbers. Raw samples: `evaluation/development/frames/results.json`, `evaluation/development/arrays/results.json` and `results_large.json`.

### Frames against pandas, 1,000,000 rows (35.8 MB), default limits

| Workload | Guion before (s) | Guion after (s) | pandas after (s) | Guion / pandas before | Guion / pandas after (95% interval) |
|---|---|---|---|---|---|
| start-up only | 0.021 | 0.019 | 0.258 | 0.068 | 0.071 (0.070–0.074) |
| read the CSV | 2.811 | 0.209 | 0.322 | 8.10 | **0.654** (0.649–0.660) |
| read, filter, sum | 2.867 | 0.219 | 0.323 | 8.14 | 0.676 (0.672–0.683) |
| read, group by, mean and count | 2.860 | 0.225 | 0.336 | 7.72 | 0.673 (0.665–0.679) |
| read, inner join | 2.884 | 0.246 | 0.371 | 7.14 | 0.664 (0.657–0.666) |
| read, sort by two keys | 2.898 | 0.287 | 0.503 | 5.32 | 0.569 (0.556–0.572) |

Reading 1M rows is now about 0.19 s of compute (start-up subtracted), against about 2.79 s before, about 15 times faster, and the end-to-end read beats pandas, mostly because pandas' import costs about 0.26 s. Compute alone is still about 3 times pandas (0.19 s against 0.064 s).

Work after the read (median of per-trial differences from the read-only lane):

| Part | Guion before | Guion after | pandas (this run) | Guion / pandas after |
|---|---|---|---|---|
| filter and sum | ~56 ms | ~9.7 ms | ~2.5 ms | ~3.8 (noisy; the earlier 5 ms pandas figure gives ~1.9) |
| group-by mean and count | ~49 ms | ~16 ms | ~13 ms | ~1.2 |
| inner join | ~73 ms | ~37 ms | ~50 ms | ~0.74 |
| two-key sort | ~86 ms | ~78 ms | ~187 ms | ~0.41 |

These are differences of two noisy end-to-end medians; pandas' filter part in particular moves between 2 and 5 ms from run to run, so the filter ratio is between about 2 and 4, and the honest summary is "about 10 ms, a few times pandas, in the noise of this method".

### Arrays against NumPy, the plan's sizes (10M elements, 512×512 matmul)

| Workload | Guion before (s) | Guion after (s) | NumPy after (s) | Guion / NumPy before | Guion / NumPy after (95% interval) |
|---|---|---|---|---|---|
| start-up only | 0.0151 | 0.0142 | 0.0610 | 0.223 | 0.233 |
| `a*b+c`, 5 passes | 0.2209 | 0.1413 | 0.1508 | 1.330 | **0.925** (0.911–0.947) |
| sum, 10 passes | 0.0599 | 0.0584 | 0.0955 | 0.588 | 0.612 (0.609–0.617) |
| 512×512 matmul | 0.0421 | 0.0166 | 0.0648 | 0.623 | **0.258** (0.251–0.279) |
| stable sort of 1M | 0.0499 | 0.0452 | 0.0809 | 0.572 | 0.559 (0.547–0.577) |
| 1M `array_get` loop | 0.0886 | 0.0871 | 0.1970 | 0.428 | 0.446 (0.430–0.451) |

Compute alone (start-up subtracted): `a*b+c` is 0.127 s for Guion against 0.090 s for NumPy, about **1.41 times NumPy**, down from about 2.1. Sum and sort are unchanged within noise (about 1.3 and 1.6 times NumPy compute). Matrix multiplication is covered in its own section below. An in-place loop of 60 `set a = array_add(a, b)` style updates measured 0.86 times NumPy in the task's own run.

### Arrays at smaller sizes (4M elements, 256×256), with plain CPython

| Workload | Guion (s) | NumPy (s) | CPython (s) | Guion / NumPy | Guion / CPython |
|---|---|---|---|---|---|
| start-up only | 0.0141 | 0.0628 | 0.0229 | 0.224 | 0.621 |
| `a*b+c`, 5 passes | 0.0667 | 0.0988 | 1.5781 | 0.674 (0.657–0.694) | 0.042 |
| sum, 10 passes | 0.0339 | 0.0768 | 0.3170 | 0.441 | 0.106 |
| 256×256 matmul | 0.0148 | 0.0615 | 0.5763 | 0.242 | 0.026 |
| sort of 1M | 0.0456 | 0.0816 | 0.1808 | 0.563 | 0.254 |
| 1M `array_get` loop | 0.0863 | 0.2002 | 0.1328 | 0.431 | 0.662 |

Before this batch the `a*b+c` row was 0.0899 s (0.869 of NumPy).

### Matrix multiplication (8d73df4)

The kernel packs the operands into tiles, multiplies register-sized blocks (NEON on aarch64, AVX2 with FMA on x86_64 chosen at run time, scalar elsewhere), computes `f32` in `f64` and narrows, and splits the rows across threads when `m·n·k` is at least 2^21 (about 128×128×128). Output is bit-identical for every thread count. Integer element types keep the checked kernel. On this machine (`available_parallelism` 10) 512×512×512 (2^27) and 256×256×256 (2^24) both run threaded. The end-to-end rows come from the full 15-trial protocol on a release build of 8d73df4 (`evaluation/development/arrays/results_large.json` and `results.json`).

| Measure | Before | After |
|---|---|---|
| 512×512 f64, end to end | 0.0421 s | 0.0166 s |
| 512×512 f64, Guion work after start-up | ~27 ms | ~3.1 ms |
| 256×256 f64, end to end | 0.0185 s | 0.0148 s |
| 512×512 f64, Guion / NumPy end to end | 0.623 | 0.258 (0.251–0.279) |
| 512×512 f64, compute only, Guion / NumPy | ~25 (earlier report) or ~10.5 (this report's method) | **3.03** (1.93 ms against 0.63 ms per multiply; above the target of 3) |

Why NumPy's figure differs between reports. The earlier native-arrays report gave NumPy about 1 ms for this matmul by subtracting the start-up lane (`import numpy; print(0)`) from the end-to-end lane. Both lanes of the matmul workload include building the inputs (two `linspace` calls, a reshape, a sum), and NumPy's first multiply also pays the one-time start of its BLAS thread pool, so the difference of two medians a millisecond apart was noise-dominated and too low. In this run the same method gives NumPy about 2.9 ms (64.8 ms against 61.9 ms) and Guion about 3.1 ms (16.6 ms against 13.5 ms), a ratio of about 1.1 that is not a compute-only comparison.

The compute-only figure comes from the committed lane `evaluation/development/arrays/matmul_compute.py` (programs in `programs_compute/`, results in `results_compute.json`). It runs the same 512×512 multiply once and 21 times on each side, with the inputs built once, output-checked between Guion and NumPy; the per-multiply cost is (21-run time minus 1-run time) divided by 20. Median of 31 interleaved trials on a release build: Guion 1.93 ms, NumPy 0.63 ms (the platform BLAS), so Guion is **3.03 times NumPy**, just above the target of 3 (per-trial ratios range 1.84–3.69). A 15-trial run gave 3.00 and uncommitted scratch runs gave 2.7–2.8, so the true figure sits at about 3 and varies by about 10% between runs. Each multiply also includes a 262,144-element `array_sum`, the same on both sides. This is one machine and one size.

AVX2 check: the AVX2+FMA kernel was run once, on emulated x86_64 in a container (`docker run --platform linux/amd64`, `QEMU_CPU=max`) that reports AVX2 and FMA; `cargo test -p guion-vm-core --lib matmul` passed there on the AVX2 path and its output was bit-identical to the scalar path. Rosetta on this Mac does not report AVX2, so earlier x86_64 runs covered the scalar path only.

### Helper-call cost

| Measure | Before | After |
|---|---|---|
| Cost of a native helper called from an interpreted loop | ~7 µs | ~0.2 µs |
| 1M-row read through the old chunked reader (pre-inlining `io.gu`) | 17.9 s | 3.4 s |

## Targets

| Target | Result |
|---|---|
| CSV read of 1M rows under 0.5 s | Met: 0.21 s end to end, 0.19 s compute |
| `a*b+c` within 1.5× NumPy | Met: 1.41× compute, 0.93× end to end |
| Filter, group-by and join within 3× pandas | Group-by (1.2×) and join (0.74×) met. Filter is about 10 ms against pandas' 2.5–5 ms: 2–4×, not cleanly under 3×; recorded as known issue 20 |

## What remains

- Matrix multiplication is about 3.0 times NumPy's compute at 512×512 (3.03 in the committed lane; known issue 3 stays open, improved in 8d73df4); NumPy still leads because the platform BLAS is tuned for this chip.
- Reading CSV is about 3 times pandas' compute; the end-to-end win comes from start-up.
- Filter after the read is a few times pandas (known issue 20).
- A native call still costs about 0.2 µs. Skipping machine-code entry for loop-free, call-free functions called from the VM would take it to about 0.11 µs, but it changes the tier policy and broke 17 backend tests, so it was left out.
- `frames` writing a CSV makes one native call per row; it was not measured for large writes.
- Transient buffers inside `csv_read_columns` are charged by capacity and a text second pass is charged on top of the first, so the allocation limit is slightly conservative.
- Unchanged from earlier reports: JSONL column order follows sorted key order, and dates, pivot, rolling windows, string methods, Parquet and Arrow are not provided.
