# Lockfile Auditor Scale Checkpoint — 2026-10-27

## Result

The lockfile auditor is faster than the Python reference from 64 through 4,096 packages in this output-checked, fresh-process source benchmark. At 4,096 packages, Guion's median was 80.5 ms versus Python's 113.7 ms (0.730× the Python time; 95% bootstrap interval 0.705–0.819). At 2,048 packages, the medians were 46.4 ms versus 94.2 ms (0.494×).

The existing frozen lockfile acceptance set passes all 17 cases in both languages. The same package fixtures and exact expected sorted JSON were checked at every scale point.

## Changes measured

- Cache the package count and policy collection sizes before the package loop; skip empty deny, exception, and version-policy lookups.
- Parse each distinct package SemVer once and reuse it for repeated versions.
- Keep each package's findings in sorted rule order and merge those small sorted lists for duplicate package names. This removes a separate sort for every package-name group while preserving the report order.
- Keep the default runtime instruction and allocation limits unchanged.

## Scale boundary

At 8,192 packages, Python completes in about 130 ms, while Guion reaches its default `R_STEP_LIMIT` during report assembly. The prior Guion implementation also exceeds its allocation budget at this size. The current implementation does not yet cover this workload; the limit was not raised to make the benchmark pass.

Manual JSON text assembly was tested and discarded: it regressed the 4,096-package median to about 159 ms. The measured results here use Guion's JSON values and serializer.

## Measurement method

Each point uses seven randomized paired trials. The timer includes a fresh Guion process, source compilation with cache bypassed, and execution; fixture generation and acceptance checking are outside the measured interval. Every output is compared to its expected sorted JSON. OS file cache state is uncontrolled, so these are warm-machine, fresh-process observations, not cold-disk timings.

The raw records are in [the scale data](2026-10-27-lockfile-scale.json), [the 8,192-package limit probe](2026-10-27-lockfile-scale-8192-limit.json), and [the frozen acceptance report](2026-10-27-lockfile-acceptance.json). The repeatable benchmark is [benchmark_lockfile_diagnostics.py](../../performance/benchmark_lockfile_diagnostics.py).

## Next investigation

The remaining lockfile gap at 8,192 packages is an instruction-budget problem during per-diagnostic report creation, not an allocation failure in the current implementation. Keep the default limits intact while evaluating whether a generally useful runtime or JSON path can lower guest instruction work. The next everyday-task candidate is health-digest, which was the closest Guion/Python timing in the prior nine-application comparison.
