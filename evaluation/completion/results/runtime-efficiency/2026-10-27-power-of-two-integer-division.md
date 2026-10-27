# Truncating integer division by powers of two

Date: 2026-10-27
Branch: `codex/guion-richards-gap-closure`
Host: macOS ARM64; Python reference: CPython 3.14.7

## Workload

The fixture loops over 25,000 negative-to-zero integers. Each iteration performs truncating integer division by 1, 2, 4, 8, and 16, then accumulates the results. The matching Python reference implements truncation toward zero using integer arithmetic. Both produce `-605454689`. This is a targeted arithmetic microbenchmark.

## Results

| Lane | Previous Guion | Candidate Guion | CPython 3.14.7 | Candidate / previous (paired 95% interval) |
| --- | ---: | ---: | ---: | ---: |
| Fresh source | 8.507 ms | 7.932 ms | 36.835 ms | 0.921 (0.908–0.958) |
| 25 executions per process | 0.679 ms per execution | 0.482 ms per execution | 9.746 ms per execution | 0.713 (0.695–0.727) |

Candidate/Python was 0.211 (paired 95% interval 0.206–0.224) for fresh source and 0.0494 (0.0484–0.0505) for the repeated lane. Seven of 30 fresh-source pairs and two of 30 repeated-lane pairs tied or regressed.

Generated native code for the fresh-source run fell from 53 to 43 Cranelift IR blocks and from 567 to 482 instructions. For the repeated wrapper, it fell from 80 to 70 blocks and from 877 to 792 instructions. Neither candidate lane deoptimized.

## Method and limits

Both lanes used randomized paired order, 30 trials per arm, two warmups, and an output check on every run. Guion's source cache was bypassed; the OS file cache was uncontrolled. The repeated lane runs 25 calculations in one process, and reports elapsed time per calculation. The result shows a gain for this focused loop; it does not predict the speed of arbitrary Guion programs.

## Verification

The native backend recognizes positive power-of-two constants for `/` and computes a truncation-corrected signed shift. The regression checks divisors 1, 2, 4, and 2^62 against hand-derived results and VM execution, including negative values and the signed 64-bit limits. It compares generated instruction counts with dynamic division. The runtime crate suite passed 176 tests, formatting passed, and the release CLI built with locked offline dependencies and two Cargo jobs.

The [Guion fixture](../../performance/fixtures/power-of-two-integer-division/main.gu), [Python reference](../../performance/fixtures/power-of-two-integer-division/reference.py), and [machine-readable comparison](2026-10-27-power-of-two-integer-division-30-trial-comparison.json) are included for reuse.
