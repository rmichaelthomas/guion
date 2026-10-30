# SDD ledger — plan: docs/superpowers/plans/2026-10-29-guion-jsonl-performance-research-plan.md

Plan revision: 2; user approved execution on 2026-10-29.
Branch: codex/guion-core-sprint-spec.
Base: 0c0239e1e9c042bc6a15f6ff8eeb1d07098e37aa.
Workspace: current named Guion worktree; no source edits at start.
Execution constraints: use existing container guion-core-north-star-sprint; do not start/restart/stop/clean Colima or containers; no external evaluator/delegation; preserve unrelated data; use python3 only.
Container: id 4e0bf3187312, image sha256:6a45e30e542ab1dc5600a1e5a3a7579e3d520107a5fb0c1471af6e6f1e3ca1ac, rustc 1.97.1, cargo 1.97.1, Python 3.11.2; 47G available on mounted filesystem.
Environment finding: docker exec with a login shell drops Cargo from PATH; non-login sh finds Rust in /usr/local/cargo/bin. No daemon change was made.
Preflight: source/spec read; plan-to-spec scope checked; existing target cache 4.0G; previous benchmark temp 7.4G; other long-running Python processes were idle and left untouched.

Task 1: baseline and causal profile are complete. The 15-pair frozen baseline passed its output oracle at both sizes and both source/artifact modes. Exact and sampled profile reports identify `main`, `jsonl_next`, `json_get_safe`, `json_int_safe`, and `json_text_safe` as the measured hot path; the exact 200k control executed 61,476,177 VM instructions across 3,770,554 VM frames. Native eligibility compiled zero functions; `main` and `jsonl_next` are first blocked at `IsOk`, and the JSON helpers at typed JSON/Result operations. The focused `compiler_stage_equality` test later passed on the current source in 450.53 seconds.

Task 2: approved execution-tier and local-operation experiments, implementation correctness work, and their benchmark decisions are complete. The current source passed the focused VM/bootstrap checks. The final candidate reached parity with its frozen Guion control but did not demonstrate a statistically clear end-to-end speed win. All rejected candidates and exact output-check results are recorded below and in the adjacent raw JSON.

Task 2: implementation experiments and correctness checks are recorded below. The profile-specialized candidate reached the final paired run but did not demonstrate an end-to-end speed win.
Task 3: final four-lane comparison and unchanged lockfile-auditor holdout are complete; correctness passed, the holdout met its guardrail, and the Python speed gate remains open. A whole-function register/SSA interpreter was not implemented in this sprint; it is the next candidate rather than a claimed result.
Task 4: outcome report and README/status updates are complete with the gate explicitly open. Focused runtime and bootstrap tests passed; the full workspace suite was not run.


Task 2 update — current candidate measured, no speed win.

The profile-specialized candidate, with the nonwinning forwarder and stack-pool experiments removed, passed the final four-lane 15-pair output-checked run. Exact result: `results/sprint-20261029-final-current-clean-paired-15.json`; candidate SHA-256 `54d3be0172f9444a168eff50fa8564f179942e362ed8359145d1eca84cdb4cfc`. Candidate/Python median ratios (paired percentile 95% intervals) are: 20k source **2.9976** (2.9753–3.0496), 20k artifact **2.9599** (2.8912–3.0167), 200k source **3.6779** (3.4355–4.1111), and 200k artifact **3.5750** (3.3602–3.8708). Candidate/control ratios are 1.0014 (0.9842–1.0155), 0.9966 (0.9625–1.0038), 1.0152 (0.9423–1.0343), and 1.0020 (0.9770–1.0542), respectively. No interval separates from parity; the Python gate remains open in all four lanes. The candidate/control 95% upper bounds are 1.0343 and 1.0542 in the 200k source and artifact lanes, exceeding the proposed 1.02 regression bound and leaving those lanes unresolved. Guion peak RSS was 9,699,328 bytes in all four lanes at both sizes.

The unchanged lockfile-auditor holdout passed exact output checks: `results/sprint-20261029-lockfile-holdout-paired-15.json`; 512 packages, 1,024 diagnostics, 15 paired trials; candidate/control median 0.9766, percentile 95% interval 0.9086–0.9993. This meets the proposed ≤1.02 upper-bound nonregression guardrail.

The required focused `compiler_stage_equality` test now passes on this source in 450.53 seconds, and the runtime execution suite passed 142/142. This is not evidence that the full workspace suite passes.

The bounded predecoded host-forwarder comparison passed correctness but failed its three-pair speed smoke (candidate/control medians 1.069–1.501); its direct variant measured 1.105–1.138. Those variants were removed. A whole-function register interpreter remains untested and is documented as the next engineering candidate; this report does not describe the performance objective as achieved.

## Approved JSONL closure plan — implementation started

On 2026-10-29 the user approved `jsonl-closure-plan-20261029.md` for implementation on this branch. This is a new implementation phase; the measurements and conclusions above remain the historical baseline and are not overwritten.

