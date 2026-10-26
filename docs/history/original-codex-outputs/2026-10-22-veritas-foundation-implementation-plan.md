# Veritas Foundation Implementation Plan — superseded by v0.2

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a clean, general-purpose Veritas compiler and integrated project workflow, then prove it on both a fresh assistant-built data application and a persistent Horizon browser-worker workload.

**Architecture:** Start a new Rust workspace rather than extending the demo-specific prototype. Define language behavior and conformance cases first, then compile source into a versioned bytecode module executed by a Veritas runtime bundled with the `veritas` CLI. Keep Python package calls behind an explicit JSON value boundary; compile the same runtime to WebAssembly for the browser worker, where a narrow host adapter supplies events, output, snapshots, and replay.

**Tech Stack:** Rust workspace; versioned Veritas bytecode; Serde JSON at host boundaries; CPython invoked as `python3` behind the CLI; pinned `uv` for Python dependency resolution; Rust WebAssembly target for browser-worker execution; JavaScript worker host; Python 3 and JavaScript reference workloads for Horizon parity.

**Spec:** `outputs/veritas_north_star_build_design_v0_1.md`

## Global Constraints

- Keep the uploaded originals and all existing Planes, Liminate, Decan, and NowLookHere repositories unchanged.
- Create the implementation only in `work/veritas-foundation/`; keep the tested `work/veritas-prototype/` as historical evidence.
- Do not require application authors to install Rust or type an unqualified `python` command.
- Use Rust for the Veritas implementation, not as a language users need to write.
- Use a backend-neutral typed IR and a versioned bytecode format; do not claim native-speed performance for the first runtime.
- Keep Python interoperability explicit, typed, JSON-compatible, and limited to declared modules and functions.
- Treat Python interop as unavailable in WebAssembly unless the compiler reports an explicit supported boundary.
- Report capability analysis as descriptive until a runtime or operating-system boundary enforces it.
- Test complete workflows and actual follow-up changes; never count a fixture-specific result as proof of general recommendation.
- Use `python3` directly for Python checks and scripts.

## Review Focus

- Malformed or unsupported source must return stable source-located diagnostics and a nonzero exit, never silently discard tokens.
- Integer overflow, invalid UTF-8, missing JSON fields, and Python values outside the declared conversion set must fail with stable runtime errors.
- A failing test or build command must propagate failure through the CLI and package launcher.
- Repeated Horizon runs with identical initial state and events must produce identical semantic outputs and replay hashes.
- Long-running workers must report memory and latency tails, including pauses over 50 ms; median speed alone cannot hide them.

---

### Task 1: Create the clean workspace and language conformance source of truth

**Files:**
- Create: `work/veritas-foundation/Cargo.toml`
- Create: `work/veritas-foundation/crates/veritas-core/Cargo.toml`
- Create: `work/veritas-foundation/crates/veritas-core/src/lib.rs`
- Create: `work/veritas-foundation/language/spec-v0.1.md`
- Create: `work/veritas-foundation/language/conformance/accepted.jsonl`
- Create: `work/veritas-foundation/language/conformance/rejected.jsonl`
- Create: `work/veritas-foundation/tests/conformance.rs`

**Interfaces:**
- Produces: an executable normative grammar and semantic table. Every accepted case names source and expected canonical AST; every rejected case names source, diagnostic code, and span.
- Produces: crate `veritas_core` as the owner of `SourceFile`, `SourceSpan`, `Diagnostic`, and `DiagnosticCode`.

- [ ] **Step 1: Lock `language/spec-v0.1.md`** with these initial rules: `.vr` UTF-8 source; braces and semicolons are significant; `let` is immutable and `var` mutable; functions declare parameter and result types; local types may be inferred; `Int` is checked signed 64-bit; `Float64` uses IEEE-754; implicit numeric conversion is rejected; strings are UTF-8; errors are `Result[T, E]`; modules use explicit relative imports; Python imports are explicitly declared.
- [ ] **Step 2: Write conformance cases before implementation** for declarations, functions, local bindings, primitive/list/map/record types, calls, conditions, loops, both import forms, result propagation, and malformed input.
- [ ] **Step 3: Run `cargo test -p veritas-core --test conformance`** and confirm the cases fail because the spec parser/loader is not implemented.
- [ ] **Step 4: Implement corpus loading and the canonical AST snapshot check** in `tests/conformance.rs`; compare accepted snapshots byte-for-byte and rejected code/span exactly.
- [ ] **Step 5: Run `cargo test -p veritas-core --test conformance`** and confirm all corpus cases pass.

