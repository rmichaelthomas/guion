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

## Native host-callback boundary follow-up — 2026-10-30

The measured implementation adds operation-specific native callbacks for JSON parsing/access and JSONL line reads, and keeps VM callers in the VM tier when their call chain reaches those host callbacks. The 200k paired results are recorded in `results/sprint-20261029-native-tier-boundary-jsonl-200000-paired-5.json` and `results/sprint-20261029-native-tier-boundary-jsonl-20000-paired-5.json`. At 200k, candidate/control artifact ratios were 0.6585 for original JSON state (95% interval 0.6423–0.6994) and 0.6221 for compact records (0.5176–0.6266); source ratios were 0.6402 (0.6190–0.7122) and 0.6048 (0.5520–0.6251). Output and failure-case checks passed. The original lane remained 3.88× Python in artifact mode, so the parity gate remains open.

The separate no-native-entry guard did not show an additional JSONL win over the immediately preceding candidate: its five-pair 200k ratios were 0.9997 for original artifact mode (95% interval 0.9901–1.4032) and 0.9870 for compact-record artifact mode (0.9606–1.0133). These direct results are in `results/sprint-20261029-native-no-entry-vs-tier-boundary-jsonl-200000-paired-5.json`. A broader runtime check changed the decision: `guion_public_service_modules_call_typed_native_apis` took 251.34 seconds with that guard disabled and 24.60 seconds with it enabled; an earlier guarded run was 28.26 seconds. This self-hosted compiler workload is distinct from JSONL, and the guard is retained for this demonstrated benefit. The JSONL results do not support claiming that the guard speeds JSONL.

Final-source verification with the guard retained: `cargo test --locked --offline -p guion-runtime` passed 244 tests. The rebuilt release binary matched the measured candidate SHA-256 `3c19a580153303f3adb48c94f43654fb83a0a2d430b6b294ab956e0bf8fe7061`. A full workspace test attempt passed the nine-application acceptance case (500.56 seconds) but was stopped after the self-hosting `compiler_stage_equality` test remained CPU-bound for over six minutes; it is not recorded as a full workspace pass. The PR's workspace CI remains the gate for that check. `cargo fmt --all -- --check` was unavailable because rustfmt is not installed in the container; `git diff --check` passed.


## Implementation continuation — 2026-10-30

The quota-faithful `TextChars` constant-index concatenation fast path is retained as an end-to-end improvement. The exact 15-pair output-checked result is `results/sprint-20261030-jsonl-textchars-quota-paired-15.json`; candidate SHA-256 `27bca691f20e10d5591d95563f8b001f83b9d902694fdf052d947aa81acf0595`, immediate control SHA-256 `2289793912b6da890119fce4500bb128c2b6a5ce94dd1199fd788aae9b2633ec`. Candidate/control medians and paired percentile 95% intervals: 20k artifact **0.9219** (0.9119–0.9434), 20k source **0.9315** (0.9178–0.9584), 200k artifact **0.9275** (0.9080–0.9360), 200k source **0.9287** (0.8957–0.9576). All four intervals exclude parity in the favorable direction; output oracles passed. The focused runtime execution suite passed 159/159. The 200k lane remains about 2.93–2.94× Python and the 20k lane 2.51–2.52× Python, so this is useful progress but not closure. Peak RSS did not materially change.

A preliminary on-demand variant incorrectly undercharged allocation and was rejected. The retained implementation replays logical list, index-clone and concatenation charges at their original instruction spans; a dedicated differential allocation-limit test and full paired output oracle pass. Earlier whole-function planning/scanning and the on-demand quota-bypass candidates are not retained as measured wins.

The next implementation target remains broad native-tier coverage for the hot JSONL caller: the exact artifact profile shows native JSON helpers compile but `main` is rejected by native type/control-flow inference, leaving the dominant loop in the VM. A diagnostic now records the bytecode location where native type inference stops; the existing container is healthy and its live source mount is the Guion worktree. This task is ongoing; no native-tier speed claim is made yet.

## Native JSON loop arena guard — 2026-10-30

The direct native `JsonSetLocal` / `JsonPushLocal` candidate exposed a more basic constraint: native value handles are retained in `NativeListArena` for the lifetime of the invocation, and the backend has no loop safepoint that can retire dead handles. On the frozen 200k workload with the optional services argument omitted, the candidate compiled `main` and reached 887 MB RSS after 58 seconds; the run was stopped. A bounded 10k comparison showed the same path at 2.0 seconds / 159.5 MB, versus the preserved control at 0.173 seconds / 9.2 MB. Supplying `--services` caused an early deoptimization at `MapSet` and masked the problem; both configurations still matched the independent oracle.

Implemented a native eligibility guard: loops that directly or transitively allocate JSON/file/text values stay in the VM until the native arena can reclaim dead handles. The range-aware check examines the loop body, so value creation outside a loop does not reject it. The rejection report uses `native_loop_value_arena_retention` and identifies the backedge. Direct JSON-constructor and transitive helper-call regression tests cover the guard; numeric, typed-list, and record-loop tests confirm those loops remain eligible. The final runtime execution integration suite passes 196/196, and the release build succeeds.