Implementation sequence deviation: runtime/compiler production edits were already present when this plan was approved in the active session, before the four focused benchmark fixtures and their hashes were created. Do not present the plan's prescribed fixture-first sequence as having been followed. The current production edits are being treated as an unmeasured candidate; focused fixtures/oracles are being created next before additional product changes. The source edits so far add native tagged Result/Option handling and typed map-record update operations. They have targeted correctness coverage but no speed-closure claim.

The new work uses the existing Guion per-function Cranelift SSA backend, rather than introducing a second backend. Result/Option tag tests and unwrapping are now native operations; JSON host operations remain explicit bridge calls under the existing opt-in feature. The map-record operation uses normal value semantics and copy-on-write for aliases, with counters for in-place updates and copied bytes. No Docker/Colima/container lifecycle action was taken. No external evaluator or delegated work was started.

Current verification at implementation start: the three focused map-record VM tests passed after fixing the direct-local update detection; the default runtime execution suite passed 143 tests before the map-record op was integrated. The focused Result/Option native execution test passed. The feature-gated JSON/native execution test passed before final direct-operation table cleanup and must be rerun. Remaining compatibility work includes artifact-format docs, self-hosted compiler/serializer support, new workload fixtures, paired performance measurements, and the final regression checks. No speed-gate result is available for this candidate yet.


Rejected direct Result/Option variant probe. A compact arena-tag helper replaced the generic mixed-value bridge for `is_ok`/`is_some`, with exact VM continuation on invalid handles. The helper unit test passed and the native-value-bridge execution suite passed 144/144, including allocation-limit, exact-fuel, cancellation, and JSONL checks. The isolated candidate binary `65f07fd51151366a58e07d6a36c6581dfb2c8e28c680388aca2a790d34eeb6c2` passed all output checks, but its three-pair candidate/control medians were 1.1143/1.1310 at 20k artifact/source and 1.2168/1.1472 at 200k artifact/source. Candidate/Python ratios were 3.43–4.10×. It failed the smoke gate and was reverted; see `results/sprint-20261029-native-direct-variant-smoke-3.json`. The tracked source patch and default candidate binary returned to the prior final measured identity.

## Approved JSONL closure implementation — measured outcome

The implementation work is recorded in `results/sprint-20261029-core-cost-closure-lazy-tag-clone-o1-paired-15.json`; final candidate SHA-256 is `47842d73f21159c3ed693ddbff1b813b1b7ba35e873cae3a3972552ffebe11af`. The runtime now keeps the first eight tagged Result/Option descriptors inline in each native invocation and spills only overflow into fixed-capacity chunks. Clone-allocation charges are computed and cached only when a tagged value is cloned. The callback uses the stable descriptor address for constant-time lookup.

One intermediate lazy-charge implementation searched the descriptor arena on every clone. The 200k Result/Option artifact exceeded a 10-second isolation timeout, so its first paired cohort was stopped; the container remained running. Restoring constant-time lookup fixed that regression. On the same artifact the earlier candidate took 112 ms, the corrected candidate 57 ms, and the frozen control 353 ms. The corrected focused 15-pair result-option lane is 0.156× the control in source mode (95% interval 0.151–0.160) and 0.132× in artifact mode (0.132–0.138); it remains about 2.0× Python.

Final JSONL paired results at 200k rows:

- Original JSON state: candidate/control is 1.416× source (1.361–1.451) and 1.388× artifact (1.369–1.415); candidate/Python is 5.696× source (5.259–5.930) and 5.645× artifact (5.267–5.915).
- Compact record: candidate/control is 1.445× source (1.358–1.465) and 1.481× artifact (1.464–1.503); candidate/Python is 4.191× source (3.799–4.404) and 4.380× artifact (4.002–4.424).
- Nested map record: candidate/Python is 4.404× source (4.373–4.548) and 4.344× artifact (4.229–4.362).

The original and compact lanes are materially slower than the frozen Guion control, so the core speed gate remains **open**. Guion uses about 10 MB peak RSS versus Python's 69 MB at 200k rows. The nested-map profile records 588,608 in-place updates out of 588,723 map-record updates, with 115 copy-on-write cases. The text-character ablation is 1.153× slower than text-slice in source and 1.167× in artifact mode. Strict-invalid input, denied outside-root access, and oversized input all failed with the expected diagnostics and left output absent in source and artifact modes.

The profile identifies the remaining execution gap: in the default CLI build, `main` is rejected from native compilation at `GetField` because the mixed-value bridge is disabled. A diagnostic feature-enabled build still rejects `main` because native type/control-flow inference cannot prove it eligible. The 200k default candidate profile executes 52,818,284 VM instructions across 2,765,826 frames. Enabling the existing native-value-bridge feature alone does not close the hot application path. The next meaningful engineering task is to extend whole-function native eligibility/type-flow across the mixed record, JSON, and Result/Option control flow in `main`; this sprint did not implement that broader tier.

Final verification against the corrected source: `cargo test --locked --offline -p guion-runtime` passed 239 tests across unit, execution, HTTP, lifecycle, data, and service suites. The `native-value-bridge` execution suite passed 153/153. The feature-enabled 200k diagnostic is a single profiled run, not a paired speed result. No Docker/Colima/container lifecycle action or outside evaluator was used. The Python parity target was not achieved; no commit was made.
