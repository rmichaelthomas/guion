# ASCII scalar storage follow-up

- Date: 2026-10-27
- Branch: `codex/guion-richards-gap-closure`
- Previous source revision: `5232bbb` (shared text storage, before ASCII scalar reuse)
- Previous binary SHA-256: `8370dae716a06bd9bd638069b306a82a5c991b991c86c0f097220b29c81b80cc`
- Candidate binary SHA-256: `a8728d613aaf43c3f81ed0fedc5d59fa49ea38709da2067a264d8b2bad55e65d`
- Host: macOS ARM64

## Change

`text_chars` previously created a new immutable string allocation for every scalar it returned. ASCII scalars now reuse lazily initialized, process-wide immutable one-character strings. Non-ASCII scalars still produce their own text values. The VM's existing logical allocation charges remain unchanged, so configured guest allocation limits retain their previous meaning.

The change is covered by `text_chars_reuses_immutable_storage_for_ascii_scalars`, which checks that repeated ASCII scalars share the same immutable storage. Existing Unicode scalar behavior is covered by the neighboring `text_chars_materializes_unicode_scalars_for_linear_scanning` test.

## Large everyday workload: 20,000 JSONL records

The direct comparison used seven randomized pairs and two warmup pairs. Guion's source cache was bypassed. Each run's output was checked against both the independent Python and SQLite expected results, and the output SHA-256 matched `50149835d981ac4b910c89429b8e07e3d72e20cd044f47fdf23b89d7e5f0bda9`.

| Build | Median fresh-source time |
| --- | ---: |
| Previous shared-text build | 364.5 ms |
| ASCII scalar reuse candidate | 352.8 ms |

The candidate/previous paired median ratio was **0.969** (95% bootstrap interval **0.967–0.971**), and the candidate was faster in **7/7** pairs. The OS file cache was uncontrolled. Full samples, trial order, input and source hashes are in the [machine-readable comparison](2026-10-27-ascii-text-scalar-jsonl-20000-head-to-head.json).

A separate randomized comparison on the same 20,000-row input measured the candidate against CPython 3.14.7 at **0.216** (95% interval **0.216–0.226**), about **4.6× faster** for these implementations. This is workload-specific; it does not establish a general speed advantage over Python. See the [Guion/Python result](2026-10-27-ascii-text-scalar-jsonl-20000-comparison.json).

## AllHereHTML

The same two builds were compared on three AllHereHTML template sizes using 15 randomized pairs per size and two warmup pairs. Every generated page matched its expected SHA-256.

| Template | Previous | Candidate | Candidate/previous (paired 95% interval) | Candidate wins |
| ---: | ---: | ---: | ---: | ---: |
| 42,225 bytes | 318.9 ms | 310.9 ms | 0.979 (0.961–0.989) | 13/15 |
| 84,931 bytes | 612.1 ms | 597.0 ms | 0.980 (0.966–0.985) | 13/15 |
| 170,325 bytes | 1,218.6 ms | 1,195.3 ms | 0.981 (0.973–0.989) | 13/15 |

These are roughly 1.9–2.1% reductions against the immediately previous build. The intervals remain below 1 across all three sizes. See the full [AllHereHTML comparison](2026-10-27-ascii-text-scalar-allherehtml-head-to-head.json).

## Broader everyday-task check

All nine established applications were compared head-to-head with seven randomized pairs and two warmup pairs for each selected case. Output checks passed in every run. The table reports candidate/previous time ratios; values below 1 favor the candidate.

| Application | Ratio (95% interval) | Candidate wins |
| --- | ---: | ---: |
| SLA report | 1.022 (0.930–1.154) | 3/7 |
| JSONL summary | 0.961 (0.920–1.068) | 4/7 |
| Release notes | 1.006 (0.987–1.025) | 3/7 |
| Calendar conflicts | 1.031 (0.973–1.039) | 2/7 |
| Webhook replay | 0.981 (0.971–1.016) | 5/7 |
| Service health | 0.994 (0.9917–0.9996) | 6/7 |
| Config migrator | 1.063 (0.954–1.127) | 3/7 |
| Document index | 0.987 (0.950–1.018) | 4/7 |
| Lockfile audit | 1.053 (0.992–1.095) | 2/7 |

These short-task samples do not establish meaningful regressions or gains outside the measured JSONL and AllHereHTML workloads. The service-health difference is under 1% and its fixed network timeout dominates execution time. The full current acceptance rerun passed all **127/127** Guion cases and all **127/127** Python-reference cases. See the [head-to-head timing data](../real-task-sprint/2026-10-27-ascii-text-scalar-nine-task-head-to-head.json), [Python comparison](../real-task-sprint/2026-10-27-ascii-text-scalar-nine-task-comparison.json), and [acceptance results](../real-task-sprint/2026-10-27-ascii-text-scalar-nine-task-acceptance.json).

## Verification and limits

- `cargo test -p guion-vm-core --locked --offline --jobs 2`: passed, 7 tests.
- `cargo test -p guion-runtime --test execution --locked --offline --jobs 2`: passed, 116 tests.
- The nine-application acceptance comparison passed 127 cases for each implementation.
- All timings are fresh-process source runs with Guion source caching disabled. OS file caching was uncontrolled.
- The separate three-task agent-choice rerun remained 3/3 for Guion, but it used frozen timing data from before these text optimizations. It does not show whether this change improves agent preference.

The results support continuing runtime optimization in text-heavy everyday workloads. They do not establish that Guion is generally faster than Python.