The initial guarded candidate completed the exact 200k no-services invocation in 2.683 seconds at 10.3 MB RSS with exact output equality. A subsequent range-aware refinement completed the same exact invocation in 2.155 seconds at 10.4 MB RSS, also with oracle equality. Three-pair official smoke reports are preserved in `results/sprint-20261030-jsonl-native-loop-guard-20k-3.json` and `results/sprint-20261030-jsonl-native-loop-guard-200k-3.json`; these are preliminary and superseded by the final paired cohort below.

The full 15-pair comparison is preserved in `results/sprint-20261030-jsonl-native-loop-guard-paired-15.json`. Correctness and output-oracle checks passed at 20k and 200k in source and artifact modes. Against the preserved Guion control, candidate/control median ratios (paired percentile 95% intervals) were: 20k source **0.9261** (0.9152–0.9348), 20k artifact **0.9292** (0.9224–0.9301), 200k source **0.8807** (0.8495–0.8869), and 200k artifact **0.8726** (0.8588–0.8796). All 15 paired trials favored the candidate in every lane. Median execution times were 0.228s vs 0.248s at 20k source, 0.227s vs 0.245s at 20k artifact, 2.042s vs 2.320s at 200k source, and 2.025s vs 2.323s at 200k artifact.

The candidate remains slower than Python: candidate/Python ratios were 2.6230 (2.5790–2.6608) for 20k source, 2.6038 (2.5517–2.6530) for 20k artifact, 3.0507 (2.7961–3.0826) for 200k source, and 3.0199 (2.7790–3.0479) for 200k artifact. At 200k, median Python execution was 0.672s. Candidate peak RSS stayed near 10 MiB; Python peaked at 66.1 MiB and the Python/SQLite reference at 77.7 MiB. The guard therefore prevents the severe native-arena memory growth and retains a reproducible 12–13% improvement over the preserved Guion control, but does not close the speed gate.

The final candidate compiled 25 helper functions, while `main` remained in the VM. The report pinpoints its rejection at the loop backedge, bytecode instruction 225, because the loop allocates values retained by the native arena until invocation end. This confirms the remaining bottleneck: the hot per-row control-flow and value path still executes in the VM. The approved sprint's speed target remains open; safe reclamation or an ownership-aware representation for loop-local native values is the next architecture-level candidate, not a result claimed by this sprint.


## Reclamation feasibility study — 2026-10-31

Environment: Colima restarted by explicit user approval; the existing `guion-core-north-star-sprint` container was reused (rustc 1.97.1, Python 3.11.2, 4 CPUs, 6 GiB). About 9 GiB of stale target/scratch data was removed from it (debug incremental cache and an old `/tmp/guion-core-cost-closure/target`); no images or other projects' containers were touched. `scripts/guion-bench-shell.sh` runs commands in that container, starting Colima/the container if needed.

Container baseline for branch commit `43a48af` (binary SHA-256 `7b4b0fad…de01`, 200k rows, 5 interleaved output-checked trials, no `--services`): Guion artifact median **1.457 s**, Guion source **1.471 s**, Python **0.648 s**, so about **2.25×** Python. Raw: `results/sprint-20261031-container-baseline-43a48af-200k-5.json`. Earlier records show ~3.0× on the original host; the container run is the reference for this study, and the two should not be mixed.

CPU sampling of the VM path (`audit_tools/guion_pc_sampler.c`, 3 runs, 4,417 samples): 23.8% of all samples are inside the VM dispatch function, about 25% in libc malloc/free, and about 8% in quota/limit bookkeeping (`value_limit_error_with_json`, `cloned_value_allocation`, `charge_allocation`, `shared_json_metrics`, `value_matches`). Dispatch is therefore an upper bound of roughly one quarter of runtime: even a free register/predecoded interpreter could not take 2.25× below about 1.7×. A whole-function register interpreter was **not built** because the ceiling cannot reach parity.

Native `main` with the loop guard disabled (temporary experiment, since removed; feature `native-value-bridge`):

| rows | VM (guarded) | native main, original arena | native main, O(1) arena lookups | RSS native |
| ---: | ---: | ---: | ---: | ---: |
| 5k | 0.056 s | 0.583 s | 0.151 s | 81 MB |
| 10k | 0.087 s | 1.942 s | 0.235 s | 155 MB |
| 20k | 0.160 s | 7.374 s | 0.409 s | 304 MB |

Outputs matched at every size. The original arena was quadratic: `materialize_native_value` and `tagged_value` scanned every list/tagged descriptor on every miss (88% of samples). Replacing those scans with constant/log-time address lookups (`AddressedBoxes`, `tagged_chunk_starts`) makes native `main` linear, and that fix is retained. It does not make native `main` fast: after ~16 ms of compile time it costs about **16 µs/row against 6 µs/row for the VM**. User time was 0.316 s vs 0.157 s and system time (page faults) only 0.060 s of 0.379 s, so memory growth explains at most ~15% of the gap.

