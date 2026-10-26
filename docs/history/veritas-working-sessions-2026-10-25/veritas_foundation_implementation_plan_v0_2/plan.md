# Veritas foundation implementation plan v0.2

Status: proposed after pre-build audit; no implementation authorized by this document alone. Supersedes the October 22 v0.1 plan. Spec: `veritas_north_star_build_design_v0_2.md`. Audit: `veritas_prebuild_audit_v0_1.md`.

Goal: deliver one general-purpose release candidate covering the complete builder lifecycle, with honest comparative evidence. This plan replaces the mandatory browser/Horizon port with concurrency, interop, deployment, documentation and maintenance work that directly supports the north star.

## Ownership and boundaries

Implementation root after approval: `work/veritas-foundation/`. Commands in this plan run from that root. Source repositories and original uploads remain unchanged. Do not create product scaffolding before approval. Internal steps are engineering dependencies, not separate user approval gates.

Proposed modules:

- `language/`: grammar, semantics, public examples, host and external-call contracts.
- `crates/model/`: source spans, diagnostics, type schemas, portable value representation; no compiler/runtime dependency.
- `crates/compiler/`: lexer/parser, module resolution, type checking, typed lowering, source maps, bytecode emission.
- `crates/runtime/`: bytecode verification/execution, memory/lifecycle limits, host services, structured workers, Python supervisor.
- `crates/cli/`: public commands, manifest/lock handling, documentation discovery and reports.
- `distribution/`: pinned toolchain assets, checksums, notices, target-specific package assembly and relocation checks.
- `tests/acceptance/` and each crate's `tests/`: black-box lifecycle and semantic checks. A virtual Cargo workspace has no implicit root integration-test package.
- `evaluation/development/`: known workload programs and reference results.
- `evaluation/protocol/`: registered measures, independent task intake, session logs, frozen-version identifiers.

Dependency direction: model is shared by compiler/runtime; CLI depends on both. Bytecode uses model-owned portable values and its own serializable instructions; it cannot depend on interpreter-owned objects. The public artifact schema is versioned; internal IR is not promised stable.

## Work package A — close the language/architecture contract and inventory reuse

Files: `language/spec.md`, `language/examples/`, `language/host-api.md`, `language/python-abi.md`, `docs/architecture.md`, `docs/reuse-inventory.md`, workspace manifests.

- [ ] Inventory existing generic prototype components against the revised spec; identify copy-with-tests, adapt, and exclude decisions. Exclude invoice/phone-specific preludes and development `.rlib` discovery.
- [ ] Record why bytecode is chosen for this release, its performance obligations, memory ownership strategy, source mapping, artifact versioning, and what a future native backend would have to preserve.
- [ ] Specify indentation, functions, first-class function references for parallel work, public annotations/local inference, records, lists/maps, loops, `Option`, `Result`, propagation, test declarations, and external declarations. Define supported generic behavior and import/export rules.
- [ ] Close numeric/collection semantics: proposed unbounded Int; explicit Float64; integer division/remainder; equality; map iteration; Unicode string operations; immutable value/copy behavior; failure rules. Include nested collections and serialization errors.
- [ ] Specify host operations for argv/env, text/bytes files, JSON, bounded HTTP, output, and clocks, with signatures, effects, errors and target availability. No network capability may appear only in an application task.
- [ ] Define the external signature schema and conversion matrix, including keyword arguments, integer/bytes transport, absence, non-finite numbers, unsupported Python objects, runtime type mismatch and package errors.
- [ ] Write independent expected-value/error cases first. Activate parser tests when the parser exists and execution cases when the runtime exists; do not mark absent implementations as passing.

Acceptance: every syntax/API used by development programs has an explicit contract. A source example demonstrates authoring and error repair without implementation-specific knowledge. Architecture choices and their costs are visible; no “all examples parse” claim before a parser exists.

## Work package B — general compiler, verified runtime and source debugging

Files: model, compiler and runtime crates; `crates/compiler/tests/{syntax,typing,modules}.rs`; `crates/runtime/tests/{execution,artifacts,lifecycle}.rs`.

Interfaces to freeze in A: `parse(SourceFile) -> Program|Diagnostics`; `check(ModuleGraph) -> TypedProgram|Diagnostics`; `lower(TypedProgram) -> Artifact|Diagnostics`; `Runtime.load(Artifact, HostPolicy) -> Instance`; `Instance.invoke(export, values) -> Value|RuntimeError`; `Instance.close()`. Invocation identifies the export explicitly. One-shot CLI and repeated application calls use the same engine.