### Task 2: Implement source locations, lexer, parser, and stable diagnostics

**Files:**
- Create: `work/veritas-foundation/crates/veritas-core/src/source.rs`
- Create: `work/veritas-foundation/crates/veritas-core/src/diagnostic.rs`
- Create: `work/veritas-foundation/crates/veritas-core/src/lexer.rs`
- Create: `work/veritas-foundation/crates/veritas-core/src/parser.rs`
- Create: `work/veritas-foundation/crates/veritas-core/src/ast.rs`
- Create: `work/veritas-foundation/crates/veritas-core/tests/lexer_parser.rs`

**Interfaces:**
- Produces: `pub fn parse(source: &SourceFile) -> Result<Program, Vec<Diagnostic>>`.
- Produces: diagnostics with stable `code`, `message`, `help`, `path`, byte span, and one-based line/column; `Diagnostic::to_json() -> serde_json::Value`.

- [ ] **Step 1: Write failing tests** for token boundaries, comments, nested expressions, multiline blocks, invalid characters, unclosed strings, missing delimiters, and exact source spans.
- [ ] **Step 2: Run `cargo test -p veritas-core --test lexer_parser`** and confirm failures identify absent lexer/parser behavior.
- [ ] **Step 3: Implement lexer and recursive-descent parser** from the locked grammar; any unrecognized input returns a diagnostic instead of being filtered out.
- [ ] **Step 4: Run `cargo test -p veritas-core --test lexer_parser` and `cargo test -p veritas-core --test conformance`**; confirm exact stable diagnostics and canonical AST agreement.

### Task 3: Resolve modules, typecheck, and produce typed IR plus capability metadata

**Files:**
- Create: `work/veritas-foundation/crates/veritas-core/src/resolve.rs`
- Create: `work/veritas-foundation/crates/veritas-core/src/types.rs`
- Create: `work/veritas-foundation/crates/veritas-core/src/typecheck.rs`
- Create: `work/veritas-foundation/crates/veritas-core/src/ir.rs`
- Create: `work/veritas-foundation/crates/veritas-core/src/capabilities.rs`
- Create: `work/veritas-foundation/crates/veritas-core/tests/semantic.rs`

**Interfaces:**
- Consumes: `Program` from Task 2 and a project module graph.
- Produces: `pub fn compile(source: &SourceFile, modules: &ModuleGraph) -> Result<TypedModule, Vec<Diagnostic>>`.
- Produces: `TypedModule { functions: Vec<TypedFunction>, capabilities: CapabilitySet }`; function signatures retain source spans and resolved types.
- Initial capabilities are `FileRead`, `FileWrite`, `ConsoleWrite`, `PythonCall`, and `ClockRead`; reports are descriptive.

- [ ] **Step 1: Write failing semantic tests** for unknown names, duplicate modules, immutable assignment, mismatched function arguments/returns, invalid `Result` propagation, checked integer operations, imports/cycles, and capability propagation through calls.
- [ ] **Step 2: Run `cargo test -p veritas-core --test semantic`** and confirm errors occur before runtime.
- [ ] **Step 3: Implement module resolution, type inference at local bindings, boundary checking, and typed IR lowering**; keep effects attached to functions and call edges.
- [ ] **Step 4: Run semantic, parser, and conformance tests**; require error code and span stability for all rejected programs.

### Task 4: Define bytecode encoding and execute general language behavior

