# Guion against CPython, free-threaded CPython and PyPy

One local comparison. Run with `python3 run.py [trials] [output.json]`; the committed
`results.json` is the 15-trial run.

## Machine and versions

- Apple M1 Pro, 16 GB (17179869184 bytes), macOS-27.0.1-arm64-arm-64bit-Mach-O.
- Guion: `guion 0.9.0 (artifact format 90, timezone data 2026c)`, `target/release/guion`,
  built from commit d1e6ccc (branch perf/data-speed).
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
| arrays/null (start-up only) | 0.399 (0.379-0.459) | 0.374 (0.354-0.437) | 0.252 (0.240-0.298) |
| arrays/elementwise | 0.023 (0.023-0.024) | 0.020 (0.020-0.020) | 0.051 (0.050-0.052) |
| arrays/sum | 0.058 (0.056-0.059) | 0.049 (0.048-0.050) | 0.234 (0.228-0.239) |
| arrays/matmul | 0.012 (0.012-0.013) | 0.010 (0.010-0.010) | 0.043 (0.043-0.045) |
| arrays/sort | 0.124 (0.118-0.126) | 0.122 (0.118-0.125) | 0.379 (0.374-0.390) |
| arrays/loop | 0.763 (0.719-0.770) | 0.628 (0.606-0.651) | 2.527 (2.462-2.547) |
| first_source/tiny | 0.355 (0.345-0.361) | 0.337 (0.326-0.341) | 0.216 (0.208-0.228) |
| first_source/arithmetic | 0.411 (0.393-0.424) | 0.395 (0.376-0.412) | 0.267 (0.246-0.273) |
| first_source/json_file | 0.260 (0.240-0.274) | 0.244 (0.237-0.251) | 0.157 (0.148-0.161) |
| first_source/imported_helper | 0.400 (0.387-0.429) | 0.387 (0.375-0.412) | 0.283 (0.263-0.291) |
| first_source/sqlite_toml | 0.333 (0.325-0.340) | 0.320 (0.301-0.332) | 0.155 (0.149-0.158) |

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
is faster than Guion. One machine, one run of 15 trials; nothing here is a claim about other
hardware or larger sizes.
