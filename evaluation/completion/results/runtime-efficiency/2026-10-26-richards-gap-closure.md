# Richards fresh-source gap closure pass

Date: 2026-10-26
Branch: `codex/guion-richards-gap-closure`
Workload: canonical Richards, fresh Guion source compilation, CPython 3.14.7 reference

## Result

The `0.60` Guion/Python ratio gate is now met. In the same randomized 15-trial cohort, the baseline measured 48.21 ms and the candidate measured 36.28 ms, while Python measured 73.12 ms. Candidate/Python median ratio was `0.498` (paired 95% interval `0.483–0.511`); candidate/baseline was `0.761` (`0.744–0.776`), a 23.9% reduction in median elapsed time. The upper confidence bound clears the gate by 0.089.

Every measured run checked Richards' expected output (`9297`, `23246`). The 15-trial cohorts randomized paired order, used two warmups, bypassed Guion's source cache, and left the OS file cache uncontrolled. Across these cohorts, the paired baseline ratio ranged from `0.642` to `0.665`; compare candidates within their own paired cohort because the Python and Guion medians shifted between sessions.

## What the phase profile says

The one-shot diagnostic profile attributed 30.51 ms to native JIT compilation, including 20.66 ms in Cranelift's `define` phase for the large `main` function. It estimated 15.80 ms of program execution after subtracting JIT compilation from the instrumented execution phase. These are phase measurements from an instrumented run, not host CPU samples. For 25 executions in one process, the profile estimated about 1.12 ms of compilation per execution and 10.51 ms of execution per run.

The accepted change moved fuel reservations from every bytecode block to function entry and loop-region boundaries. It calculates the longest reachable instruction path until the next loop boundary or function return and reserves enough fuel before entering that region. Existing cancellation polls remain at the same function and loop entries. If the budget cannot cover the whole region, native execution falls back before entering it.

The change cut `main` from 652 to 358 Cranelift IR blocks (45.1%) and from 7,549 to 6,569 IR instructions (13.0%). Its measured Cranelift `define` time fell from 19.64 ms to 11.28 ms (42.6%), and total native compilation profile time fell from 27.10 ms to 17.01 ms (37.2%). The 25-execution lane also improved from 10.59 ms to 9.83 ms per execution (7.2%); the fresh-source win did not hide a repeated-execution regression.

## Experiments

| Experiment | Fresh-source result | Decision |
| --- | --- | --- |
| Cranelift `opt_level=speed` | 13.4% slower than baseline; paired 95% interval `12.9–14.2%` slower. In a separate 25-run lane it was 19.0% faster per execution. | Reject for the fresh-source goal; it trades away startup for repeated execution. |
| Cranelift `single_pass` register allocation | 11.9% slower than baseline; paired 95% interval `9.7–14.1%` slower. Compilation got shorter, but execution slowed more. | Reject. |
| Static list-set helpers by type and copy mode | 0.2% faster by median; paired interval ranged from 3.8% faster to 0.9% slower. | No reliable improvement. |
| Separate profiled and ordinary list helpers | 0.4% faster by median; paired interval ranged from 4.0% faster to 1.6% slower. | No reliable improvement. |
| Guarded direct JIT writes for small local integer/boolean lists | 4.6% slower fresh-source; paired interval `2.1–6.5%` slower. A 25-run lane was 0.6% faster, a negligible change. | Reject; generated-code growth outweighed any repeated-run benefit. |
| VM-only source execution | 717 ms median versus 49 ms native and 75 ms Python in a five-trial diagnostic. | Not a viable way to skip JIT compilation for Richards. This is triage data, not a release benchmark. |

Earlier rejected runtime changes have been removed from the branch. The benchmark reports remain as the record of what was tested and why it was not retained.

## Next target

The Richards-specific `0.60` gate is achieved. A follow-up that removed one intermediate array copy in the small-list arena did not produce a reliable fresh-source gain: candidate/baseline was `0.997` with paired 95% interval `0.983–1.008`. That implementation was discarded. The profile still reports 84,497 copy-on-write list copies per execution, but the older host profile attributed only 12 of 98 terminal samples to the list helper and 14 to allocator or memory movement; it predates the regional-fuel change and cannot attribute the current candidate's remaining CPU time.

The next compile-side probe is to avoid emitting per-instruction exit IP constants where no fuel fallback or cancellation check can consume them. The current `main` remains at 358 Cranelift IR blocks and 6,569 instructions after the accepted change. This is a hypothesis to measure, not an assumed bottleneck.

The inline-store attempt increased `main` from 7,510 to 8,395 Cranelift IR instructions and slowed the complete fresh-source cohort. That makes a smaller IR lowering or a cheaper JIT compilation path a stronger next target than further hand-written list stores.

## Verification and hygiene

- The candidate release binary built with locked, offline Cargo dependencies and two build jobs; `cargo fmt --all -- --check` passed. No Rust test suite was run for this candidate change.
- The randomized cohort used 15 trials per arm, two warmups, uncached source runs, serial execution, and output checks on every warmup and trial. OS file cache remained uncontrolled.
- Native diagnostic profiles also checked the expected output in their timed lanes. Their phase timings are instrumented diagnostics, separate from the uninstrumented comparison cohort.
- Baseline and candidate binaries and temporary run files are in ignored `target/perf-session/regional-fuel` scratch pending the next experiment; remove them after the follow-up. The existing Cargo build cache is retained.

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
- [Regional fuel reservation comparison](2026-10-26-richards-regional-fuel-comparison.json)
- [Regional fuel baseline profile](2026-10-26-richards-regional-fuel-baseline-profile.json)
- [Regional fuel candidate profile](2026-10-26-richards-regional-fuel-candidate-profile.json)
- [Inline list-copy probe](2026-10-26-richards-list-copy-comparison.json)