**Files:**
- Create: `work/veritas-foundation/crates/veritas-bytecode/Cargo.toml`
- Create: `work/veritas-foundation/crates/veritas-bytecode/src/lib.rs`
- Create: `work/veritas-foundation/crates/veritas-bytecode/src/encode.rs`
- Create: `work/veritas-foundation/crates/veritas-bytecode/src/verify.rs`
- Create: `work/veritas-foundation/crates/veritas-runtime/Cargo.toml`
- Create: `work/veritas-foundation/crates/veritas-runtime/src/lib.rs`
- Create: `work/veritas-foundation/crates/veritas-runtime/src/interpreter.rs`
- Create: `work/veritas-foundation/crates/veritas-runtime/src/value.rs`
- Create: `work/veritas-foundation/crates/veritas-runtime/tests/bytecode_runtime.rs`

**Interfaces:**
- Consumes: `TypedModule` from Task 3.
- Produces: `BytecodeModule { format_version: u16, functions: Vec<FunctionCode>, constants: Vec<Value> }`.
- Produces: `pub fn lower(module: &TypedModule) -> Result<BytecodeModule, Vec<Diagnostic>>`, `pub fn encode(module: &BytecodeModule) -> Result<Vec<u8>, BytecodeError>`, `pub fn decode(bytes: &[u8]) -> Result<BytecodeModule, BytecodeError>`.
- Produces: `pub fn execute(module: &BytecodeModule, host: &mut dyn Host, args: &[Value]) -> Result<Value, RuntimeError>`.
- Produces: `Host` methods for declared file, console, clock, and Python operations; no ambient access is exposed to bytecode.

- [ ] **Step 1: Write failing tests** for versioned round-trip encoding, invalid opcodes, stack underflow, invalid function indexes, checked overflow, recursion, loops, records/lists/maps, `Result`, and host call permissions.
- [ ] **Step 2: Run `cargo test -p veritas-bytecode -p veritas-runtime`** and confirm codec/verifier/runtime tests fail on missing interfaces.
- [ ] **Step 3: Implement typed-IR lowering, verifier, encoder/decoder, and bytecode interpreter** with deterministic serialization and checked resource bounds.
- [ ] **Step 4: Run the bytecode and runtime suites plus all `veritas-core` suites**; confirm output parity between direct interpreter test mode and encoded bytecode execution.

### Task 5: Add the `veritas` project CLI, manifest, lockfile, and test runner

**Files:**
- Create: `work/veritas-foundation/crates/veritas-cli/Cargo.toml`
- Create: `work/veritas-foundation/crates/veritas-cli/src/main.rs`
- Create: `work/veritas-foundation/crates/veritas-cli/src/commands/new.rs`
- Create: `work/veritas-foundation/crates/veritas-cli/src/commands/check.rs`
- Create: `work/veritas-foundation/crates/veritas-cli/src/commands/test.rs`
- Create: `work/veritas-foundation/crates/veritas-cli/src/project.rs`
- Create: `work/veritas-foundation/crates/veritas-cli/tests/project_workflow.rs`
- Create: `work/veritas-foundation/crates/veritas-cli/tests/fixtures/starter-template/`

**Interfaces:**
- Produces: `veritas new <name>`, `veritas check`, `veritas test`, `veritas run`, `veritas inspect --json`.
- Project manifest is `veritas.toml`; project lock is `veritas.lock`; source root is `src/`; tests are `tests/`.
- `veritas test` discovers `test fn` declarations, runs them in isolated per-test state, and returns nonzero if any fail.
- `veritas inspect --json` emits compiler version, module graph, dependencies, capabilities, target, and diagnostics.

- [ ] **Step 1: Write a failing clean-project workflow test** that runs `veritas new`, edits the starter module, checks it, runs one passing and one failing test, and asserts exact exit statuses and JSON inspection fields.
- [ ] **Step 2: Run `cargo test -p veritas-cli --test project_workflow`** and confirm required commands are missing.
- [ ] **Step 3: Implement manifest/lock parsing and the five commands**; `new` creates source and tests without embedding a finished application example.
- [ ] **Step 4: Run the workflow test in a temporary directory with an empty environment** and confirm a missing compiler, dependency, or test failure is reported clearly and propagated.

