# Supplemental exploratory results (2026-10-25)

These are author-run, warm-cache experiments recovered from the temporary `work/` area during migration. They are not the formal completion performance suite or the independent v1/v2/v3 campaign scores. Keep them separate when discussing speed.

## What the retained measurements say

- The macOS package-analysis launch comparison used eight paired fresh processes. The fast Veritas mode had a 0.337 s median and Python had 0.095 s; Veritas verification mode had a 1.074 s median. Outputs matched after each pair and the packages were verified.
- The macOS 100,000-row package-analysis run used eight pairs. Fast Veritas had a 0.415 s median and Python had 0.163 s; verification mode had a 1.186 s median. Reports and stdout matched for every pair and package checks passed.
- The Debian ARM64 config-migration comparison used eight pairs. Veritas had a 0.262 s median and Python had a 0.213 s median, so Veritas was about 23% slower on that workload. Output and comment checks passed after each run.
- The native package-analysis run on 100,000 rows measured 0.464 s for Veritas and 0.112 s for Python. Report parity passed; this run was substantially slower in Veritas.
- A separate 16-pair JSON-column reduction measured 0.107 s for Veritas and 0.112 s for Python, with output parity. A copy-on-write comparison measured 0.125 s before borrowing and 0.124 s after; the medians are effectively tied.

These measurements show workload-specific wins and losses. They do not support a general claim that Veritas is faster than Python.

## Reproduction files

The directory retains the small JSON result files, source programs, acceptance scripts, and the 100,000-row input. The input is stored as `native-package-analysis/dataset.json.zst` to keep the repository small. Its compressed file passed `zstd -t`; decompressing it produces SHA-256 `7c34a5aeb714026e2624d64980999cc40612b6f230843516d5b6b9f59279a40b`, matching the original dataset.

Compiled `.vrb` files and packaged runtime copies were omitted because the retained source can regenerate them. Two source-free Python lock smoke reports remain as JSON; their delivery folders were omitted, so the report-recorded artifact hashes cannot restore those files.

## Retained source for exploratory runs

The Linux config-migration source and timing drivers are retained under [`config-migrator-linux/`](config-migrator-linux/) and [`scripts/`](scripts/). The latest lock smoke's small Python project source and manifest are under [`python-lock-smoke/`](python-lock-smoke/); its generated delivery folders are omitted. The final independent-v3 candidate source is retained separately at [`evaluation/results/independent-v3/source-amendment/`](../../../results/independent-v3/source-amendment/). These files support inspection and rerunning selected cases; they do not turn the exploratory timings into the formal completion suite.
