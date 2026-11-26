# Guion against CPython, free-threaded CPython and PyPy

One local comparison. Run with `python3 run.py [trials] [output.json]`; the committed
`results.json` is the 15-trial run.

## Machine and versions

- Apple M1 Pro, 16 GB (17179869184 bytes), macOS-27.0.1-arm64-arm-64bit-Mach-O.
- Guion: `guion 0.9.0 (artifact format 90, timezone data 2026c)`, `target/release/guion`,
  built from commit 262f12a (a development build made after 0.9.0, with the shared thread pool,
  matmul tuning and native element reads).
- CPython: 3.14.7 (uv, `cpython-3.14.7-macos-aarch64-none`).
- Free-threaded CPython: 3.14.10 (uv, `3.14t`; uv offers no 3.14.7 free-threaded build, so the
  newest 3.14t it provides was used).
- PyPy: PyPy implementing Python 3.11.17 (uv `pypy@3.11`; it reports
  `PyPy 8.0.1` with `Python 3.11.17`). Full version strings are in `results.json`.

## Protocol

15 trials. Each trial runs every lane of every workload once, in a shuffled order
(seed 2026), and records end-to-end wall time from process start to exit. Guion is run with
`guion run --no-cache`, so its time includes compilation; the Python lanes include
interpreter start-up and imports. Every lane's output is checked against the standard CPython
lane (numbers to a relative 1e-9, everything else exactly) on every trial, and the run stops
on a mismatch. Python lanes run with `-I` for the array programs; the first-source Python lanes run without it because `imported_helper` imports a module from the script's directory. Each first-source sample runs in a fresh copy of the workload directory, since `sqlite_toml` writes a database. The ratio is Guion's time over the other lane's time, paired within a trial;
the median and a 95% bootstrap interval (`bootstrap_median_ci` from the arrays kit) are in
`results.json` under `summary`. A ratio below 1 means Guion is faster.

## Results

Median of paired per-trial ratios Guion time / other lane time, with 95% bootstrap interval; below 1 means Guion is faster. Generated from `results.json` `summary`. The `null` row measures process start-up only.

| workload | Guion / CPython | Guion / free-threaded | Guion / PyPy |
|---|---|---|---|
| arrays/null (start-up only) | 0.420 (0.407-0.436) | 0.403 (0.393-0.431) | 0.277 (0.266-0.287) |
| arrays/elementwise | 0.024 (0.023-0.024) | 0.021 (0.020-0.021) | 0.052 (0.051-0.053) |
| arrays/sum | 0.065 (0.064-0.067) | 0.054 (0.053-0.058) | 0.257 (0.250-0.269) |
| arrays/matmul | 0.012 (0.012-0.014) | 0.011 (0.011-0.011) | 0.045 (0.045-0.049) |
| arrays/sort | 0.124 (0.119-0.129) | 0.124 (0.118-0.128) | 0.383 (0.371-0.403) |
| arrays/loop | 0.134 (0.131-0.137) | 0.113 (0.106-0.115) | 0.435 (0.427-0.456) |
| first_source/tiny | 0.353 (0.333-0.385) | 0.346 (0.315-0.362) | 0.225 (0.217-0.246) |
| first_source/arithmetic | 0.414 (0.394-0.429) | 0.390 (0.373-0.419) | 0.272 (0.258-0.275) |
| first_source/json_file | 0.258 (0.255-0.268) | 0.248 (0.242-0.256) | 0.152 (0.150-0.159) |
| first_source/imported_helper | 0.411 (0.394-0.414) | 0.391 (0.372-0.404) | 0.280 (0.276-0.289) |
| first_source/sqlite_toml | 0.342 (0.325-0.347) | 0.311 (0.302-0.321) | 0.152 (0.143-0.157) |

## Scope: what this does and does not say

Workloads are the six plain-Python programs of `evaluation/development/arrays/programs`
(`null`, `elementwise`, `sum`, `matmul`, `sort`, `loop`; `null` is the start-up baseline) and
the five `evaluation/first_source_speed/workloads` (`tiny`, `arithmetic`, `json_file`,
`imported_helper`, `sqlite_toml`). The array programs are the small sizes the plain-Python
lane can finish, written as loops in Python and as array operations in Guion, so they compare
a program in each language's natural style, not identical algorithms. No NumPy lane is used.
The first-source workloads are start-up dominated.

The Python programs are single-threaded, so the free-threaded build's lack of a global
interpreter lock does not help them; this does not measure multi-threaded Python code. PyPy is
a JIT, so its numbers depend on how long a program runs; the loop workload is the one where it
was faster than Guion in the earlier run (2.5 times) and is not in this one (Guion 0.435 of PyPy's time). One machine, one run of 15 trials; nothing here is a claim about other
hardware or larger sizes.
