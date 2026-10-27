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

At this checkpoint, Python completed in about 130 ms while Guion reached its default `R_STEP_LIMIT` during report assembly. The prior Guion implementation also exceeded its allocation budget at this size. The then-current implementation did not cover this workload; the limit was not raised to make that benchmark pass. The later post-merge follow-up below resolved this boundary with an application-specific ceiling.

Manual JSON text assembly was tested and discarded: it regressed the 4,096-package median to about 159 ms. The measured results here use Guion's JSON values and serializer.

## Measurement method

Each point uses seven randomized paired trials. The timer includes a fresh Guion process, source compilation with cache bypassed, and execution; fixture generation and acceptance checking are outside the measured interval. Every output is compared to its expected sorted JSON. OS file cache state is uncontrolled, so these are warm-machine, fresh-process observations, not cold-disk timings.

The raw records are in [the scale data](2026-10-27-lockfile-scale.json), [the 8,192-package limit probe](2026-10-27-lockfile-scale-8192-limit.json), and [the frozen acceptance report](2026-10-27-lockfile-acceptance.json). The repeatable benchmark is [benchmark_lockfile_diagnostics.py](../../performance/benchmark_lockfile_diagnostics.py).

## Next investigation

At the time, the remaining lockfile gap at 8,192 packages was an instruction-budget problem during per-diagnostic report creation, not an allocation failure in the current implementation. The follow-up confirmed the 8,192 fixture needs 2,154,912 guest instructions and resolved it with a project-only ceiling, without changing Guion's global defaults.

## Post-commit recheck

After commit `6004e9a`, the same seven-pair scale benchmark again passed at every size through 4,096 packages. At 4,096, Guion took a median 78.0 ms versus Python's 111.6 ms (ratio 0.698; 95% interval 0.690–0.709). The frozen acceptance rerun passed all 17 cases and 43 checks for each language. [Post-commit scale data](2026-10-27-lockfile-scale-postcommit.json) and [acceptance](2026-10-27-lockfile-acceptance-postcommit.json) record the rerun.

The three existing agent evaluations again chose Guion (3/3), unchanged from the previous maximum score. The lockfile policy-report case remained about 5.6× faster in Guion; health and webhook results also favored Guion. See the [preference results](agent-choice/2026-10-27-lockfile-scale/summary.json).

Health-digest initially looked closest to parity, but its selected fixture intentionally waits for a 700 ms endpoint until a 500 ms timeout. With that single delay removed and its expected result adjusted, the seven-pair diagnostic measured 33.8 ms for Guion and 127.7 ms for Python (ratio 0.260; 95% interval 0.256–0.305). That gap shows the original 0.839 ratio was dominated by the fixture's network wait, so health is not the next runtime optimization target. Calendar-conflicts also contains fixed 100 ms endpoint delays; isolate its parsing cost before treating its end-to-end ratio as a language-runtime gap.

## Post-merge 8,192-package correction — 2026-10-27

The merged baseline at `653165a` failed the 8,192-package output-checked case at the default 2,000,000-instruction ceiling. A limit-boundary probe found **2,154,912** was the lowest passing configured instruction ceiling for this fixture. The lockfile auditor now declares a **2,250,000-instruction project ceiling** in its own `veritas.toml`; Guion's global default remains 2,000,000. A repeatable regression test checks the complete sorted JSON report for 8,192 packages.

Seven randomized, output-checked fresh-source pairs measured **131.1 ms for Guion** and **136.3 ms for Python** at 8,192 packages. The paired median ratio was **0.974** (95% interval **0.960–0.992**). At 4,096 packages, Guion remained faster: **76.2 ms versus 114.5 ms** (ratio **0.666**, 95% interval **0.653–0.670**), with no reliable change against the pre-follow-up Guion build (ratio **0.992**, interval **0.976–0.996**).

Three output-checked peak-RSS samples at 8,192 packages measured **51.2 MB for Guion** and **42.2 MB for Python**; Guion used about **21% more peak memory** at this size. At 4,096 packages, Guion used **31.3 MB** and Python **38.0 MB**. The scale boundary is now covered, but the 8,192 memory gap and behavior at larger inputs remain optimization targets. See the [follow-up report](2026-10-27-lockfile-scale-followup.md), [timing and memory data](2026-10-27-lockfile-scale-postmerge-fix.json), [17-case acceptance](2026-10-27-lockfile-acceptance-postmerge-fix.json), and [8,192-package regression test](../../performance/test_lockfile_scale.py).
