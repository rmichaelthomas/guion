# Guion core optimization outcomes

Date: 2026-10-28
Workload: frozen 20,000- and 200,000-row JSONL source/artifact lanes
Environment: Debian 12 ARM64, pinned sprint container, offline after prefetch

## Decision

No speed change was retained from this sprint's candidate experiments. Both candidates were reverted because their paired intervals did not meet the predeclared retention gate. Their raw samples, patches, correctness checks, and decision notes are retained beside the profile evidence.

## Profile evidence

The 200,000-row workload used 200,947 physical lines. Paired decomposition measured a median of 0.3601 seconds for file reading and 1.2202 seconds for JSONL parsing plus line iteration; the latter was 3.3587 times the former (95% percentile interval 3.3265–3.4124). Output line counts matched on every run. The runtime profile recorded guest allocation-budget events and estimated guest bytes, not host allocator traffic. No CPU sampling or per-function attribution was available in this environment, so the decomposition identifies a costly workload phase but not a specific parser function to optimize.

## Candidate outcomes

| Candidate | Comparison | Decision |
| --- | --- | --- |
| Guarded skip in reserved-key scanning | At 20k rows, candidate/base median ratio 1.0053 (95% interval 0.9664–1.0237). At 200k, 1.0141 (0.9911–1.0235). Peak RSS did not show a consistent reduction. Existing parser correctness tests passed. | Reverted; candidate was neutral to slower and did not show a meaningful memory improvement. See `2026-10-28-jsonl-parser-fast-path.json`, `optimization-01.patch`, and `optimization-01.md`. |
| `text_join` for the date-summary key | On 200k rows, candidate/base ratio 0.99617 (95% interval 0.97605–1.01027); peak RSS medians were equal. Output checks passed. | Rejected as inconclusive because the interval crosses parity. See `2026-10-28-jsonl-date-key-text-join.json`, `optimization-02.patch`, and `optimization-02.md`. |

The measurements do not support a general speed claim or a retained optimization. The next useful step is to add a safe profiler or phase-level instrumentation that can attribute JSONL parse cost before selecting a narrower candidate.


## Final frozen-workload measurements

The final run used the same pinned Debian 12 ARM64 environment, CPython 3.14.7 reference, and final release candidate at source revision `7c7b8a9`. It completed seven workloads with output checks passing. Sixteen randomized trials were collected for each broad candidate. The report status is `failed` because the preset required speed win for `jsonl-summary-data` was not observed; the 200,000-row lane also remained slower than Python. This is a performance-gate failure, not a workload correctness failure.

| Workload | Guion artifact median | Python reference median | Paired ratio (95% interval) |
| --- | ---: | ---: | ---: |
| 20,000-row JSONL | 0.566 s | 0.121 s | 4.632 (4.477–4.758) |
| 200,000-row JSONL | 5.879 s | 0.810 s | 7.113 (6.684–7.384) |

The paired short-source comparison used a fresh locked release rebuild from frozen base `a1c0d40` (SHA-256 `d182bfffac526de8fe4828763558b523c14c1070af3c7c127863a16b9679ce92`) and candidate SHA-256 `d8d495abbe220e5602f4c63f940394b2b65cb6f186104fbee2cfdcc9312ffb6b`. Tiny source medians were 9.401 ms (base) and 9.256 ms (candidate), ratio 0.991 (95% interval 0.955–1.028), consistent with parity. The local HTTP JSON path measured 12.659 ms (legacy base) and 13.409 ms (typed candidate), ratio 1.064 (95% interval 1.006–1.210); 51 of 51 fixture requests were observed and output matched. The original preserved short-lane baseline (`f7d20351…`) did not match a same-toolchain base rebuild and is retained only as diagnostic evidence. See `2026-10-28-linux-arm64-final.json`, `2026-10-28-short-http-final.json`, and the completion report for raw data and methods.
