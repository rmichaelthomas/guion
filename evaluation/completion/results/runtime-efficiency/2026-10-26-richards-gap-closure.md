# Richards fresh-source gap closure pass

Date: 2026-10-26
Branch: `codex/guion-richards-gap-closure`
Workload: canonical Richards, fresh Guion source compilation, CPython 3.14.7 reference

## Result

The Richards-specific `0.60` Guion/Python ratio gate is met by the corrected candidate. In its randomized 15-trial cohort, baseline Guion measured 48.64 ms, corrected Guion measured 37.48 ms, and Python measured 73.56 ms. Candidate/Python median ratio was `0.510` (paired 95% interval `0.496–0.515`); candidate/baseline was `0.780` (`0.753–0.791`), a 22.0% reduction in median elapsed time. The upper confidence bound clears the `0.60` gate by 0.085.

Every warmup and timed run checked Richards' expected output (`9297`, `23246`). The cohort randomized paired order, used two warmups, bypassed Guion's source cache, and left the OS file cache uncontrolled. See the corrected comparison [JSON](2026-10-26-richards-call-boundary-retained-fuel-comparison.json).

**Correction to the first report:** the earlier `0.498` result (36.28 ms; 358 main-function IR blocks) came from a candidate that returned unused parent fuel before a native call but did not start a new fuel-reservation region at the call continuation. That meant the speed result did not establish correct instruction-budget behavior on that path. It is superseded and must not be used as the accepted result. The original raw comparison remains in the evidence list as historical experiment data.

## Follow-up: remove the call-continuation boundary

After preserving the parent's fuel reservation across native calls, the compiler no longer needs to start a new reservation region immediately after each call. In a randomized 30-trial follow-up, the latest candidate measured 36.55 ms versus 37.13 ms for the preceding corrected build and 73.33 ms for Python. The paired candidate/preceding-build ratio was `0.992` (95% interval `0.979–0.999`), a small 0.8% median improvement; 10 of 30 paired samples were ties or regressions. Candidate/Python was `0.502` (`0.494–0.505`). Use the 30-trial [comparison](2026-10-27-richards-no-call-continuation-boundary-30-trial-comparison.json) as the primary follow-up result; the initial 15-trial probe is preserved [here](2026-10-27-richards-no-call-continuation-boundary-comparison.json).

The diagnostic profile shows the expected compile-time change: `main` fell from 376 to 358 Cranelift IR blocks and from 6,629 to 6,569 IR instructions. Measured `define` time fell from 14.46 ms to 11.84 ms, while total native compilation profile time fell from 19.93 ms to 17.50 ms. The 25-execution diagnostic medians were 9.619 ms and 9.594 ms per run, effectively flat; the profile sessions were separate, not paired. These lower-level data support a smaller JIT workload, while the end-to-end improvement remains modest. See the [candidate profile](2026-10-27-richards-no-call-continuation-boundary-profile.json).

This candidate also has 358 `main` IR blocks, the same count as the superseded first candidate, but it keeps the caller's unused reservation available to the caller across child calls. The existing exact-budget parity test passed for nested VM/JIT/fallback execution, as did the IR-metering invariant test.

## What the phase profile says

The corrected one-shot diagnostic profile measured 19.93 ms of native JIT compilation, including 14.46 ms in Cranelift's `define` phase for `main`. These are instrumented phase timings, not host CPU samples. `main` fell from 652 to 376 Cranelift IR blocks and from 7,549 to 6,629 IR instructions. Its measured `define` time fell from 23.00 ms to 14.46 ms (37.1%); total native compilation profile time fell from 30.64 ms to 19.93 ms (35.0%).

In a separate 15-trial phase-profile session, the 25-execution lane median was 10.68 ms per run for baseline and 9.62 ms for the corrected candidate. Its candidate p95 was 11.44 ms versus 11.20 ms for baseline, so treat the lower median as directional; this was not a randomized paired comparison. The candidate's one-shot profile lane also had a 127.7 ms outlier. The output-checked, uninstrumented paired cohort above is the primary speed result.

The corrected change reserves enough instruction fuel for each straight-line region and loop path. Native callers retain their unused reservation across child calls; each call continuation starts a new region, which checks whether the remaining reservation covers that path and otherwise returns it before requesting more. Unused parent fuel is returned on depth-limit and child-failure exits. A focused Richards run with a 1,000-instruction budget produced the same `R_STEP_LIMIT` diagnostic at the same source location for baseline and candidate. The existing nested-call budget parity test also passed across VM, JIT, and fallback.

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

