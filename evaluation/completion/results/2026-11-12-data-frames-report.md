# Data frames report

Date: 2026-11-12. Branch `feat/data-frames`. Artifact format 63. Verification table: `frames-verification.md` (produced by `scripts/verify-frames.sh`).

## What landed

| Task | Contents |
|---|---|
| 1 | Format-63 array kernels: `array_take`, `array_factorize`, `array_first_index`, `array_count_by`, `array_sum_by`, `array_min_by`, `array_max_by`, `array_mean_by`, `array_join_index`. Catalog-driven, so both compilers accept and compile them identically; format-62 artifacts cannot use them |
| 2 | The `frames` package in `packages/frames`: column and frame records, select, drop, rename, add, filter, take, head, slice, concat, missing-value handling, masks and reductions |
| 3 | CSV reading with kind inference, explicit kinds, JSONL reading, CSV writing and an aligned printed table |
| 4 | Multi-key stable sort (missing last) and group-by with `sum`, `mean`, `min`, `max`, `count` |
| 5 | Inner and left joins on one or more keys; missing keys never match |
| 6 | Benchmarks against pandas in `evaluation/development/frames` |

The kernel contract is in `language/spec.md` (Runtime rules, native arrays bullet); the package API is in `packages/frames/README.md`.

## Speed

One Apple M1 Pro, macOS 27.0.1, release build, pandas 3.1.1 with NumPy 2.5.4, CPython 3.14.7. A seeded 1,000,000-row CSV (35.8 MB: `id`, `group` with 1,000 values, `x` and `y` with about 1% missing, `flag`) and a 1,000-row lookup table. Each lane ran 15 times, interleaved in shuffled order, timed end to end (process start to exit, including Guion's compile and the pandas import). Every Guion output matched pandas to a relative tolerance of 1e-9. Ratios above 1 mean Guion took longer. Raw samples: `evaluation/development/frames/results.json`.

| Workload | Guion (s) | pandas (s) | Guion / pandas (95% interval) |
|---|---|---|---|
| start-up only | 0.021 | 0.297 | 0.068 (0.060–0.078) |
| read the CSV | 2.811 | 0.359 | **8.10** (7.62–8.33) |
| read, filter `x > 0.5`, sum `y` | 2.867 | 0.365 | 8.14 (7.87–8.47) |
| read, group by `group`, mean and count of `x` | 2.860 | 0.373 | 7.72 (7.43–8.01) |
| read, inner join to the lookup | 2.884 | 0.406 | 7.14 (7.03–7.40) |
| read, sort by `group` then `x` descending | 2.898 | 0.546 | 5.32 (5.23–5.33) |

Every workload is dominated by reading the file. Subtracting medians gives rough estimates of the parts:

| Part | Guion | pandas | Estimate |
|---|---|---|---|
| reading 1M rows (read minus start-up) | ~2.79 s | ~0.063 s | Guion about **45× slower** |
| filter and sum (workload minus read) | ~56 ms | ~5 ms | Guion about 11× slower |
| group-by mean and count | ~49 ms | ~13 ms | Guion about 4× slower |
| inner join | ~73 ms | ~47 ms | Guion about 1.6× slower |
| two-key sort | ~86 ms | ~187 ms | Guion about 2.2× faster |

These differences of medians are noisy at tens of milliseconds; they say which side is faster and by roughly how much, not more.

So for a short script on a small file Guion finishes first (pandas' import alone is about 0.3 s), and once data is in memory the kernels are within a small factor of pandas and faster on sorting. Reading CSV is the problem: it runs per value in the VM.

## Limits found

The first three limits below were fixed by the speed batch; measurements are in [the speed report](2026-11-12-speed-report.md).

- ~~**CSV reading is about 45× slower than pandas.**~~ Fixed: a native `csv_read_columns` builtin reads 1M rows in about 0.2 s, under pandas end to end.
- ~~**A call inside a large guest loop can cost microseconds.**~~ Fixed: the cause was a per-call rescan of the program for constants; a call now costs about 0.2 µs.
- ~~**File size limit.**~~ Fixed: streaming CSV and line readers no longer count the file against `max_file_bytes`, and the allocation-limit error names the setting to raise.
- **JSONL column order** follows the first object's keys as its JSON text lists them, which is sorted: no builtin enumerates an object's keys in file order.
- **Not provided:** dates and times, pivot and melt, rolling windows, string methods, Parquet and Arrow, and publishing `frames` to the registry.
