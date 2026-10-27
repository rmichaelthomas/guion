# Stable `sort_by` post-commit results

**Code commit:** `8e1b43b` (`perf(runtime): scale sort_by for large lists`)

**Release binary SHA-256:** `672d90d1e32462d256e76ad991e1e85ed86dd41ccb1670cdda0d29027cc0084d`

## What changed

`sort_by` now uses insertion sort for lists of 32 items or fewer and stable merge sort for larger lists. This changes the large-list comparison count from quadratic to O(n log n), keeps equal items in their original order, leaves input aliases unchanged, and charges temporary index storage to the guest allocation budget. The release-notes application now uses this builtin instead of repeatedly selecting and copying the next version.

## Large-list sort measurement

The comparison used the prior committed runtime binary (`5404e86`) and the release binary from `8e1b43b`. Each run launched a fresh process with source caching disabled, sorted reverse-ordered integers, and checked in-program that the output length and endpoints were correct. Timings include source compilation and execution; OS file caching was uncontrolled. Seven randomized pairs were run at each measured size.

| Items | Previous median | New median | New / previous | 95% interval |
| ---: | ---: | ---: | ---: | ---: |
| 128 | 8.24 ms | 5.85 ms | 0.710× | 0.702–0.741 |
| 512 | 47.31 ms | 7.30 ms | 0.153× | 0.150–0.160 |

At 512 items, the new runtime is about **6.5× faster** in this source-run test. At 1,024 and 2,048 items, the previous runtime hit the default instruction limit before completing; the new runtime completed and passed the value assertions. Those boundary results are single attempts, not timing estimates. The full raw samples and command outcomes are in [sort scale data](2026-10-27-sort-by-postcommit-scale.json).

The runtime suite passed **186 tests**. Formatting passed, the optimized CLI built successfully, and the release-notes frozen acceptance cases passed for both Guion and Python (17/17 each). The timed release-notes case measured a Guion/Python ratio of 0.239 (95% interval 0.223–0.299); that small four-version fixture is not large enough to establish the large-list sort gain by itself.

## Everyday application check

The current release binary was compared with the Python references on nine frozen everyday tasks. Each timed run checked its expected output. Guion had a lower median in all nine; ratios below 1 mean less time than Python.

| Task | Guion / Python median | 95% interval |
| --- | ---: | ---: |
| SLA report | 0.164× | 0.150–0.175 |
| JSONL summary | 0.159× | 0.153–0.168 |
| Release notes | 0.239× | 0.223–0.299 |
| Calendar conflicts | 0.682× | 0.621–0.690 |
| Webhook replay | 0.195× | 0.189–0.206 |
| Health digest | 0.834× | 0.825–0.857 |
| Config migrator | 0.143× | 0.127–0.148 |
| Documentation index | 0.189× | 0.173–0.202 |
| Lockfile auditor | 0.181× | 0.178–0.200 |

This is a workload-specific source-run comparison, not a general language-speed claim. Fixture setup is excluded, source caches are bypassed, and OS file caching is uncontrolled. The full samples are in [nine-task results](../real-task-sprint/2026-10-27-sort-by-postcommit-nine-apps.json).

The Python reference initially lacked its pinned `icalendar` package. The declared benchmark dependencies were installed once under ignored repository scratch, the affected case was rerun successfully, and then the full nine-task measurement completed. No dependency or environment files were added to the repository.

## Agent preference check

The same three exploratory preference evaluations were repeated against this commit. The score remains **3/3 for Guion**, unchanged from the previous 3/3 result. It cannot increase above that ceiling. Current full acceptance also passed for both implementations: health digest 10/10 cases and 33/33 checks; lockfile auditor 17/17 and 43/43; webhook replay 9/9 and 98/98. See [acceptance evidence](../real-task-sprint/2026-10-27-sort-by-postcommit-preference-acceptance.json) and [agent preference results](../real-task-sprint/2026-10-27-sort-by-postcommit-agent-preference.json).

The lockfile auditor evaluation identified a specific next opportunity: its diagnostic output still uses a quadratic selection-and-copy loop. Replacing that application-level loop with the now-efficient stable `sort_by` is the next everyday-task experiment; large generated lockfiles should be used to measure it.
