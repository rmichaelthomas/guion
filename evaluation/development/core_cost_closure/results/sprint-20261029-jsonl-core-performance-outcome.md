# Guion JSONL core-performance sprint outcome

**Outcome: correctness and the unrelated-app holdout passed; the Python-parity performance gate remains open.** This report covers the approved 2026-10-29 implementation sprint on Debian 12 ARM64 with Rust 1.97.1 and Python 3.11.2. The final four-lane run used 15 randomized paired trials and checked each Guion and Python output against the frozen oracle.

## Final four-lane results

| Rows | Mode | Guion candidate median | Python median | Candidate/Python median (paired 95% interval) | Candidate/frozen Guion control (paired 95% interval) |
| ---: | --- | ---: | ---: | ---: | ---: |
| 20,000 | Uncached source | 0.280532 s | 0.091166 s | 2.9976 (2.9753–3.0496) | 1.0014 (0.9842–1.0155) |
| 20,000 | Artifact | 0.269558 s | 0.091166 s | 2.9599 (2.8912–3.0167) | 0.9966 (0.9625–1.0038) |
| 200,000 | Uncached source | 2.728588 s | 0.734498 s | 3.6779 (3.4355–4.1111) | 1.0152 (0.9423–1.0343) |
| 200,000 | Artifact | 2.651506 s | 0.734498 s | 3.5750 (3.3602–3.8708) | 1.0020 (0.9770–1.0542) |

All four upper confidence bounds remain above 1.0. The median Guion time would need to fall by about 66.2–72.8% across these lanes to reach parity, before accounting for interval uncertainty. Candidate/control medians are near parity, but their 95% intervals extend above the proposed 1.02 upper regression bound at 200k source (1.0343) and artifact (1.0542). Those two lanes remain unresolved against that guardrail. No speed change in this candidate has a statistically supported end-to-end gain.

Guion peak RSS was 9,699,328 bytes in all four lanes at both sizes, with no measured increase versus the control. The final report is [`sprint-20261029-final-current-clean-paired-15.json`](sprint-20261029-final-current-clean-paired-15.json). The control binary SHA-256 is `f2da40c3bf09e27d45523078ce3ffdaa97afaa75a179609424c0b9bfe4446c1b`; the candidate SHA-256 is `54d3be0172f9444a168eff50fa8564f179942e362ed8359145d1eca84cdb4cfc`. The raw report records the frozen inputs, runner/toolchain versions, trial order, timings, memory, and output-check status. Candidate source was based on revision `0c0239e1e9c042bc6a15f6ff8eeb1d07098e37aa`; the tracked source patch over that base has SHA-256 `8a211512ad30b4622c41f37b5ed0f21bf658c0923656bf1e77ecc0a4dead232c` across the six Rust source/test/manifest paths listed in the run manifest.

## Candidate decisions

- The mixed-value native bridge and opt-in native-tier candidate passed their correctness smoke but regressed the required lanes (candidate/control medians about 1.11–1.14). They were not retained as a default execution path.
- A direct Result/Option variant-probe path replaced the generic bridge for `is_ok`/`is_some` and preserved VM parity, but its output-checked three-pair smoke was slower still: candidate/control medians were 1.114–1.131 at 20k and 1.147–1.217 at 200k. The candidate/Python ratios were 3.43–4.10×. It was rejected at the smoke gate and its code was removed; raw evidence is [`sprint-20261029-native-direct-variant-smoke-3.json`](sprint-20261029-native-direct-variant-smoke-3.json). The helper unit test passed and the feature-enabled runtime suite passed 144/144 before this rejected code was reverted.
- The predecoded host-forwarder candidate passed output checks but regressed its three-pair smoke in every lane (candidate/control medians 1.069–1.501); the direct-forwarder variant also regressed every lane (1.105–1.138). Both were removed.
- A bounded VM stack-pool experiment was neutral in its 30-pair confirmation (medians 0.997–1.003 with intervals spanning parity) and was removed.
- Profile-specialized dispatch and the final candidate were non-regressing against the frozen control, but the 15-pair final intervals span parity. They are not reported as a speed win.
- Earlier allocation/last-use and text-concatenation candidates also failed their stated end-to-end gates and were removed. Detailed records remain in the adjacent raw result files.

These results reject the tested local dispatch, forwarding, and pooling candidates as a route to the required speedup. They do not establish that all possible compiler or runtime designs are exhausted. The next evidence-based implementation target is a genuine whole-function register/SSA execution plan for the measured `main`/JSON helper path, with explicit VM continuation maps; its compilation and transition costs must be included in the same paired workload. The profile shows approximately 61.5 million VM instructions and 3.77 million VM frames on the 200k workload, while native eligibility still excludes the mixed Result/JSON path. A per-operation generic bridge has already lost end-to-end time, so a new tier must keep common execution inside the function and side-exit with exact state.

## Holdout and correctness

The unchanged lockfile-auditor holdout matched all expected output for 512 packages and 1,024 diagnostics. Across 15 paired trials, the candidate/control median ratio was 0.9766 with a 95% interval of 0.9086–0.9993, inside the proposed 1.02 upper-bound guardrail. See [`sprint-20261029-lockfile-holdout-paired-15.json`](sprint-20261029-lockfile-holdout-paired-15.json).

The runtime execution integration suite passed **142/142**. The exact `compiler_stage_equality` test passed in **450.53 seconds**. These are focused results; a full workspace test suite was not run. The typed HTTP gate remains closed based on its separate 15-pair report; Python-bridge performance remains outside this Python-free core sprint.

## Gate status

The required JSONL gate is **open in all four lanes**. Outputs and the unrelated holdout are correct, memory is unchanged, and the candidate does not materially regress the holdout. Speed parity with pinned Python is still 2.96–3.68× away by median. The next implementation needs to reduce the measured guest dispatch/frame cost substantially; none of the tested changes in this sprint achieved that reduction.