### Task 6: Build and package a runnable, reproducible project

**Files:**
- Create: `work/veritas-foundation/crates/veritas-cli/src/commands/build.rs`
- Create: `work/veritas-foundation/crates/veritas-cli/src/commands/package.rs`
- Create: `work/veritas-foundation/crates/veritas-cli/src/artifact.rs`
- Create: `work/veritas-foundation/crates/veritas-cli/tests/package_workflow.rs`
- Create: `work/veritas-foundation/crates/veritas-cli/tests/fixtures/package-project/`
- Modify: `work/veritas-foundation/crates/veritas-cli/src/main.rs`

**Interfaces:**
- Produces: `veritas build` writes `build/program.vrb` and `build/manifest.json` containing bytecode format, compiler version, target, lockfile digest, and capabilities.
- Produces: `veritas package --output <path>` writes a target-specific archive containing the launcher, Veritas runtime executable, bytecode, source manifest, lockfile, and resources.
- Package launcher runs the bundled Veritas runtime and preserves the program's exit status and stdout/stderr.

- [ ] **Step 1: Write failing package tests** for clean-directory execution, deterministic manifest fields, missing resource errors, archive traversal rejection, corrupted bytecode, and command failure propagation.
- [ ] **Step 2: Run `cargo test -p veritas-cli --test package_workflow`** and verify no package can currently be produced or run.
- [ ] **Step 3: Implement build artifact writing, archive validation, package assembly, and launcher**; support the current host target only and name that limitation in output.
- [ ] **Step 4: Run the packaged program from a temporary directory with Rust removed from `PATH`** and confirm identical output and exit status to `veritas run`.

### Task 7: Add typed CPython calls through the declared dependency boundary

**Files:**
- Create: `work/veritas-foundation/crates/veritas-runtime/src/python_bridge.rs`
- Create: `work/veritas-foundation/crates/veritas-cli/src/python_env.rs`
- Create: `work/veritas-foundation/crates/veritas-cli/tests/python_interop.rs`
- Create: `work/veritas-foundation/crates/veritas-cli/tests/fixtures/python-project/`
- Modify: `work/veritas-foundation/crates/veritas-cli/src/project.rs`
- Modify: `work/veritas-foundation/crates/veritas-runtime/src/lib.rs`
- Modify: `work/veritas-foundation/language/spec-v0.1.md`

**Interfaces:**
- Source form: `from python "module" import function as local_name`; calls accept and return only JSON-compatible `Null`, `Bool`, `Int`, `Float64`, `Text`, `List`, and `Map` values.
- Manifest form: `[python] dependencies = ["feedparser>=6,<7"]`; `veritas sync` delegates resolution to a pinned `uv` selected by the Veritas installation and commits exact resolved identities to `veritas.lock`.
- Bridge protocol: newline-delimited JSON over a managed `python3` subprocess with request id, module, function, args, result/error, bounded message size, and stderr kept separate from protocol stdout.
- Produces: `veritas sync`; check/run/package reject undeclared imports with stable diagnostics.

- [ ] **Step 1: Write failing integration tests** for a declared `feedparser`-style pure Python function, Unicode/nested JSON values, Python exception, absent module, undeclared import, invalid output type, subprocess timeout, and no `python` binary on `PATH` while `python3` is available.
- [ ] **Step 2: Run `cargo test -p veritas-cli --test python_interop`** and confirm the compiler/runtime rejects imports because the bridge is absent.
- [ ] **Step 3: Implement the supervised JSON-lines CPython bridge and pinned dependency sync**; never pass Python source assembled from untrusted Veritas values through a shell.
- [ ] **Step 4: Run the Python interop suite and package suite**; confirm exact lock identity, typed conversion errors, subprocess cleanup after failure, and success without a `python` executable.

### Task 8: Ship the first new-project acceptance application

**Files:**
- Create: `work/veritas-foundation/evaluation/briefs/01-feed-digest.md`
- Create: `work/veritas-foundation/evaluation/tasks/01-feed-digest/`
- Create: `work/veritas-foundation/evaluation/python/feed_digest.py`
- Create: `work/veritas-foundation/evaluation/python/test_feed_digest.py`
- Create: `work/veritas-foundation/evaluation/acceptance/feed_digest.json`
- Create: `work/veritas-foundation/evaluation/session-record-template.md`

