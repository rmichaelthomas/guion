# Guion JSONL Performance Gap: Research-Led Implementation Plan

**Revision 2 — approved by the user on 2026-10-29.** The decision rules and workflow below govern the tasks in this document. Research supports specific engineering options; it cannot guarantee a numerical result before measurement. Completion remains defined by the four Python comparison gates, never by completing an implementation checklist.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task by task. Steps use checkbox syntax for tracking.

**Goal:** Close the measured Guion-versus-Python performance gap on the frozen 20,000-row and 200,000-row JSONL workflows while preserving exact language behavior, artifact compatibility, and runtime policy.

**Architecture:** First attribute costs on the exact committed build. The leading hypothesis is broad guest execution overhead: the profile shows tens of millions of interpreted instructions and no native-compiled functions, while local opcode fusions and the per-operation mixed-value bridge did not produce a net win. If the fresh profile confirms that, compile a profile-selected whole function or hot region from verified bytecode into an internal Cranelift tier, lowering common operations directly and using the existing VM at precise side exits. If the profile instead identifies a general host operation as the largest cost, optimize that shared operation first. Keep the existing VM as the correctness fallback and do not change serialized bytecode in the first pass.

**Tech Stack:** Rust workspace, existing Cranelift native backend, Guion artifact format 45, Debian 12 ARM64 container. The latest retained Linux cohort used Python 3.11.2; preserve that exact reference for reproduction rather than silently changing versions.

**Spec:** `docs/superpowers/specs/2026-10-28-guion-core-north-star-sprint-design.md`; current measured follow-up: `docs/superpowers/plans/2026-10-29-guion-runtime-gap-follow-up.md`.

## Global Constraints

- The performance objective is a fast, useful, understandable Python-free core; Python interoperability remains optional.
- Keep the verified `.gub` artifact as the source of truth. Preserve format 45 unless measurements establish that a serialized change is necessary and a separate compatibility plan approves it.
- Preserve user-visible semantics, source spans, diagnostics, capability checks, instruction fuel, allocation limits, cancellation, and artifact compatibility.
- Do not introduce benchmark-specific builtins, drop JSON validation, raise global resource limits, or route the core workload through Python.
- Reuse the existing worktree and running container. Do not start, stop, restart, or prune Docker/Colima. Run expensive builds and benchmark cohorts sequentially.
- Use `python3` only. Do not edit project-level `AGENTS.md` files.
- No external evaluator, outside worker, PR, push, merge, or commit is required by this plan.
- Separate correctness evidence from speed evidence. Compare against the same frozen input, Python reference, build settings, and candidate binary hashes.

## Review Focus

- JSON values can contain integers outside i64, mixed number kinds, nested arrays/objects, null, and malformed or duplicate-key inputs; preserve parser and accessor semantics.
- Result and Option values can take error/none paths as well as success/some paths; preserve variants and diagnostics at native side exits.
- An unsupported value or opcode can occur after earlier native work; resume the VM with exactly the original locals, stack, instruction pointer, fuel, and source span.
- Fuel or allocation exhaustion and cancellation can occur at a tier boundary; preserve the same failing instruction and error code.
- A denied file or other capability operation can occur inside a hot region; the native tier must continue through the existing policy-enforcing host boundary.

---

## Evidence and Research Decisions

### Current local evidence

- Latest retained 15-pair comparison: `evaluation/development/core_cost_closure/results/jsonl-inline-i64-vs-preinline-paired-15.json`. At 20k rows, candidate/Python median ratios are 3.141 source and 3.083 artifact. At 200k, they are 3.859 source and 3.871 artifact. Output checks pass, but all four lanes miss Python parity.
- In the same report, the inline-i64 change is a modest incremental win: 20k source/artifact ratios versus its predecessor are 0.977/0.989; 200k source is 0.964 and artifact is 0.982 with a confidence interval crossing parity. Peak Guion RSS is 9.70 MB at both sizes. Memory is not the main observed gap.
- Earlier whole-workload profile and eligibility records show about 61.5M VM instructions, 3.77M frames, and zero native functions. `Load`, `Store`, conditional branches, calls, and Result/Option operations are frequent. The eligibility report identifies mixed-value operations as the path that excludes the JSONL program from the existing numeric native subset. Re-capture this profile against the exact current build before treating the counts as current.
- Retained experiments reject further allocation-reservation, isolated Store→Load and Not→JumpIfFalse fusions, and the per-operation mixed-value native bridge as the next default path. The bridge reduced VM instructions but lost end-to-end time; the fusions were neutral or regressed in at least one required lane.
- The typed HTTP response gate is already closed in `evaluation/development/core_cost_closure/results/http-shared-types-paired-15.json`: typed/legacy ratios are 0.929 source and 0.886 artifact at 64 KiB, and 0.761/0.716 at 1 MiB. Do not spend this sprint re-opening or re-optimizing that path.
- The broader north-star records an adjacent optional Python-bridge gap: about 1.48x at 1,000 repeated calls and about 1.22x versus the standard-library Python reference for one 100,000-row application on macOS. This is deliberately outside the Python-free core sprint, consistent with the approved north-star spec; it is not being presented as closed.
- One separate correctness issue was observed during the prior continuation: `compiler_stage_equality` exhausted its 8 GiB invocation allocation budget after very large finite VM work. Reproduce it on the current commit and diagnose the cause without raising the quota. Keep this separate from performance attribution.

