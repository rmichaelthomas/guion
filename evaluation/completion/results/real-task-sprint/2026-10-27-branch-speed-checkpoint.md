# Guion speed sprint checkpoint — 2026-10-27

## What changed since the sprint baseline

There is no single honest speed multiplier across unlike programs. The measured changes are workload-specific:

| Workload | Sprint starting result | Latest useful result | Change and scope |
| --- | --- | --- | --- |
| Richards scheduler | 48.38 ms Guion at shared VM baseline `9d57434` | 24.72 ms Guion; 65.61 ms Python | About 49% less Guion time, or 1.96× the starting throughput. The latest measured candidate is a working tree based on `bd0687a`, not branch HEAD `b83b989`; Richards has not been remeasured at HEAD. |
| JSONL summary, 200,000 rows | 4.160 s Guion at sprint baseline | 3.690 s at commit `54603bd` | About 11.3% less time. This is a separate seven-pair, output-checked comparison; it is not a measurement of HEAD. |
| Lockfile audit, 4,096 packages | 80.5 ms Guion vs 113.7 ms Python in the earlier scale run | 77.84 ms Guion vs 117.02 ms Python at HEAD | Guion remains about 1.51× faster than Python. The head-to-head against runtime baseline `6004e9a` measured 77.84 vs 77.70 ms (ratio 0.992; 95% interval 0.974–1.027), so the latest runtime change did not establish another speed gain. |
| Interleaved calendar conflicts, 8,000 events per calendar | No matching large-scale baseline | 1.230 s Guion vs 37.671 s Python | About 30.3× faster on this deliberately large, synthetic workload. This is an application-specific result, not a general language multiplier. |

The branch contains 38 commits beyond `origin/main`. The best-supported overall conclusion is that Guion made large gains on Richards and a more modest gain on large JSONL processing, while application-shaped workloads vary. On the selected everyday application cases, the latest comparisons range from roughly 1.2× to 6.4× faster than the shared Python reference. Those are comparisons of named implementations and inputs, not proof of a universal advantage over optimized Python.

## Latest lockfile boundary check

The current release binary at `b83b989` was measured in seven randomized trials per arm. At 4,096 packages (8,192 diagnostics), all outputs matched the expected sorted JSON. Guion's median was 77.843 ms and Python's was 117.017 ms; the Guion/Python ratio was 0.664 (95% bootstrap interval 0.647–0.680).

At 8,192 packages (16,384 diagnostics), both Guion binaries reached the unchanged default `R_STEP_LIMIT` at `main.vr:305:40` while assembling report entries. Python passed its single preflight in 144.6 ms. This confirms the scale boundary remains open; this run did not raise the runtime limit or produce a Guion timing at that size.

The trials used fresh Guion processes with source cache bypassed and checked every successful output. Fixture setup was outside the timer. OS file-cache state was uncontrolled. The baseline and candidate app source hashes match, so the 4,096 head-to-head isolates the Guion runtime builds rather than app-source edits.

Raw records: [lockfile head benchmark](2026-10-27-lockfile-jsonsetlocal-head.json), [starting Richards benchmark](richards-post-vm-baseline.json), [latest measured Richards comparison](../../runtime-efficiency/2026-10-27-shared-text-richards-fresh-source-comparison.json), [sprint-start JSONL baseline](../../runtime-efficiency/2026-10-26-baseline.json), [latest measured JSONL comparison](../../runtime-efficiency/2026-10-27-json-shared-tree-postcommit-200k.json), and [calendar scale report](2026-10-27-calendar-interleaved-scale.md).

## Stop point

This is a clean stopping point after the current benchmark and its reporting. The known 8,192-package instruction-budget failure remains a concrete next target. The latest completed agent-preference result remains 3/3 for Guion; the requested fresh rerun did not execute because the local evaluator could not access its private state database, so no new score is claimed.
