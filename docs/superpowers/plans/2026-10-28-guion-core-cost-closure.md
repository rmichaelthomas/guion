# Guion Core Data and HTTP Performance Closure Sprint

> For implementation: use superpowers:executing-plans in the existing worktree. Checkbox steps track actual evidence. This plan authorizes no external sharing and introduces no evaluator prerequisite.

**Goal:** Close the frozen 20k/200k JSONL gap against pinned Python and remove the typed HTTP workflow penalty while preserving useful behavior, compatibility, capability checks and resource limits.

**Architecture:** Attribute the general mixed-value VM path and HTTP phases, then optimize the owning runtime/compiler boundaries. Retain the public typed HTTP API, the original application and its oracle. Extend native execution only if attribution identifies interpreted dispatch as the remaining dominant cost; preserve interpreter fallback and error/budget parity.

**Tech stack:** Existing Rust workspace, Guion stdlib, Python3 benchmark tools; Rust/Cargo 1.97.1 and CPython 3.14.7 for comparable Debian ARM64 evidence.

**Spec:** Original 2026-10-28 north-star design plus the user's seven stated aims. Technical basis: evaluation/completion/results/real-task-sprint/2026-10-28-core-cost-analysis.md.

## Global constraints

- Reuse codex/guion-core-sprint-spec and its current worktree; freeze 41d3b48 as the new before-build. Preserve existing raw evidence. Do not edit AGENTS.md.
- Use python3. Inspect the existing container, active jobs, free disk and caches before expensive work. Keep heavy builds/runs sequential; do not start/restart Colima or prune shared resources.
- Keep the core Python-free. No benchmark-specific summary builtin, hidden Python dispatch, removed validation, or weakened quotas.
- No outside evaluator, transfer, PR, push or merge is authorized or required for this sprint. Do not use an unrelated usability exercise to halt building.
- Do not commit or push this work unless the user later asks; keep the worktree reviewable in place.
- Retain format-44/45 readability. Introduce a format revision only if adding an opcode requires it; append tags and update bootstrap/catalog/tests together.
- Preserve both the unchanged application lane and separately labeled idiomatic-source improvements. Never overwrite the before-source with a faster algorithm and call the result a runtime improvement.
- Retain unsuccessful candidates with why they failed. A small improvement is progress, not closure of the Python gap.

## Review focus

1. Duplicate/reserved JSON keys, huge exact numbers and nesting/collection/text limits retain their error contracts.
2. Shared aliases and nested maps/JSON retain immutable value semantics after update optimizations.
3. Invalid UTF-8, malformed JSON, non-2xx status and allocation/text ceilings preserve typed HTTP behavior.
4. Cancellation, instruction exhaustion and parallel cumulative allocation quotas remain enforced in interpreter and native paths.
5. Cold source, cached source, artifact and in-process execution remain distinct; all timed trials check outputs and fixture request counts.

## Completion criteria

These are proposed engineering acceptance criteria for this sprint, not claims of achieved performance:

- Both 20k and 200k frozen JSONL programs have correct output and a paired Guion/Python ratio whose 95% upper bound is below 1.0 for artifact and uncached source on the pinned target. Both sizes become required gates. If unmet, explicitly retain the gap as open rather than marking the performance goal complete.
- Typed HTTP on the final executable is measurably faster than the before-build typed path (paired 95% upper bound below 1.0), and within a proposed 2% non-inferiority margin of the final executable's legacy path (upper bound at most 1.02), for source and artifact at 64KiB. Publish smaller/larger payload results; a favorable average cannot hide a large-payload regression.
- A retained candidate has an interval below parity against the immediately relevant Guion baseline and no unexplained peak-RSS increase or resource-policy weakening. Report every tradeoff explicitly.
- Full before/after table includes timing, intervals and separately measured peak RSS for every frozen workload, plus an unrelated data-processing holdout. Passing correctness and failing speed are separate statuses.

### Task 1: Establish a valid comparison and attribution harness

**Files:** Modify evaluation/development/core_north_star_sprint/benchmark_core_workloads.py and evaluation/completion/performance/runner.py; create evaluation/development/core_cost_closure/measure_costs.py and fixtures/; create a manifest and results directory for this sprint.

**Interface:** Emit JSON containing source revision and SHA-256 for each executable, toolchain/build flags, source/input hashes, trial ordering, per-run output validation, wall samples, phase timings, direct paired ratios and peak-RSS method. Report correctness_status and performance_status separately; retain overall failed status when a required speed gate fails.

