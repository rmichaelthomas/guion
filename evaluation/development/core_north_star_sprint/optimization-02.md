# Optimization experiment 02: JSONL date-key construction

## Evidence and hypothesis

The output-checked 200,000-row runtime profile reports about 3.92 GB in estimated guest allocation-budget charges, with no list copy-on-write activity. The file-read/JSONL-parse decomposition accounts for 1.22 s in parsing and iteration, while the full application median is 5.33 s. Source inspection shows the application formats every parsed timestamp as UTC, materializes all Unicode scalars with `text_chars`, then concatenates the first ten scalar strings with nine guest `Text + Text` operations to form its date map key. That pattern is executed once per input event and is a plausible high-allocation guest path; the runtime profile does not attribute those charges to this expression.

This diagnostic candidate changes only the workload source expression to call the existing native-backed `text_join` on the same ten scalars. It preserves the Unicode scalar sequence and output, and tests whether repeated guest concatenation is material at this scale. This is an application-level probe, not a claim about improving runtime internals.

## Correctness and comparison policy (set before candidate build)

- Baseline source and artifact are from revision `a1c0d40f1445e6806238ae252fc1842d0a18a6c1`; executable SHA-256 `d3ed076fdc32a7a53f0f923675f78cb8c80b5f80e98759dd0866f7df98f9ff34`.
- Both artifacts run in the same existing Debian 12 ARM64 container on the same 200,000-row JSONL input. Every warmup, timed trial, and memory sample must match the independent Python output oracle.
- Measurement: 2 warmups, 16 randomized paired trials, fresh artifact process, raw samples and 95% paired interval, then 3 direct-child peak RSS samples per arm. Timer is child-process wall time through exit; oracle checks are immediately afterward and excluded.
- Retain a workload-source change only if the paired 95% interval's upper bound is below 1.0 and candidate median peak RSS is no more than baseline median plus 1 MiB. Otherwise keep it as a diagnostic result and make no code change.

## Result

All warmup, timed, and peak-memory outputs matched the independent Python oracle. The `text_join` variant median was 5.543 s versus 5.571 s for baseline (candidate/baseline median ratio 0.9962; paired 95% interval 0.9760–1.0103). Both peak RSS medians were 10,616,832 B. Because the interval crosses parity, the preset gate rejects this as inconclusive; the application source is unchanged. Raw samples and commands are in `evaluation/completion/results/real-task-sprint/2026-10-28-jsonl-date-key-text-join.json`; baseline and candidate source plus the exact diff are retained under `evaluation/development/core_north_star_sprint/profile-inputs/` and `optimization-02.patch`.
