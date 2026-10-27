# Single-scalar text delimiter fast path

- Date: 2026-10-27
- Baseline revision: `bb31cc3`
- Candidate: direct single-scalar comparison in `text_matches_at`
- Binary SHA-256: unchanged between arms; standard-library source hashes identify the change

## Change

`text_matches_at` now compares the only scalar directly when the delimiter has length one. Empty delimiters, multi-scalar matching, Unicode scalar semantics, and trailing empty fields keep their previous behavior. This changes no public API.

## Results

Fifteen randomized pairs per selected application, with two warmup pairs. Each fresh Guion source run bypassed the source cache and passed the frozen output oracle. The baseline loaded the standard library from commit `bb31cc3`; the candidate loaded the edited working-tree standard library.

| Selected frozen case | Candidate/baseline ratio (95% interval) | Candidate wins |
| --- | ---: | ---: |
| Document index | 1.007 (0.984–1.021) | 5/15 |
| Release notes | 0.972 (0.923–0.974) | 14/15 |
| JSONL summary control | 0.973 (0.965–1.063) | 10/15 |

The release-notes case was **2.8% faster** with its full interval below parity. Document index and the JSONL control showed no reliable change; their intervals cross 1. The result supports keeping this narrow fast path for release-note workloads, but does not show a general text-processing gain.

## Verification and limits

- `cargo test -p guion-runtime --test stdlib_data --locked --offline --jobs 2`: passed, 13 tests, including ASCII, Unicode, empty-input, trailing-field, and replacement behavior.
- Every measured output matched its frozen oracle.
- Timings include process launch and oracle checks; OS file cache was uncontrolled.
- Full samples, paired ordering, source hashes, and intervals are in the [head-to-head data](../real-task-sprint/2026-10-27-text-split-single-scalar-head-to-head.json).