- [ ] Rebuild 41d3b48 and candidate under identical locked release settings; reject mismatched manifests before timing. Verify final reports' binary hashes.
- [ ] Add same-binary legacy/typed HTTP arms with the same explicit timeout, request body and grants. Add base-typed/final-typed arms separately. Retain original legacy/base comparison as historical evidence.
- [ ] Add source, artifact and repeated in-process synthetic HttpResponse arms at 0, 1KiB, 64KiB and 1MiB. Separate compile/load, network, decode, parse and wrapper work. Check all outputs and exact server request count.
- [ ] Add JSONL ablations: read-only; parse-only; parse + field extraction; + timestamp validation; + formatting/date extraction; + grouping/update; full unchanged application. Keep the same input/order and output oracle per ablation. Do not subtract unrelated cohorts as exact CPU attribution.
- [ ] Add harness tests that reject mismatched binaries, detect output corruption, require both JSONL gates, and preserve explicit failed performance status even when correctness passes.
- [ ] Run a short correctness/smoke set before full baselines. Then collect randomized paired baselines (at least the previous 15/16 trials) with separate memory samples. Save harness and immutable evidence by explicit path.

### Task 2: Locate the general runtime execution cost

**Files:** crates/vm-core/src/lib.rs (execute_vm_frame, Budget, RuntimeProfile); crates/runtime/src/lib.rs (host_op and JSON parse/metrics boundaries); crates/runtime/src/native_backend.rs (eligibility diagnostics); runtime profile tests; measure_costs.py.

**Interface:** Opt-in profiling report with per-op counts, host-operation counts/durations, parse versus metrics durations, wrapper clone and map/JSON copy-on-write counts, budget events, and per-function native rejection reasons. Default execution must not read clocks per instruction.

- [ ] Add meaningful profile tests: known tiny programs produce exact operation counts; disabled profiling leaves output and limits unchanged; native rejection reports the actual first unsupported operation/callee.
- [ ] Instrument host boundaries and sampled VM execution separately. Measure profiler overhead using uninstrumented builds; use instrumentation for attribution only, not final speed claims.
- [ ] Run the frozen JSONL ablations at both sizes. Identify the largest recoverable cost and reconcile it with full-program execution. Record what is measured versus inferred, including allocation-budget charges versus actual allocations.
- [ ] Produce a ranked cost table and select Task 3's branch from that evidence. This is an internal engineering decision, not a user-approval blocker. Record diagnostics independently.

### Task 3: Remove the dominant general JSONL runtime cost

**Files selected by Task 2:** crates/vm-core/src/lib.rs; crates/runtime/src/lib.rs; crates/runtime/src/native_backend.rs; existing execution/parser/resource tests; add focused regression tests beside the owning crate.

**Interfaces preserved:** Value semantics, host_op error behavior, Budget consume/allocate behavior, finish_shared_json validation, public json_parse/jsonl_next signatures.

- [ ] If dispatch/wrapper/accounting dominates: first remove avoidable instruction cloning and transient wrapper allocation where ownership proves safe. Preserve logical accounting totals. Benchmark each change independently.
- [ ] If atomic accounting dominates: design frame-local reservations using existing reserve/refund mechanisms, with exact exhaustion/cancellation boundaries and shared parallel quota tests before enabling. Reject a faster implementation that admits work past a configured ceiling or changes guest-visible behavior without an explicit compatibility decision.
- [ ] If JSON validation/metrics dominates: accumulate metrics during parse or cache validated immutable-subtree metrics, with recomputation at mutations. Prove duplicate/reserved keys, exact numbers, malformed errors, depth/item/text limits and aliasing behavior against current implementation. Do not revive the rejected reserved-key scan patch without new attribution.
- [ ] If map/JSON update copies dominate: optimize the existing local assignment ownership path, preserving aliases. Use a two-alias regression where updating one value must not modify the other; measure actual copy counts and allocation budgets.
- [ ] After every retained change, repeat both frozen sizes against the before-build. If interpretation remains dominant, first collect dynamic opcode-transition/loop-hotness evidence and test a small profile-selected set of internal superinstructions with unchanged serialized artifacts and exact per-op fuel, cancellation, error-span, stack, allocation, and VM-fallback behavior. Retain only paired gains. If dispatch still dominates and local changes cannot close the measured budget, implement a generic mixed-Value/host-call native bridge in native_backend.rs with explicit ownership, error, cancellation and budget contracts. Start with one emitted helper-call fixture, then the real JSONL loop; do not require every called helper to be native. This is a distinct architecture checkpoint with VM/native differential tests, not an assumed small patch.
**Mixed-value bridge design selected from the measured gate:** keep the current typed native path unchanged. Add a separate per-invocation Value-handle arena for functions whose parameters or intermediates are Text/Json/Option/Result/Map/Record values; handles never outlive the invocation. Cranelift lowers supported control flow, locals, stack movement, and numeric operations directly, and calls a checked runtime trampoline for supported host operations. The trampoline receives the owning Instance, verified function/op location, frame values, and shared Budget; it returns a value or a stored RuntimeError, and no unwind crosses the native ABI. Compile a function only when every operation and callee has a supported lowering; otherwise select the VM before entry. Start with a JsonTryText helper fixture, then widen only through the measured JSONL call path. Keep bytecode/artifact format unchanged.