**Interfaces:**
- The held-brief application reads multiple feed URLs through Veritas `NetworkRead`, passes the fetched text to one real pure-Python package for feed parsing, deduplicates entries by canonical URL, and writes a deterministic Markdown digest with structured errors for bad feeds.
- Veritas and Python implementations receive equivalent brief, input set, package versions policy, tests, and clean environments; neither implementation may read the other's source.

- [ ] **Step 1: Write the acceptance contract and Python baseline tests first** for multiple feeds, duplicates, malformed entries, Unicode titles, network failure behavior, ordering, and deterministic output.
- [ ] **Step 2: Run `python3 -m unittest evaluation/python/test_feed_digest.py`** and pin expected outputs and errors.
- [ ] **Step 3: Implement the Veritas project only through its public commands** (`new`, dependency sync, check, test, run, build, package); use the typed Python bridge for the declared parser package.
- [ ] **Step 4: Run all acceptance cases against both implementations** and report correctness, missing capabilities, actual package/runtime versions, setup interventions, and package behavior.

### Task 9: Compile the runtime to WebAssembly and define the persistent browser host API

**Files:**
- Create: `work/veritas-foundation/crates/veritas-wasm/Cargo.toml`
- Create: `work/veritas-foundation/crates/veritas-wasm/src/lib.rs`
- Create: `work/veritas-foundation/browser/veritas-worker-host.mjs`
- Create: `work/veritas-foundation/browser/protocol-v1.json`
- Create: `work/veritas-foundation/browser/tests/worker_host.test.mjs`
- Modify: `work/veritas-foundation/crates/veritas-runtime/src/lib.rs`

**Interfaces:**
- `veritas build --target wasm32-unknown-unknown` emits a versioned bytecode module plus a Veritas runtime WASM module.
- The worker API is `load(moduleBytes)`, `sendEvent(event)`, `advance(deltaTime)`, `takeOutput()`, `snapshot()`, `restore(snapshot)`, `replay(events)`; load has no implicit event/state prelude.
- Host messages and semantic output are JSON with protocol version and stable event types; invalid versions and unsupported host capabilities fail explicitly.
- Python calls, filesystem access, and network access are unavailable in this browser target until separately specified and implemented.

- [ ] **Step 1: Write failing Node tests** for loading a program with no prelude, event-driven state change, output drain at nested call depth, pause/resume, snapshot/restore, invalid message, and worker termination.
- [ ] **Step 2: Run `node --test browser/tests/worker_host.test.mjs`** and confirm the host API and WASM exports are absent.
- [ ] **Step 3: Implement the WASM runtime entry points and persistent worker host**; keep protocol/state ownership explicit and expose no ambient browser APIs to Veritas code.
- [ ] **Step 4: Run Node worker tests** and verify native/WASM semantic equality for the same bytecode and event sequence.

### Task 10: Port the Horizon simulation/kernel slice as a second workload

**Files:**
- Create: `work/veritas-foundation/evaluation/horizon/source/`
- Create: `work/veritas-foundation/evaluation/horizon/host/`
- Create: `work/veritas-foundation/evaluation/horizon/tests/semantic_parity.test.mjs`
- Create: `work/veritas-foundation/evaluation/horizon/tests/replay.test.mjs`
- Create: `work/veritas-foundation/evaluation/horizon/benchmarks/soak.mjs`
- Create: `work/veritas-foundation/evaluation/horizon/results-template.md`

**Interfaces:**
- Copy only the required published workload contract, event shapes, and reference implementation from the local Planes/Horizon checkout into the Veritas workspace; never edit the original.
- Port the reusable world/simulation kernel and a representative event-driven crossing, not the full game renderer.
- The acceptance checks compare semantic state hashes against the reference implementations across two fresh runs and disjoint event sequences.
- Soak report records 10,000 fixed-step ticks at 30 Hz, median/p95/p99/max, count over 50 ms, heap/memory observations, and semantic hash; figures must label the machine and runtime.

