# Optimization experiment 01: JSON reserved-key pre-scan

## Evidence and hypothesis

The frozen Linux ARM64 baseline's generated 200,000-row JSONL application has a 5.326 s Guion source median versus 0.799 s for the Python reference. A same-input artifact decomposition isolates file iteration at 0.360 s and `jsonl_next` parsing plus iteration at 1.220 s (15 randomized paired trials; parse/read median ratio 3.359, percentile 95% interval 3.326–3.412). Runtime counters also show substantially more guest allocation charges for parsing than line reading. The exact outputs were checked on every trial.

Source inspection finds `parse_unique_shared_json` calls `escape_reserved_json_keys` before Serde parses every JSON text. That helper scans quoted tokens to detect the serde-private reserved key even for ordinary lines with no backslash escapes. The candidate will take a fast return only when the raw JSON contains neither a backslash nor the reserved spelling; any escaped input or possible reserved key continues through the existing logic. This is a source-informed hypothesis based on the measured parsing cost; the container cannot attribute CPU samples to individual functions.

## Correctness and comparison policy (set before candidate build)

- Baseline revision: `a1c0d40f1445e6806238ae252fc1842d0a18a6c1`.
- Baseline executable: `target/perf-session/cargo/release/guion`, SHA-256 `d3ed076fdc32a7a53f0f923675f78cb8c80b5f80e98759dd0866f7df98f9ff34`; preserve a copy before rebuilding in that target directory.
- Correctness: run all `unique_json_parser_tests` before and after; every benchmark arm must match the independent Python-generated 200,000-row JSON output on warmups and timed samples. Preserve duplicate-key, malformed JSON, arbitrary-precision, and escaped/literal reserved-key behavior.
- Measurement: same Debian 12 ARM64 container, same compiler flags and Cargo target path, `--offline`, fresh Guion artifact process, 2 warmups and 16 randomized paired trials. Report raw samples, paired median ratio, and bootstrap 95% interval. Capture output-checked peak RSS for each arm with the existing direct-child `RUSAGE_CHILDREN` method.
- Retain only if every output check passes, the paired 95% interval's upper bound is below 1.0, and candidate median peak RSS is no more than 1 MiB above baseline. If the initial interval crosses 1.0 but the median ratio is below 0.99, run a 32-pair confirmation; otherwise label the candidate inconclusive and revert it.

## Result

The candidate passed the 8 existing `unique_json_parser_tests` and all full-application outputs matched the Python oracle. It failed the predeclared performance gate and was reverted. At 20k rows the candidate/baseline median ratio was 1.0053 (95% interval 0.9664–1.0237) with RSS medians 10,485,760 B candidate and 10,616,832 B baseline. At 200k rows the ratio was 1.0141 (95% interval 0.9911–1.0235), with RSS medians 10,616,832 B candidate and 10,485,760 B baseline. The 200k interval crosses parity and RSS is 128 KiB higher, so no confirmation or retention is warranted. Full raw samples and commands are in `evaluation/completion/results/real-task-sprint/2026-10-28-jsonl-parser-fast-path.json`; the exact candidate diff is retained in `optimization-01.patch`. No runtime performance code from this candidate remains in the worktree.
