# Guion against CPython, free-threaded CPython and PyPy

One local comparison. Run with `python3 run.py [trials] [output.json]`; the committed
`results.json` is the 15-trial run.

## Machine and versions

- Apple M1 Pro, 16 GB, macOS (Darwin 27.0.0), arm64.
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
on a mismatch. The ratio is Guion's time over the other lane's time, paired within a trial;
the median and a 95% bootstrap interval (`bootstrap_median_ci` from the arrays kit) are in
`results.json` under `summary`. A ratio below 1 means Guion is faster.

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
