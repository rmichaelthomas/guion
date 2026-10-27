# Richards typed list-update helpers

Date: 2026-10-27
Branch: `codex/guion-richards-gap-closure`
Host: macOS ARM64; Python reference: CPython 3.14.7

## Change

The native compiler now calls a list-update helper selected by the list's known element type. The runtime no longer receives and branches on a per-call type tag. The helper still enforces the existing bounds, budget, and copy-on-write behavior; the typed entry points share the same implementation.

## Results

| Lane | Previous Guion | Candidate Guion | CPython 3.14.7 | Candidate / previous (paired 95% interval) |
| --- | ---: | ---: | ---: | ---: |
| Fresh source | 28.784 ms | 28.134 ms | 74.905 ms | 0.990 (0.962–1.010) |
| 25 executions per process | 6.045 ms per execution | 5.830 ms per execution | 37.011 ms per execution | 0.966 (0.961–0.969) |

The fresh-source result is inconclusive: its interval includes no change, with 11 of 30 paired runs tying or slower. The repeated lane shows a reliable 3.4% reduction, with 2 of 30 pairs tying or slower. The fresh candidate/Python ratio was 0.373 (95% interval 0.369–0.381).

Generated native code fell from 4,086 to 4,048 Cranelift instructions in both lanes. Neither candidate lane deoptimized.

## Method and limits

Both lanes used randomized paired order, 30 trials per arm, two warmups, and an output check on every run. The canonical Richards output was `9297` and `23246`. Guion's source cache was bypassed; the OS file cache was uncontrolled. The Python repeated lane reloads the reference module within one process, so the candidate/previous Guion comparison is the useful result for that lane.

## Verification

The regression test checks native/VM agreement and verifies that updates to Int, Bool, and Float64 lists preserve the original lists. It also checks the reduced generated-code instruction count. The runtime suite passed 178 tests; formatting and the locked offline release build passed with two Cargo jobs.

The [machine-readable comparison](2026-10-27-richards-typed-list-set-comparison.json) retains the trial samples, execution order, binary hashes, and native backend profiles.
