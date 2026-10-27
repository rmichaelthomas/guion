# Richards native list-update mode specialization

- Date: 2026-10-27
- Branch: `codex/guion-richards-gap-closure`
- Candidate commit: `17255af`
- Host: macOS ARM64; Python reference: CPython 3.14.7

## Change

The native compiler now selects list-update helpers using both the known element type and update mode. A local update can mutate its owned list; an explicit `list_set` copies. The generated call no longer passes a dynamic copy-mode flag. The runtime keeps the same bounds checks, allocation budget, alias behavior, and native/VM results.

## Results

| Lane | Previous Guion | Candidate Guion | CPython 3.14.7 | Candidate / previous (paired 95% interval) |
| --- | ---: | ---: | ---: | ---: |
| Fresh source | 28.004 ms | 27.617 ms | 73.189 ms | 0.988 (0.976–0.997) |
| 25 executions per process | 5.991 ms/execution | 5.990 ms/execution | 38.903 ms/execution | 0.997 (0.984–1.013) |

Fresh source improved by about 1.2%; the paired interval is below 1. In the repeated lane, the median changed by less than 0.3% and the interval includes no change, so this build does not establish a steady-state Richards speedup. Guion remains faster than the Python reference in both measured lanes.

The repeated run's native code fell from 4,048 to 4,010 Cranelift instructions (38 fewer); neither build deoptimized.

## Method and limits

Both lanes used randomized paired order, 30 trials per arm, two warmups, and output checks on every warmup and trial. Each run produced the canonical Richards output `9297` and `23246`. Guion's source cache was bypassed. The operating-system file cache was uncontrolled. The repeated lane runs 25 workload executions per process and reports time per execution; its Python wrapper reloads the reference module for each execution.

## Verification

The regression test compares native and VM behavior for copy-required and local Int, Bool, and Float64 updates, including preservation of copied-list aliases. It confirms local updates lower to `ListSetLocal` and caps the generated test function at 461 Cranelift instructions (the pre-change version produced 467). The runtime suite passed 178 tests; formatting and the locked offline release build passed with two Cargo jobs.

The [machine-readable comparison](2026-10-27-richards-list-set-copy-mode-comparison.json) includes raw samples, trial order, binary hashes, and native backend profiles.
