# Shared immutable text storage

- Date: 2026-10-27
- Branch: `codex/guion-richards-gap-closure`
- Baseline commit: `bd0687a4e9cc8abc76725c17397f2db9102fe704`
- Baseline binary SHA-256: `b7d3499e5548e0e901eaf14cb19052b38cf3af32436bf4c75beab35b13abd000`
- Candidate binary SHA-256: `f85a615353bc1479be161c99335a799e6018751fa37465006ad269c16865e31f`
- Host: macOS ARM64; Richards Python reference: CPython 3.14.7

## Change

Guion text values now use immutable, reference-counted UTF-8 storage. Cloning a `Text` value shares its bytes instead of copying a `String`; this particularly benefits lists of text, such as the character lists indexed repeatedly by the AllHereHTML builder. The guest allocation quota retains its established logical text-copy charges, so the storage optimization does not relax configured allocation limits.

## Results

The output-checked AllHereHTML comparison ran 15 randomized baseline/candidate pairs per size, with two warmup pairs. Each fresh process bypassed Guion's source cache, and every generated HTML file matched its expected SHA-256.

| AllHereHTML template | Baseline | Candidate | Candidate / baseline (paired 95% interval) | Candidate wins |
| ---: | ---: | ---: | ---: | ---: |
| 42,225 bytes | 337.1 ms | 313.0 ms | 0.941 (0.926–0.948) | 14 / 15 |
| 84,931 bytes | 637.2 ms | 625.4 ms | 0.973 (0.967–1.000) | 12 / 15 |
| 170,325 bytes | 1,268.4 ms | 1,220.5 ms | 0.972 (0.964–0.975) | 14 / 15 |

This is a repeatable improvement on the text-heavy application, from about 2.7% at 2× input to 5.9% at the original input size. The 2× interval is very close to no change, while the other two intervals are more clearly below 1.

Richards serves as the compute-heavy guardrail. In 30 randomized, output-checked fresh-source trials, the median was 24.536 ms before and 24.720 ms after; candidate/baseline was 1.008 (95% interval 0.999–1.013), so the difference is inconclusive. In a separate 15-trial lane with 25 executions per process, the median was 5.511 versus 5.503 ms per execution; candidate/baseline was 1.004 (0.996–1.006), also inconclusive. Every Richards run matched `9297` and `23246`. The candidate remained faster than CPython on this workload, at a candidate/Python median ratio of 0.376.

## Interpretation and limits

This optimization helps a real, text-heavy everyday program and does not show a reliable change on Richards. It is evidence for the workload it measured, not a general claim that Guion is faster than Python. All source caches were bypassed; the OS file cache was uncontrolled, so this is not a cold-disk test. The AllHereHTML comparison measures Guion against the preceding Guion build, not against Python. The existing nine-application acceptance and agent-choice evaluations remain separate evidence.

The paired AllHereHTML harness is [`compare_allherehtml_builds.py`](../../compare_allherehtml_builds.py). Machine-readable timings, trial order, outputs, and hashes are in the [AllHereHTML comparison](2026-10-27-shared-text-allherehtml-comparison.json), [Richards fresh-source comparison](2026-10-27-shared-text-richards-fresh-source-comparison.json), and [Richards repeated comparison](2026-10-27-shared-text-richards-25-run-comparison.json).
