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
