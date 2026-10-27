# Integer division by negative powers of two

Date: 2026-10-27
Branch: `codex/guion-richards-gap-closure`
Host: macOS ARM64; Python reference: CPython 3.14.7

## Change

The native compiler now recognizes a literal negative power-of-two divisor written with unary minus. For `//`, `/`, and `%`, it uses sign-correct shifts, masks, and small adjustments instead of general signed divide and remainder operations. It specializes magnitudes through 2^62. Dynamic divisors and `-1` stay on the existing checked path, preserving the `i64::MIN / -1` arbitrary-precision fallback. The signed `i64::MIN` divisor is also left on the general path.

## Results

The fixture processes 25,000 values spanning negative and positive integers. Every iteration performs three floor divisions, three remainders, and three truncating divisions by negative powers of two. All runs produced `-328117`.

| Lane | Previous Guion | Candidate Guion | CPython 3.14.7 | Candidate / previous (paired 95% interval) |
| --- | ---: | ---: | ---: | ---: |
| Fresh source | 9.204 ms | 8.383 ms | 38.797 ms | 0.904 (0.856–0.934) |
| 25 executions per process | 1.183 ms per execution | 0.635 ms per execution | 11.854 ms per execution | 0.534 (0.527–0.543) |

Fresh-source candidate/Python was 0.214 (95% interval 0.203–0.220); the repeated-lane ratio was 0.0531 (0.0529–0.0539). Three of 30 fresh-source pairs and one of 30 repeated-lane pairs tied or regressed against the previous Guion build.

Generated native code fell from 728 to 595 Cranelift instructions in the fresh-source lane and from 1,038 to 905 in the repeated lane. The candidate did not deoptimize in either measurement.

## Method and limits

Both lanes used randomized paired order, 30 trials per arm, two warmups, and an output check on every run. Guion's source cache was bypassed with `--no-cache`; the OS file cache was uncontrolled. The repeated lane ran 25 calculations in one process and reports elapsed time per calculation. This is a focused arithmetic workload, not a general Python performance claim.

## Verification

The regression test compares native and VM results against hand-calculated expectations for `-4`, `-8`, and `-2^62`, including negative values and signed 64-bit boundaries. It verifies that dynamic operations remain equivalent, `-1` triggers the arbitrary-precision fallback, and specialized functions emit fewer native instructions. The runtime suite passed 177 tests; formatting and the locked offline release build passed with two Cargo jobs.

The [Guion fixture](../../performance/fixtures/power-of-two-negative-division/main.gu), [Python reference](../../performance/fixtures/power-of-two-negative-division/reference.py), and [machine-readable comparison](2026-10-27-negative-power-of-two-division-30-trial-comparison.json) are included for reuse.