- [ ] Run parser/execution/resource tests relevant to each candidate, then the unchanged full application and unrelated holdout. Retain only measured gains. Record remaining time required to meet Python at each checkpoint; continue the selected architecture path rather than declaring closure after a marginal gain.

### Task 4: Remove the measured HTTP cost at its owning phase

**Files:** language/stdlib/http.gu; crates/runtime/src/lib.rs (http_response_text_value, JsonParse and parser entry points); compiler lowering if helper dispatch is selected; tests/http.rs; catalog/model/bootstrap sources only if a new operation is justified.

**Interface preserved:** http_response_text(HttpResponse) -> Result[Text,HttpError]; std:http.http_response_json(HttpResponse) -> Result[Json,StdError].

- [ ] Pin success/error equivalence for empty/multibyte text, retained response aliases, invalid UTF-8, malformed/duplicate-key JSON, non-2xx response, max_text_bytes boundary and allocation-budget boundary.
- [ ] Select from controlled Task 1 measurements: improve reachable-import compilation if source-only cost dominates; inline/lower the small helper if wrapper execution dominates; otherwise parse checked body bytes directly for the JSON-only helper if conversion dominates.
- [ ] For direct bytes parsing, retain HTTP encoding/limit errors before JSON errors and the established JSON code/message/line/column behavior. Avoid the intermediate Text allocation while preserving documented cumulative allocation policy; record whether accounting remains deliberately conservative. Do not promise Vec-to-Arc<str> zero-copy through try_unwrap.
- [ ] If adding an internal operation, add red compiler/model tests, append its opcode, implement both source/artifact paths, update catalog/self-host sources and artifact compatibility, then regenerate/bootstrap. Preserve existing public APIs.
- [ ] Repeat all payload/source/artifact/in-process arms against the same executable and before-build; retain correct requests/output and measured improvement. Check unrelated HTTP examples and network policy tests.

### Task 5: Verify useful application behavior and transferability

**Files:** evaluation/completion/applications/jsonl-summary/; new closure fixtures; existing acceptance suite; parser/runtime/native tests.

- [ ] Keep the original application frozen for runtime attribution. Create a separately labeled idiomatic application variant only when Task 2 attributes significant source-level overhead, such as character-list date extraction or repeatedly materialized JSON aggregate state.
- [ ] Evaluate typed aggregate records and existing text slicing facilities before adding public APIs. Preserve timestamp validation/normalization and stable report ordering. Never replace validation with fixed offsets for arbitrary input.
- [ ] Add adversarial cases: CRLF/blank Unicode lines, malformed rows, wrong field types, duplicate keys, fractional timestamps/offsets, group cardinality, strict mode, allow-list filtering and large invalid-row output. Document Python-reference semantic differences; compare only equivalent cases.
- [ ] Add an unrelated JSON/map data-processing holdout using the same general runtime paths; report it beside JSONL. Run original application acceptance cases for both source and artifact.
- [ ] Run VM/native differential cases, cancellation and resource exhaustion, then the full workspace release suite and compatibility/bootstrap checks required by changed surfaces. Do not repeat broad checks without new code changes or unresolved failures.

### Task 6: Close the evidence and report the remaining distance honestly

**Files:** new immutable final JSON/Markdown reports; README.md; docs/status.md; docs/agents/ references and migration guidance only if behavior/API changes.

- [ ] Repeat every original baseline workload with direct before/final paired arms and pinned Python arms, retaining source/artifact distinction and peak memory. Include JSONL 20k/200k, HTTP payload matrix, tiny source, collections/control flow, text/JSON, useful application and CPU diagnostics.
- [ ] Tie each retained speed change to its source revision and isolated measurements. Record medians, paired intervals, memory, compiler/native coverage, request counts, limits, failures and unmeasured targets. Run a fresh confirmation cohort for any marginal proposed winner.
- [ ] Apply completion criteria explicitly. If either JSONL size still loses or typed HTTP still exceeds its agreed margin, keep that outcome open and identify the measured residual cost and implementation work left. Do not replace success with a weaker aggregate gate.
- [ ] Update README/status so the latest target-specific evidence and incomplete outcomes are clear. Preserve historical macOS reports as separate cohorts.
- [ ] Review changed scope by file, check whitespace/secrets/junk, record disk before/after and remove only this sprint's disposable scratch. Preserve raw evidence, fixtures and builds needed to reproduce it. Do not commit unless the user asks.

## Execution order and scope

Tasks 1–2 produce the causal evidence that selects the Task 3/4 implementation branch. Tasks 3–4 are the performance work, Task 5 prevents benchmark specialization and semantic drift, and Task 6 proves closure. Work stays local and sequential. Implementation is authorized and underway; incomplete speed gates remain open until measured closed.