### Historical lessons applied

1. CPython's accepted [PEP 659](https://peps.python.org/pep-0659/) specializes frequently executed operations with quickening and local fallback. It reports that specialization contributes more than dispatch fusion alone. Guion should use observed types and stable control flow to select common paths, not keep adding unmeasured adjacent-op fusions.
2. The empirical [stack-versus-register VM study](https://doi.org/10.1145/1328195.1328197) reports 46% fewer executed VM instructions and an average 1.48x speedup for its register VM over the equivalent switch-dispatched stack VM, with a 26% bytecode-size increase. This supports register-style lowering as a useful technique, but that result comes from a different JVM implementation and is far too small to promise closure of Guion's current 3.8x gap by itself.
3. [Vmgen](https://onlinelibrary.wiley.com/doi/10.1002/spe.434) evaluates threaded dispatch, stack caching, and superinstructions as distinct techniques. Guion has already tested individual fusions without a reliable win, so the next candidate must change the full hot execution path and must be measured end to end.
4. [Deegen](https://arxiv.org/abs/2411.11469) and [Druid](https://doi.org/10.22152/programming-journal.org/2025/10/9) show that interpreter semantics can support a lower-startup baseline JIT, and that generating the tier from the interpreter can reduce semantic drift. Their systems are research precedents, not drop-in components or speed guarantees. Guion should compile its existing verified bytecode and test VM/native equivalence rather than add a separate language semantics.
5. [Are We Fast Yet?](https://doi.org/10.1145/2989225.2989232) argues for comparing a range of common language abstractions and benchmark characteristics. Keep the frozen real JSONL program as the acceptance workload and add an unrelated existing application as a holdout against workload-specific tuning.

## Files and Responsibilities

- `crates/vm-core/src/lib.rs`: opt-in profile counters and VM frame handoff only if the exact-build profile lacks needed attribution.
- `crates/runtime/src/native_backend.rs`: eligibility, direct bytecode lowering, native outcome or side-exit state.
- `crates/runtime/src/lib.rs`: select the internal tier and preserve the existing host-policy boundary.
- `crates/runtime/tests/execution.rs`: VM-versus-tier differential, side-exit, fuel, cancellation, and resource-limit tests.
- `crates/cli/tests/bootstrap.rs`: existing compiler-stage equality test if it reproduces the known allocation-budget failure.
- `evaluation/development/core_cost_closure/`: frozen manifests and raw profile/paired results; do not alter workload code to make the candidate faster.
- `README.md`, `docs/status.md`, and agent-facing references: update only after final measurements.

## Implementation Tasks

### Task 1: Reproduce the current control and make the profile causal

**Files:**
- Modify: `crates/vm-core/src/lib.rs` only if current counters cannot attribute sampled guest time to a function and separate guest execution from host-operation time.
- Test: `crates/runtime/tests/execution.rs`.
- Create: exact-build profile and baseline reports under `evaluation/development/core_cost_closure/results/`.

- [ ] Rebuild the release CLI from the current committed source in the existing container; record commit, toolchain, binary SHA-256, input hashes, Python version, and container identity.
- [ ] Run the frozen JSONL baseline at 20k and 200k for uncached source and artifact against the pinned Python reference, with 15 randomized paired trials per lane and output checks. Record medians, paired 95% intervals, and peak RSS.
- [ ] Add the failing profile test `vm_profile_attributes_sampled_guest_time_by_function_and_separates_host_time`; run it before implementation with `cargo test --locked --offline -p guion-runtime --test execution vm_profile_attributes_sampled_guest_time_by_function_and_separates_host_time`. Expected: fail because per-function sampled-time and exclusive host-time fields are absent.
- [ ] If needed, add low-overhead opt-in per-function sampled guest time, per-function loop-backedge counts, and exclusive host-operation timing. Do not add per-instruction timers to ordinary execution. Confirm profiling stays absent by default.
- [ ] Generate the native eligibility report from the exact rebuilt baseline. Name each excluded hot function and its first reason; do not infer current eligibility from an older binary.
- [ ] Re-run `compiler_stage_equality`. If it fails at the same allocation ceiling, retain the failure evidence and root-cause the excessive work; do not raise the policy limit. Continue independent performance profiling; resolve the test cause before claiming the test suite passes. This test-health issue is not a speed result.
- [ ] Decide the next task from measured exclusive time: use the native execution tier in Task 2 if repeated guest dispatch/frame work is dominant; otherwise target the highest-cost general runtime operation using its shared implementation. Record the decision before editing that path.

### Task 2: Add an internal baseline execution tier for the measured hot path

**Files:**
- Modify: `crates/runtime/src/native_backend.rs`, `crates/runtime/src/lib.rs`, and `crates/vm-core/src/lib.rs` only where tier handoff requires it.
- Test: `crates/runtime/tests/execution.rs`.

- [ ] Add VM-only reference fixtures for a hot loop with typed JSON access, Result/Option control flow, local calls, large integers, and an unsupported operation that forces a side exit.
- [ ] Define and test the side-exit state before adding more operations: function index, exact bytecode instruction index, locals, operand stack, spent fuel, call depth, and source span must reconstruct a VM frame with identical behavior.
- [ ] Compile the profile-selected whole function or loop from the verified `Function` into an internal Cranelift representation. Use register/SSA values for stable locals and common typed control flow. Keep the existing VM as fallback; do not alter format 45 artifacts.
- [ ] Lower frequent local loads/stores, branches, direct calls, Result/Option checks, record-field access, and the profile-selected JSON access operations directly where types prove the fast path. Keep JSON parsing, I/O, and capability-sensitive work in the existing shared Rust/host implementations.
- [ ] Do not route each guest opcode through `guion_bridge_value_operation`. Any fallback must side-exit to the VM at the original instruction; common operations that have a shared Rust helper may call that helper directly without a generic stack-marshalling trampoline.
- [ ] Add `native_tier_matches_vm_for_json_option_result_and_large_int_paths`, `native_tier_side_exit_resumes_exact_vm_state`, `native_tier_preserves_exact_instruction_limit`, `native_tier_preserves_allocation_limit_and_cancellation`, and `native_tier_keeps_capability_checks_on_host_operations`.
- [ ] Run the focused tests with the native tier forced on and forced off. Require identical outputs, error codes, source locations, traces, fuel outcomes, allocation outcomes, and cancellation behavior before timing.

### Task 3: Prove or reject the architecture with paired runs

**Files:**
- Modify: `evaluation/development/core_cost_closure/benchmark_jsonl_binary_pair.py` only if it needs to pin the exact candidate and control hashes or capture the selected profile metadata.
- Create: candidate manifests, paired raw result files, and a concise decision report under `evaluation/development/core_cost_closure/`.

- [ ] Run a three-pair smoke in each required lane. Stop timing and fix any output or policy mismatch before proceeding.
- [ ] Run at least 15 randomized paired trials at both input sizes in source and artifact modes against the same Python oracle and immediate pre-tier Guion control. Keep instrumentation disabled in timed binaries.
- [ ] Retain the candidate only if it improves end-to-end time with a paired 95% interval that excludes parity against the immediate control, does not materially regress any required lane, and has no unexplained peak-RSS increase. If an interval overlaps parity despite a plausible effect, run a 30-pair confirmation once; otherwise remove the candidate and record the negative result.
- [ ] Run the unchanged lockfile-auditor application as the unrelated holdout and confirm exact output plus no material runtime regression.
- [ ] If the tier improves the application but misses Python parity, use the updated profile and the closure calculation below to extend the same tier to the next highest-cost general function or operation. If its attainable savings cannot close the gap, execute the already-scoped alternative comparison below. Record the choice and continue locally without requesting approval at each task boundary. A material language/API change still requires a revised design.

### Task 4: Close the north-star performance gate and report honestly

**Files:**
- Modify: `README.md`, `docs/status.md`, and relevant `docs/agents/` pages.
- Create: final before/after report and raw measurement records under `evaluation/completion/results/real-task-sprint/` and `evaluation/development/core_cost_closure/results/`.

- [ ] Re-run the final output-checked 20k and 200k source/artifact cohorts and report absolute times, Python ratios, paired intervals, peak RSS, build identity, and host limitations.
- [ ] The JSONL gate is closed only when the candidate/Python 95% upper ratio bound is below 1.0 at both sizes in both source and artifact modes. Otherwise leave it explicitly open and state the measured remaining distance and the next evidence-backed action.
- [ ] Run the relevant runtime, compiler conformance, artifact compatibility, and CLI tests, including the bootstrap test after its diagnosis. Do not claim the complete test suite passes if a reproducible failure remains.
- [ ] Update README and agent-facing docs to identify the HTTP gate as already met, show the final JSONL result without rounding away a miss, and separate Python-free core performance from optional Python-bridge performance.
- [ ] Review the diff and preserve all benchmark evidence. Commit only after the user approves the plan and later authorizes a commit.

## Acceptance Gates

- JSONL output matches the frozen oracle and both sizes pass in source and artifact modes.
- Candidate/Python paired 95% upper ratio is below 1.0 in all four required lanes.
- Runtime behavior, fuel, allocation, cancellation, host policy, error traces, and artifact format remain correct.
- The unrelated application holdout has exact output and no material regression.
- Every retained speed change has evidence on the exact binary and same input; rejected candidates are recorded rather than presented as wins.
- The HTTP result remains reported as closed; bridge-performance results remain separate and outside this Python-free core sprint.

## Explicitly Out of Scope

- Reopening the typed HTTP implementation already measured within the existing non-inferiority gate.
- Optimizing Python bridge payload or repeated-call performance; those remain a separate optional interoperability gap and are not required for this core objective.
- A fresh-builder or outside evaluator, external delegation, new Docker/Colima lifecycle actions, broad artifact-format changes, or benchmark-specific language features.

---

## Plan Self-Review

### Revision 2: additional primary research and selected options

| Precedent | Lesson applicable to Guion | Proposed use and limitation |
| --- | --- | --- |
| [V8 Sparkplug](https://v8.dev/blog/sparkplug) | Compiles existing bytecode quickly and maintains interpreter-compatible frames. Startup and frame interoperability are explicit design objectives. | Prefer a whole-function baseline tier with a canonical slot/frame layout. Reuse Cranelift; do not copy V8 machine-code generators or build another backend. Cranelift compilation cost must be measured; Sparkplug performance does not predict it. |
| [WebKit FTL entry](https://webkit.org/blog/3362/introducing-the-webkit-ftl-jit/) and [B3 exits](https://webkit.org/blog/5852/introducing-the-b3-jit-compiler/) | Entering a running loop and leaving speculative code require explicit state maps. Entry and exit are different problems. | Start with function-entry compilation and compatible slots. Add loop-header entry only if a function runs once for long enough that entry-only tiering misses the useful work. Map every live value and guest instruction at exits. |
| [LuaJIT FFI semantics](https://luajit.org/ext_ffi_semantics.html) | Callbacks and unsupported operations in inner loops can defeat JIT benefits. Inlining work in the calling context can matter more than the label native. | Count guest-to-helper calls, VM exits, conversions and boxing per row. Reject a candidate that merely trades dispatch for repeated marshalling. This is a design analogy; Guion does not use LuaJIT FFI. |
| [Wasmtime Pulley](https://bytecodealliance.org/articles/wasmtime-portability) | A register-based internal interpreter representation can reuse an existing compiler rather than create a second optimizer. | Bounded alternative: derive a predecoded/register execution plan from verified bytecode using current analysis. Keep the artifact format unchanged. Do not adopt Pulley as a drop-in Guion VM: Wasm semantics differ. |
| Deegen, Druid, PEP 659 and Vmgen above | Fast common paths, limited compilation cost, reusable semantics and fallback support are complementary techniques. | Reuse existing code and semantics before adding infrastructure. Do not import an entire VM framework to obtain one optimization. |

The original plan overcommitted to a speculative hot region before establishing its entry/exit cost. Revision 2 prefers a simpler compatible-frame, function-level baseline tier. Region compilation and loop entry are conditional extensions with measured justification. Static Guion types should eliminate checks where proven; dynamic feedback is reserved for JSON shapes and values that static types cannot establish.

### Closure calculation and decision rules

The latest 200k source median is approximately 2.507 seconds against 0.652 seconds for Python. Closing that lane requires roughly 74% less elapsed time; 20k needs roughly 68% less. A few-percent optimization cannot be the primary closure strategy.

For measured current time T, targeted fraction f and achievable acceleration s, estimate candidate time as T × ((1 − f) + f/s) + added compilation/transition time. This is a planning bound, not a substitute for paired trials. Even infinite acceleration of half the program cannot close a 3.86x gap. Use exclusive times from the same run; never add overlapping host and VM timers. Function counts and logical allocation charges alone cannot supply f.

1. Reuse the current baseline if its binary, source, harness and input hashes are reproducible. Run a three-pair sanity check first. Repeat the full baseline only for a changed environment, mismatched identity, unexplained drift, or a final comparison needing contemporaneous pairs.
2. Capture an exact-build profile with guest-only time, host time, frame/call costs and representative hot functions. Measure profiling overhead separately. Stratify or randomize sampling so periodic sampling does not repeatedly select the same position in a regular loop. Do not label instrumented elapsed time as normal performance.
3. Select the compatible-frame baseline tier if the measured guest execution fraction can support the required savings. Direct lowering and small static helper inlining are the first candidate; cap inlining/code size and use existing type/control-flow analysis.
4. If host work leaves a floor above the Python time, pair the tier work with the largest measured general host cost. Consider typed JSON access, avoiding repeated tree traversal/materialization, or text/date operations only where the exact profile identifies them. Parser replacement or SIMD adoption is conditional on parser cost, semantic compatibility, dependency/license review and demonstrated end-to-end benefit.
5. If direct native execution loses to its control after correctness passes, run one bounded comparison of an internal predecoded/register interpreter on the same hot functions. Reuse verified bytecode, spans and semantic helpers. Do not revive the rejected two-op fusions or generic mixed-value trampoline. Apply the same measurement and correctness gates.
6. After each retained change, recompute the remaining distance and target the largest remaining causal cost. Continue through these approved options without ending the session at every task boundary. If all scoped options are falsified, report the gate as open with the limiting evidence; never fabricate certainty or weaken the target.

Proposed practical regression rules for approval: no required lane or holdout may have a paired 95% upper candidate/control ratio above 1.02; an interval crossing that boundary is unresolved. Record peak RSS, native code bytes and compilation time. An RSS increase above 10% of the control requires demonstrated necessity and an explicit tradeoff in the report before default enablement. These are proposed guardrails, not historical user requirements. Python closure still requires an upper ratio below 1.0 in all four lanes.

### Missing correctness and runtime engineering practices

- Treat every exit as a continuation, not a restart. Effects already completed must never repeat. Test an exit after file reading, a map update and observable output, including a failing next operation. Record whether an exit occurs before or after the guest instruction and settle its fuel exactly once.
- Preserve Value ownership and aliasing: cloned JSON/maps retain immutable value behavior, unique updates remain unique, and side exits cannot leak, double-drop, or outlive their native code/arena. Add aliasing and repeated invocation tests; use existing sanitizer or Miri tooling where supported rather than making new tooling a prerequisite.
- Use differential generated programs with fixed seeds and bounded loops to exercise VM versus candidate outputs, errors, traces and limits. Retain minimized counterexamples. Run Cranelift verification and the existing artifact verifier; include malformed-artifact rejection. These checks supplement the hand-written cases in Task 2.
- Specify safepoints at original guest boundaries, exact error precedence, and shared fuel/allocation behavior across workers. Test cancellation during host calls and at native/VM transitions. Do not hoist capability-sensitive operations across observable effects.
- Bound compilation work and native-code memory per invocation. Keep code ownership alive through worker completion; fall back cleanly if executable memory or the native target is unavailable. Use the existing backend memory protections and target support, not ad hoc executable-memory allocation.
- Measure compilation break-even: compilation cost divided by saved time per invocation or iteration. Cold short programs stay on the cheaper path; a once-entered long loop must still benefit. Keep cached source, uncached source, artifact and steady-state measurements separately labeled.
- Build a source-hash-to-binary-hash-to-result chain. A report status named passed must identify whether it means correctness, candidate improvement, or Python closure. Do not use an aggregate passed field to hide failed speed gates.
- Preserve Linux Python 3.11.2 as the historical control. Add a separately labeled comparison to the already available pinned Python 3.14.7 environment if available; do not pool platforms or replace the historical control silently. A win against 3.11.2 alone supports only that version-specific claim.

### Self-contained execution and permission workflow

No document can guarantee that host permissions or services never change. The workflow resolves foreseeable access first, records the scope actually granted, and avoids treating access failure as a code defect.

1. At execution start, resume the session contract and inspect git status. Record the worktree path, current commit, plan revision, existing container ID, container working directory, mount mapping, Rust/Python versions, offline dependency availability and free disk. Discover actual values; never assume a container path or hard-code a stale binary location.
2. Request only the needed repository/build/result writes through the environment permission mechanism. The worktree is outside the normal writable roots; git metadata may also live outside the worktree. Validate that metadata path before any authorized commit. Do not assume this turn-only planning grant persists into execution.
3. Check the existing Docker socket using the authorized read-only context, then use the existing container through docker exec. If access is denied, inspect the exact denial and use the supported permission/escalation mechanism once with the narrow action. Do not restart a healthy daemon or repeatedly retry unchanged denied commands. If an actual service problem is demonstrated, diagnose and repair within user authorization, preserving shared resources.
4. Validate bounded scratch/result writes, offline build access and local loopback fixture access once before expensive work. External browsing and container network access are different permissions. Do not require outside networking for timed workloads. Missing dependencies are repaired once in the same environment, then execution returns to offline mode.
5. Maintain one run manifest and one progress ledger under evaluation/development/core_cost_closure. Record each task as pending/running/passed/rejected/blocked, with source/binary/input hashes, command, exit code, output path and next action. Write reports atomically. Never mark a task passed solely because a command was launched.
6. Run one heavy process at a time, retain its process/session identifier and poll that process instead of launching duplicates. Batch independent reads. Send short progress updates at meaningful findings and within about a minute during active work. Task completion updates the ledger and proceeds to the next dependency; it does not require another user turn.
7. Run the smallest relevant tests first, then affected integration tests, and the final regression matrix once. Re-run only when changes or failures invalidate earlier evidence. Three-pair smoke filters candidates; confirmation runs use a fixed sample count chosen before the run. Retain failed trials and reasons rather than discarding slow samples.
8. If a tool permission is denied, continue independent analysis, tests or documentation that remain possible. Report the precise denied operation and reason. Never bypass controls or claim the access succeeded. Do not turn optional hardware counters or an external evaluator into prerequisites.
9. Keep scratch paths explicit and bounded. Clean only files created and owned by this sprint after durable evidence is saved. No shared cache/container pruning. Finish with the result matrix, unresolved items, disk accounting and git status.

### Approval scope and completion

Approval covers executing the above ranked options, diagnosing the known bootstrap failure, meaningful differential tests, and updating results/docs locally. It does not authorize changing language semantics, weakening resource limits, deploying, pushing or merging. The bootstrap failure is a correctness issue to resolve in this work, not a reason to abandon independent performance progress. The optional Python bridge remains a separate workstream under the existing core sprint scope.

Completion requires passing the four frozen JSONL/Python gates, the stated regression guardrails and relevant correctness checks, plus a report tying each claimed improvement to its mechanism and exact measured binary. Neither a new tier nor a reduced instruction count is completion by itself. The user approved execution on 2026-10-29; implementation is in progress.

- Spec coverage: the residual measurable core speed gate, correctness/resource guardrails, reproducible paired comparisons, documentation, and honest incomplete status map to Tasks 1–4. Typed HTTP and capability audit outcomes already met in the current sprint are preserved rather than repeated.
- Step scan: every task has a named deliverable, a regression test or exact benchmark, and a clear pass/stop interpretation.
- Type and boundary consistency: the plan uses the existing verified artifact, existing Cranelift backend, `VmFrame`-style state handoff, and existing policy-enforcing host operations; it adds no new public Guion API.
- Review focus: JSON type diversity, Result/Option errors, deoptimization state, limits/cancellation, and denied host operations each have explicit differential coverage in Task 2.
- Proportion: implementation is gated by exact-build attribution; the plan avoids assuming that past instruction counts alone prove the current bottleneck.
