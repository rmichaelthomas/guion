# Richards follow-up: literal division by two

Date: 2026-10-27
Branch: `codex/guion-richards-gap-closure`
Workload: canonical Richards; macOS ARM64; CPython 3.14.7 reference

## What changed

The native compiler recognizes literal division and remainder by positive two in straight-line code. It lowers `value // 2` to a signed arithmetic shift and `value % 2` to a one-bit mask. Both preserve Guion's floor-based results for negative values. Dynamic divisors and other integer operators keep their existing lowering.

## Results

| Measurement | Previous build | Candidate | Candidate / previous (paired 95% interval) |
| --- | ---: | ---: | ---: |
| Fresh source, one execution | 30.045 ms | 28.554 ms | 0.940 (0.930–0.967) |
| Fresh source, 25 executions per process | 7.164 ms per execution | 6.061 ms per execution | 0.847 (0.843–0.854) |

CPython 3.14.7 measured 73.617 ms on the fresh-source Richards workload. Candidate/CPython was 0.389 (paired 95% interval 0.382–0.395). Four of 30 fresh-source candidate/baseline pairs and one of 30 repeated-lane pairs were ties or regressions.

The repeated-lane native profile fell from 409 to 398 Cranelift IR blocks and from 4,234 to 4,086 instructions. Neither build deoptimized during the measured run.

## Method and limits

Both cohorts used randomized paired order, 30 trials per arm, two warmups, output checks on every run (`9297`, `23246`), and `--no-cache` to bypass Guion's source cache. The OS file cache was uncontrolled. These are Richards measurements on one macOS ARM64 host; they do not establish cold-disk performance or a general speed claim over Python.

## Verification

The new regression checks literal `// 2` and `% 2` with negative values and the signed 64-bit boundaries, verifies native and VM results, and limits generated IR for the two functions to fewer than 451 instructions. The generic lowering produced 489. The full runtime crate suite passed 160 tests, formatting passed, and the release CLI built offline with two Cargo jobs.

Machine-readable evidence: [30-trial fresh-source comparison](2026-10-27-richards-power-of-two-fresh-source-30-trial-comparison.json) and [30-trial 25-execution comparison](2026-10-27-richards-power-of-two-25-run-30-trial-comparison.json).
