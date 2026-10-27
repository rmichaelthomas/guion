# Text join chunk size

- Date: 2026-10-27
- Baseline revision: `eb46ba0` (32-character chunk threshold)
- Candidate: 64-character base chunks in `text_join_range`
- Same release CLI binary used in both arms; standard-library roots were pinned independently.

## Change

The balanced join helper now concatenates up to 64 pieces in each base case instead of 32. This keeps the public API and text semantics unchanged. A 16-piece threshold was also screened and rejected because its results regressed on several cases.

## Confirmatory results

Fifteen randomized paired trials per selected frozen case, with two warmup pairs. Each fresh source run bypassed the source cache and passed its output oracle; AllHereHTML outputs matched their expected hashes.

| Selected case | Candidate/baseline ratio (95% interval) | Candidate wins |
| --- | ---: | ---: |
| Release notes | 0.985 (0.974–0.998) | 12/15 |
| Document index | 0.986 (0.928–1.017) | 9/15 |
| JSONL summary control | 0.965 (0.937–1.003) | 10/15 |
| AllHereHTML 1× | 0.997 (0.985–1.006) | 10/15 |
| AllHereHTML 2× | 0.979 (0.969–0.998) | 11/15 |
| AllHereHTML 4× | 0.986 (0.980–0.996) | 12/15 |

The selected release-notes case improved by **1.5%** and AllHereHTML improved by **2.1% at 2×** and **1.4% at 4×**, with all three intervals below parity. The 1× AllHereHTML screen result did not reproduce in confirmation. Document-index, the JSONL control, and the 1× AllHereHTML case showed no reliable change. Keep the larger threshold for the repeatable gains, while treating the result as workload-specific.

## Verification and limits

- `cargo test -p guion-runtime --test stdlib_data --locked --offline --jobs 2`: passed, 13 tests.
- Every measured app output passed its frozen oracle; each AllHereHTML output matched its expected hash.
- Timings include process launch and oracle time; OS file cache was uncontrolled.
- The [screening data](2026-10-27-text-join-chunk-size-screen.json) records the 16- and 64-piece candidates; [confirmation data](2026-10-27-text-join-chunk-size-confirmation.json) contains all paired samples and intervals.
