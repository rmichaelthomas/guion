# Richards fresh-source gap closure pass

Date: 2026-10-26
Branch: `codex/guion-richards-gap-closure`
Workload: canonical Richards, fresh Guion source compilation, CPython 3.14.7 reference

## Result

Guion remains faster than Python on this workload, but this pass did not reach the `0.60` Guion/Python ratio target. The clean baseline cohort measured 48.69 ms for Guion and 74.45 ms for Python, a ratio of `0.642` (paired 95% interval `0.620–0.664`). Other randomized cohorts put the baseline ratio between `0.642` and `0.656`; the target therefore needs about 4 ms less from a roughly 48–49 ms Guion run, or around an 8% reduction.

Every measured run checked Richards' expected output (`9297`, `23246`). The 15-trial cohorts randomized paired order, used two warmups, bypassed Guion's source cache, and left the OS file cache uncontrolled. Across these cohorts, the paired baseline ratio ranged from `0.642` to `0.665`; compare candidates within their own paired cohort because the Python and Guion medians shifted between sessions.

## What the phase profile says

The one-shot diagnostic profile attributed 30.51 ms to native JIT compilation, including 20.66 ms in Cranelift's `define` phase for the large `main` function. It estimated 15.80 ms of program execution after subtracting JIT compilation from the instrumented execution phase. These are phase measurements from an instrumented run, not host CPU samples. For 25 executions in one process, the profile estimated about 1.12 ms of compilation per execution and 10.51 ms of execution per run.

That points the next optimization at the size and cost of compiling `main`. Tweaks to the list helper alone did not produce a reliable first-run gain, and the VM-only route is much slower.

## Experiments

| Experiment | Fresh-source result | Decision |
| --- | --- | --- |
| Cranelift `opt_level=speed` | 13.4% slower than baseline; paired 95% interval `12.9–14.2%` slower. In a separate 25-run lane it was 19.0% faster per execution. | Reject for the fresh-source goal; it trades away startup for repeated execution. |
| Cranelift `single_pass` register allocation | 11.9% slower than baseline; paired 95% interval `9.7–14.1%` slower. Compilation got shorter, but execution slowed more. | Reject. |
| Static list-set helpers by type and copy mode | 0.2% faster by median; paired interval ranged from 3.8% faster to 0.9% slower. | No reliable improvement. |
| Separate profiled and ordinary list helpers | 0.4% faster by median; paired interval ranged from 4.0% faster to 1.6% slower. | No reliable improvement. |
| Guarded direct JIT writes for small local integer/boolean lists | 4.6% slower fresh-source; paired interval `2.1–6.5%` slower. A 25-run lane was 0.6% faster, a negligible change. | Reject; generated-code growth outweighed any repeated-run benefit. |
| VM-only source execution | 717 ms median versus 49 ms native and 75 ms Python in a five-trial diagnostic. | Not a viable way to skip JIT compilation for Richards. This is triage data, not a release benchmark. |

The rejected runtime changes have been removed from the branch. The benchmark reports remain as the record of what was tested and why it was not retained.

## Recommended next pass

Target native compilation cost directly. Reduce work emitted for Richards' `main` and lower the time spent in Cranelift `define`, while preserving bytecode fuel, cancellation, fallback, and source error behavior. The acceptance gate should be a fresh-source Richards ratio whose paired 95% upper bound is at most `0.60`, with the expected output checked on every run. Keep the repeated-execution lane as a secondary guard so a startup win does not hide a runtime regression.

The inline-store attempt increased `main` from 7,510 to 8,395 Cranelift IR instructions and slowed the complete fresh-source cohort. That makes a smaller IR lowering or a cheaper JIT compilation path a stronger next target than further hand-written list stores.

## Verification and hygiene

- Runtime library tests: 17 passed.
- Runtime execution tests: 109 passed, including alias preservation, native/VM parity, limits, and profiling.
- A broader package run also reached HTTP integration tests, but 11 tests could not open their local test sockets under the current macOS process permissions. This is separate from the list/JIT change; no permission escalation was attempted.
- Release builds were capped at two Cargo jobs. Benchmark processes ran serially. The 15-trial three-arm source cohort used about 2.7 seconds of user CPU and 0.5 seconds of system CPU across all arms.
- All generated binaries, temporary VM harness files, and per-run scratch directories are removed after preserving the JSON results below. The existing Cargo build cache is retained.

## Evidence

- [Baseline comparison](2026-10-26-gap-baseline-comparison.json)
- [Baseline phase profile](2026-10-26-gap-baseline-profile.json)
- [Cranelift speed comparison](2026-10-26-gap-candidate-comparison.json)
- [Cranelift speed repeated-run comparison](2026-10-26-gap-speed-repeated.json)
- [Cranelift speed phase profile](2026-10-26-gap-speed-profile.json)
- [Single-pass comparison](2026-10-26-gap-single-pass-comparison.json)
- [Single-pass phase profile](2026-10-26-gap-single-pass-profile.json)
- [Static list-set comparison](2026-10-26-gap-specialized-comparison.json)
- [Static list-set phase profile](2026-10-26-gap-specialized-profile.json)
- [Separate profiled-helper comparison](2026-10-26-gap-profile-fast-comparison.json)
- [Inline-store comparison](2026-10-26-gap-inline-comparison.json)
- [Inline-store phase probe](2026-10-26-gap-inline-phase-profile.json)
- [Inline-store baseline profile](2026-10-26-gap-inline-baseline-profile.json)
- [Inline-store repeated profile](2026-10-26-gap-inline-repeated-profile.json)
- [VM-only triage](2026-10-26-gap-vm-native-triage.json)