Conclusion: safe reclamation or ownership-aware handle retirement is necessary to run the native loop in bounded memory, but it is **not sufficient** for speed. The slowdown is the per-operation mixed-value bridge (generic operation dispatch, Value decode/encode, and per-call fuel, cancellation and clone-charge callbacks), which matches the earlier no-gain result for the per-op bridge. A reclamation safepoint alone is expected to land near or below VM speed, so it was not built. The next candidate that can plausibly close the gap is native-resident representations and inline lowering for the hot JSON/Text/Option/Result operations, with reclamation added once those values no longer round-trip through `Value`.

## Loop-backedge arena reclamation — 2026-10-31 (opt-in `native-value-bridge` only)

Implemented a native loop safepoint. At each backedge of an allocating loop, native code spills the live handle-typed locals as `(slot, tag, handle)` triples and calls `native_loop_safepoint`; once the arena holds at least `max(2048, 2× the entries left after the last reclamation)` entries, the runtime materializes those locals, empties every arena handle store, re-stores the live values (without charging the allocation budget again, so quotas stay cumulative and VM-identical) and the generated code reloads the rewritten handles. Supported live local types: scalars, Text, Bytes, Json and the error types, `Map`, `NativeResource`, `List[Text]`, and Option/Result over those. A function qualifies only when nothing else calls it, every allocating backedge is an unconditional `Jump` with an empty operand stack, and every live local is supported; otherwise the existing `native_loop_value_arena_retention` guard still keeps it in the VM. The guard is lifted only with the `native-value-bridge` feature, so default builds behave exactly as before (the only default-build change is the O(1) arena lookups).

Result on the frozen 200k-row JSONL workload (container, feature build, output identical, `--services` omitted): `main` now compiles natively with peak RSS **20 MB** (previously 304 MB at only 20k rows without reclamation) and a linear time curve, but it takes **2.67 s against 1.46 s for the VM binary** (20k rows: 0.326 s vs 0.163 s). Reclamation fixed memory and the quadratic scans; it did not make the native loop fast. Sampling the native run shows the remaining cost is the per-operation mixed-value bridge: generic operation dispatch, Value materialization/encoding, per-call clone-charge/fuel/cancellation callbacks, and roughly 140 heap allocations per row that glibc tuning does not change (GLIBC_TUNABLES trim/top-pad/arena settings measured 1.43–1.46 s against 1.44–1.46 s for the unmodified VM run).

Verification: `cargo test -p guion-runtime` passes 196 execution tests in the default build and 197 with `native-value-bridge` (the two guard tests are now default-build only, and a new feature-gated test runs a 12,000-iteration native JSON loop past the reclamation threshold against the VM). `stdlib_data` and `stdlib_services` crash with SIGSEGV in debug builds; the `stdlib_data` crash (`every_shipped_data_module_compiles_as_an_independent_std_import`) reproduces on the unmodified branch, and the `stdlib_services` crash was not checked against it. `cargo fmt --all` is clean.

The direction to close the speed gap is now narrowed to removing bridge work, not memory: lower the hot JSON/Text/Option/Result operations inline in native code on native-resident representations, and cut per-row allocations.

## Pool allocator and native-resident design findings — 2026-10-31

Allocation attribution (VM, `43a48af`, 200k-row JSONL): about 82 `malloc` calls per row, of which about 36 are reading and parsing the line (`jsonl_next`) before any user code runs. Native `main` through the bridge made about 140 per row.

New lever, not previously tried: `crates/cli/src/pool_alloc.rs`, a dependency-free thread-local size-class pool installed as the `guion` binary's global allocator (blocks up to 512 bytes, 16-byte classes, 256 KiB chunks never returned to the OS, shared spill list past 16k blocks so cross-thread frees cannot grow memory without bound). Container, 200k rows, interleaved, output hash identical:

| Build | Time | Peak RSS |
| --- | --- | --- |
| VM, glibc malloc (`43a48af`) | 1.37 s | 12 MB |
| VM, pool allocator | 1.16 s | 12 MB |
| Native `main` (bridge feature), glibc | 2.67 s | 20 MB |
| Native `main` (bridge feature), pool | 1.60 s | 21 MB |

Python is 0.648 s on the same workload, so the VM is now about 1.75x Python (was 2.25x). Native `main` is still slower than the VM, so the bridge stays opt-in.

Post-pool VM profile: dispatch about 29%, `value_limit_error_with_json` about 5%, Value clone/drop about 7%, quota bookkeeping about 4%. The limit check, `map_clone_allocation` and the map clone each walk the whole map on every `map_set`, so the per-row cost grows with distinct dates per service. Closing that needs an in-place `set m = map_set(m, ...)` form with an incrementally maintained clone-charge, which is a bytecode and artifact-format change.

Fixed along the way: VM-to-native entry passed `List[Text]` as a handle-table index that native indexing dereferenced as an address (SIGSEGV in `text_split`, `stdlib_data`, `stdlib_services`). Regression test `native_text_list_parameters_match_the_vm`.