- [ ] Implement parsing and source spans with complete token consumption; test malformed syntax and Unicode spans.
- [ ] Implement type checking, symbol visibility, module graph resolution, return-path checking and stable diagnostics. Test module cycles, collisions, generic container use and incorrect external declarations.
- [ ] Implement typed lowering, bytecode format/verifier, calls, control flow and source maps; validate stack/type/control-flow and index consistency on artifact load.
- [ ] Implement value semantics and resource limits for stack, steps, allocation accounting and output. Bound recursion and cancellation polling; runtime implementation bugs must not masquerade as source errors.
- [ ] Implement explicit lifecycle and source-level stack traces. Test repeated invocations, independent instances, cleanup and failure after partial external work.
- [ ] Exercise known and generated small programs, algebraic/metamorphic properties, changed constants/control flow, malformed artifacts, and independent expected results. Tests must expose semantic mutations rather than just compare two wrappers around the same engine.

Acceptance: all supported features execute from arbitrary source; every construct typechecked as supported has lowering/runtime coverage. Cargo formatting and crate suites pass; source changes alter outputs as semantics require. Compiler internals contain no application-specific answers.

## Work package C — managed dependencies and real Python calls

Files: `crates/runtime/src/python/`, `crates/cli/src/dependencies/`, `distribution/toolchains/manifest.json`, corresponding integration tests.

- [ ] Define `veritas.toml` project/package/entry/test/resource fields and local Veritas dependency identities. Lock source digests and reject dependency path collisions/escapes according to the declared package root.
- [ ] Use a separate uv-managed bridge project with its native lockfile. `veritas.lock` records its digest, interpreter identity, resolver identity, target and artifact checksums; do not discard transitive/marker/index/hash information.
- [ ] Implement explicit sync/update and locked execution. Acquire pinned verified uv/CPython assets into managed storage, with offline cache behavior and useful missing-asset errors. Do not select an incidental system interpreter silently.
- [ ] Implement the dedicated framed sidecar channel with separate stdout/stderr, request IDs, bounded frames/nesting, schema validation, timeout, EOF/crash handling and cancellation cleanup. Imports and calls may themselves fail or print output.
- [ ] Statically check caller declarations; dynamically validate bridge values. Keep foreign effects opaque even when annotations exist. A child process is not a sandbox.
- [ ] Test a useful pure-Python library, a compiled wheel, and an object/array-heavy compatibility challenge. Record required adapter code and copies; test corrupted data, precise large integers, bytes, keyword calls, wrong return types and stalled/terminated children.

Acceptance: one Veritas invocation manages dependency setup; later locked runs do not mutate the lock or rely on global Python/uv installations. Useful package calls work through documented declarations; unsupported objects fail without data loss. Array/object compatibility limitations remain visible in evaluation.

## Work package D — structured concurrency and complete host operations

Files: `crates/runtime/src/{host,workers}/`, compiler signatures for the concurrency library, `crates/runtime/tests/{host,parallel,cancellation}.rs`.

Initial general operation: `parallel_map(items, worker_function, limit) -> Result[List[U], TaskError]`. Function references are typechecked; captured/input values must obey the immutable transfer contract from A. Worker instances do not share mutable interpreter state.

- [ ] Implement the catalogued filesystem, HTTP, JSON and output APIs through runtime-owned host services. Test status failures, response/stream limits, timeouts, resource cleanup and partial-write reporting.
- [ ] Implement bounded parallel workers, backpressure, ordered results, task identity and joined scope exit. On failure, stop scheduling, cancel pending work, join running workers and report the triggering error plus any secondary failures.
- [ ] Poll cancellation in language execution and integrate timeouts with host calls. Terminate/reap stalled Python workers where supported; do not imply rollback of completed side effects.
- [ ] Test CPU parallel work separately from overlapping I/O. Include failure while siblings run, cancellation before/after effects, saturated queues, repeated groups and cleanup. Avoid brittle hard speedup assertions on machines with different core counts.
- [ ] Expose known runtime-mediated operations and opaque foreign calls in inspection; do not present declared annotations as verified confinement.

Acceptance: concurrency is callable from ordinary source and has bounded resource behavior and defined failures. The report states which work ran concurrently and which actually used multiple CPU cores.

## Work package E — usable CLI, documentation, tooling and packages

Files: `crates/cli/src/commands/`, `docs/reference/`, `distribution/packaging/`, `crates/cli/tests/`, `tests/acceptance/`.