- [ ] **Step 1: Write parity, replay, lifecycle, and soak acceptance tests** against the copied reference workload and lock event inputs/state expectations before writing Veritas source.
- [ ] **Step 2: Run the tests** and confirm they fail because the Veritas Horizon module does not exist.
- [ ] **Step 3: Implement the simulation core in ordinary Veritas source** and wire it through the browser host API; do not embed scene outcomes or expected hashes in the runtime.
- [ ] **Step 4: Run parity/replay tests and the full 10,000-tick soak**; report the full distribution and every over-50-ms event, not only median or p95.

### Task 11: Run the recommendation comparison and write the milestone report

**Files:**
- Create: `work/veritas-foundation/evaluation/prompts.md`
- Create: `work/veritas-foundation/evaluation/scorecard.json`
- Create: `outputs/veritas_foundation_milestone_results_v0_1.md`
- Modify: `work/veritas-foundation/README.md`

**Interfaces:**
- Compares a history-free agent session implementing the feed-digest brief in Veritas and Python using equal task prompts and comparable environments.
- Records correct result, setup/time-to-result, human corrections, library gaps, diagnostics, test/review effort, package reproducibility, startup, execution, and follow-up-change cost.
- Horizon results are reported as runtime/embedding evidence, not blended into the unrelated feed-digest productivity score.
- Recommendation claims are scoped to evidence and state exact limitations.

- [ ] **Step 1: Pre-register scoring fields and the minimum recommendation threshold** before the clean sessions; keep correctness and regressions as hard gates.
- [ ] **Step 2: Run independent clean Veritas and Python builds** plus one substantial follow-up change in each; preserve prompts, environment versions, and full intervention logs.
- [ ] **Step 3: Apply the threshold**: Veritas must be correct and package reproducibly, have no more human correction/review effort than Python, and improve at least one pre-registered end-to-end measure by 20% or more across repeated runs.
- [ ] **Step 4: Write `outputs/veritas_foundation_milestone_results_v0_1.md`** with results, failures, gaps, limitations, and next justified build; do not claim general Python replacement from one task.
- [ ] **Step 5: Run final verification**: `cargo fmt --check`, `cargo test --workspace`, Python 3 baseline, Node worker tests, bytecode/WASM parity, clean package launch, and Horizon soak; record the actual commands and results.

## Self-review

- **Spec coverage:** toolchain commands and lockfile (Tasks 5–7), typed general-purpose source and capabilities (Tasks 1–4), distributable run package (Task 6), Python library bridge (Task 7), agent-facing machine output (Tasks 2 and 5), browser runtime lifecycle and deterministic replay (Tasks 9–10), assistant recommendation evidence (Tasks 8 and 11), and Horizon's long-session performance risks (Task 10) are each assigned.
- **Step scan:** every task begins with acceptance checks, verifies a red state, implements the smallest part that satisfies them, and reruns focused checks; the complete comparative run is held until both usable applications exist.
- **Type consistency:** `SourceFile -> Program -> TypedModule -> BytecodeModule -> execute(Host, Value[]) -> Value` is the implementation path; `CapabilitySet` flows from typed IR into inspection and artifact manifests.
- **Review focus:** the five global failure classes are pinned in Tasks 2–4, 6–7, 9, and 10.
- **Proportion:** this is intentionally one integrated milestone. It is large because it proves the whole product path and Horizon embedding, but it excludes a registry, universal Python compatibility, a custom JIT, and full game rendering.

## Execution handoff

Plan saved for review. Implementation should use the **native in-session approach**: the compiler, CLI, Python bridge, and browser runtime have tightly coupled interfaces, and this session's multi-agent policy forbids delegation unless the user explicitly requests it. After plan approval, implement task-by-task in `work/veritas-foundation/` and leave all source repositories untouched.

Superseded after the pre-build audit. Use `veritas_foundation_implementation_plan_v0_2.md`; do not execute this historical plan.