The call-boundary correction was measured in two implementations. Releasing and refilling the parent reservation at each call continuation also passed the Richards ratio gate; retaining the parent reservation across native child calls avoids that repeated handoff while preserving the exact low-budget outcome. The latter is the candidate retained here. Its release binary SHA-256 and source fingerprint are recorded in the comparison and profile evidence.

## Follow-up probes

The Richards-specific `0.60` gate is achieved. Two further optimization hypotheses were measured and their code was discarded:

- Removing an intermediate array copy in the small-list arena measured candidate/baseline `0.997` with paired 95% interval `0.983–1.008`. The direct-copy version was output-correct but produced no reliable fresh-source gain.
- Emitting exit IP metadata only at fuel, cancellation, or operation-fallback points removed 497 IR instructions from `main` (6,569 → 6,072). Its 30-trial candidate/baseline median ratio was `1.003` (95% interval `0.999–1.014`), with medians 36.87 ms versus 36.40 ms. This did not establish a fresh-source improvement. Instrumented compilation profiles showed a smaller phase change: total native compile 16.52 → 15.89 ms and `main` define 10.98 → 10.82 ms. The 25-run lane was 9.69 → 9.77 ms per execution. The reduced IR did not justify retaining more compiler bookkeeping without a measured end-to-end win.

The list-copy profile still counts 84,497 copy-on-write list copies per execution, but the older host profile attributed only 12 of 98 terminal samples to the list helper and 14 to allocator or memory movement. That profile predates the regional-fuel change and cannot attribute the current candidate's remaining CPU time. The measurements do not support treating list copying as the established next CPU bottleneck.

The inline-store attempt increased `main` from 7,510 to 8,395 Cranelift IR instructions and slowed the complete fresh-source cohort. That makes a smaller IR lowering or a cheaper JIT compilation path a stronger next target than further hand-written list stores.

## Verification and hygiene

- The candidate release binary built with locked, offline Cargo dependencies and two build jobs; `cargo fmt --all -- --check` passed. The focused existing `nested_calls_keep_exact_instruction_budget_parity_across_vm_jit_and_fallback` and `native_fuel_metering_adds_control_flow_per_bytecode_block` tests passed in release mode; the full Rust test suite was not run.
- Baseline and candidate both returned `R_STEP_LIMIT` with the same message and source location at instruction limit 1,000; the machine-readable comparison is [here](2026-10-26-richards-call-boundary-budget-check.json).
- Randomized cohorts used 15 trials per arm, two warmups, uncached source runs, serial execution, and output checks on every warmup and trial; the extended exit-metadata cohort used 30 trials per arm. OS file cache remained uncontrolled.
- Native diagnostic profiles also checked the expected output in their timed lanes. Their phase timings are instrumented diagnostics, separate from the uninstrumented comparison cohort.
- Builds were limited to two Cargo jobs. Benchmark cohorts and profile lanes ran serially; the 30-trial exit-metadata cohort used 4.33 seconds of user CPU and 0.86 seconds of system CPU.
- Experiment binaries and run scratch are temporary under ignored `target/perf-session/` and will be removed after the next same-branch optimization probe. The existing Cargo build cache is retained.

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
- [Call-boundary release/refill comparison](2026-10-26-richards-call-boundary-release-refill-comparison.json)
- [Call-boundary release/refill profile](2026-10-26-richards-call-boundary-release-refill-profile.json)
- [Call-boundary retained-fuel comparison](2026-10-26-richards-call-boundary-retained-fuel-comparison.json)
- [Call-boundary baseline profile](2026-10-26-richards-call-boundary-baseline-profile.json)
- [Call-boundary corrected candidate profile](2026-10-26-richards-call-boundary-retained-fuel-profile.json)
- [Call-boundary instruction-budget check](2026-10-26-richards-call-boundary-budget-check.json)
- [Call-continuation-boundary 15-trial probe](2026-10-27-richards-no-call-continuation-boundary-comparison.json)
- [Call-continuation-boundary 30-trial comparison](2026-10-27-richards-no-call-continuation-boundary-30-trial-comparison.json)
- [Call-continuation-boundary phase profile](2026-10-27-richards-no-call-continuation-boundary-profile.json)
- [Inline list-copy probe](2026-10-26-richards-list-copy-comparison.json)
- [Exit-metadata 15-trial comparison](2026-10-26-richards-exit-args-comparison.json)
- [Exit-metadata 30-trial comparison](2026-10-26-richards-exit-args-30-trial-comparison.json)
- [Exit-metadata baseline profile](2026-10-26-richards-exit-args-baseline-profile.json)
- [Exit-metadata candidate profile](2026-10-26-richards-exit-args-profile.json)