- [ ] Implement new/check/fmt/test/run/build/package/sync/inspect/doc/doctor against the same public compiler/runtime path. Formatter preserves meaning and comments; `fmt --check` reports drift. Tests discover the specified test declarations and isolate state.
- [ ] Ship version-matched local docs, executable examples, searchable public signatures, structured diagnostics/test reports and source runtime traces. Test repairing an error using only these shipped materials.
- [ ] Record compiler/artifact/host/schema versions, target, source/resource digests and dependency locks in build metadata. Reject incompatible artifacts rather than silently misexecuting them.
- [ ] Build current-host pure-Veritas bundles with runtime, resources and launch entry; build Python-assisted bundles with relocatable interpreter, locked wheel dependencies, native-library accounting and notices. Reject unsupported targets/dependencies explicitly.
- [ ] Validate archive paths and output replacement; verify failures survive launcher boundaries. Test offline launch, paths with spaces/Unicode, changed working directory, relocation and unavailable original caches/toolchains.
- [ ] Where available run an actual fresh target environment. Record lack of such a target as an unverified deployment claim, not a pass.
- [ ] Add version compatibility policy, migration notes and an explicit dependency-update workflow. Exercise update failure recovery without silently changing a previously locked working project.

Acceptance: someone can create, understand, modify, test, run and deliver a project using the installed product. A temporary directory is labelled a relocation test; genuine clean-machine success requires separate evidence. Reproducible artifact bytes exclude or normalize volatile metadata if advertised.

## Work package F — broad development programs, maintenance and evaluation

Files: `evaluation/development/`, `evaluation/protocol/`, `evaluation/results/`, user-facing milestone report in the parent workspace's `outputs/`.

- [ ] Develop ordinary programs spanning file/data CLI work, concurrent network jobs, package-backed computation, and a persistent application. Include nontrivial modules, collection transformations, errors and changed requirements. Tests may use controlled inputs; production runtime behavior must remain general.
- [ ] Label feed digest and Horizon-derived workloads as known development cases. If Horizon is copied, normalize observable output against a pinned reference revision; do not compare unspecified internal hashes or require Planes provenance machinery. No browser port is needed for this milestone.
- [ ] Run long-session allocation/output/lifecycle stress and paced responsiveness checks where relevant. Record machine/runtime, warmup, input size, repetition count, throughput, p50/p95/p99/max and memory trend. Separate burst throughput from wall-clock scheduling; do not adopt Horizon's 50 ms as a universal language gate.
- [ ] Freeze compiler, docs and packages before comparative evaluation. Pre-register task classes, one primary outcome per class, correctness/deployment/maintenance guardrails and the number of sessions feasible with available resources.
- [ ] Obtain genuinely independent unseen briefs and follow-up changes when independent builders/sessions are available. Preserve prompts, model identifiers, tools, attempts, human interventions, exclusions and environment costs. Do not use new directories as a substitute for fresh context.
- [ ] Give Python current appropriate tooling, an environment-managing entry command, and equal task access. Attribute language, runtime, documentation and integration benefits separately; tooling convenience alone cannot establish that switching languages is beneficial. For any claim of a unique advantage, inspect the nearest existing-language alternative relevant to that claim. Do not charge Veritas compiler development to a user task or hide one-time installation costs; show cold and warm user workflows separately.
- [ ] Report all correctness failures and costs. Choose meaningful effect-size targets before runs; treat 20% as a proposed practical target only. Publish variation and adverse cases, including where the assistant should recommend Python.
- [ ] If independent sessions or hosts are unavailable, ship the executable evaluation kit and author-run results with that limitation; do not claim the recommendation gate passed. Required build work remains complete separately from missing external evidence.

Acceptance: the complete release candidate runs multiple kinds of real program and survives substantial follow-up edits. An initial recommendation requires evidence across more than one relevant project class and usable deployment/compatibility; otherwise the verdict names what prevents recommendation. Full general-purpose default adoption is not claimed from this release.

## Final verification and reporting

Run `cargo fmt --check` and `cargo test --workspace` from the foundation root, then installed-product acceptance scripts for dependencies, offline relocation, concurrency cleanup, long-session behavior, executable documentation and maintenance. Run a Python baseline with `python3` where used. Pin exact commands after the repository exists; no fabricated commands or passing totals in this plan.

Deliver source, an installable local current-target release candidate (public signing/notarization remains unverified), example projects, executable docs, machine-readable reports, and a concise milestone verdict in `outputs/`. State precisely what was built, which claims were tested, the remaining limitations and whether the evidence justifies recommending Veritas for any task class.

## Explicit unresolved risks carried into the approved build

Bytecode may be slower than the competitive baseline; the sidecar may require unacceptable adapters or copying; compiled Python dependencies may defeat easy relocation; static typing may make ordinary authoring too verbose; independent evaluation resources may be absent. These are engineering/evidence risks with assigned checks, not permissions to call a narrower demo a complete product. Significant architecture or compatibility changes prompted by those results must be surfaced.

The detailed grammar, ABI limits and platform asset versions are closed during A/C before their dependent code. This is an integrated implementation proposal, not a claim those specifications or release assets already exist. Proceed only after Rob approves the revised scope.
