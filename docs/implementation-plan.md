# Veritas foundation implementation plan v0.2

> Historical implementation plan. The active completion plan is [Veritas complete product](superpowers/plans/2026-10-24-veritas-complete-product.md), which reconciles the later full self-hosting and Python-free core requirements. Retain the history below as prior evidence, not the next execution queue.

Status: approved by Rob for implementation; build in progress. Supersedes the October 22 v0.1 plan. Spec: `north-star-design.md`. Pre-build audit findings are recorded in the prior conversation.

Latest CLI progress: v1.34 added `sync --json` lock fingerprints, v1.35 added typed dependency setup failure categories, v1.36 adds `doctor --json` environment readiness discovery, v1.37 aligns the shipped Python ABI guide with current bundled-runtime behavior and guards that claim with an acceptance test, v1.38 adds no-follow destination checks plus atomic no-replace package publication on macOS, v1.39 validates release archive members and rewrites packaged Python environment links away from staging paths, v1.40 reports newly resident native images loaded through Python at call boundaries on macOS, v1.41 records and verifies native images found in extracted offline wheel-cache distributions, v1.42 makes dependency updates transactional, v1.43 documents the tested artifact/package/Python/host compatibility boundaries and migration procedure, and v1.47 adds static capability inspection and the prior-language reuse inventory. RC71 passes the 34-test installed acceptance kit, 126 workspace tests, relocation, package verification and launch, offline dependency update, and archive safety checks. The shipped run ledger validates paired evidence; independent recommendation evidence remains open.

Goal: deliver one general-purpose release candidate covering the complete builder lifecycle, with honest comparative evidence. This plan replaces the mandatory browser/Horizon port with concurrency, interop, deployment, documentation and maintenance work that directly supports the north star.

Implementation update: v1.14 added JSON construction; v1.15 added atomic text replacement plus typed missing-file errors; v1.16 added a typed `Json` value bridge to Python; v1.17 added checked NumPy array materialization into declared nested Veritas lists; v1.20 added source-free current-host packaging for managed Python and native wheels; v1.21 adds `verify-package`, launch-time hash checks, stable Python runtime contents across repeated launches, and a shipped evaluation kit; v1.22 adds a project-environment Python dependency/license inventory; v1.23 adds managed-interpreter pip metadata, CPython/uv/Tcl/Tk notices, and a fail-closed macOS Mach-O load-command scan with per-image integrity checks. v1.24 carries and verifies Rust dependency notices inside app bundles built from prepared distributions; v1.25 attributes installed native Python images to `RECORD` owners and verifies their linked license records; v1.26 adds a tested current-host installer that keeps versioned runtime files together and avoids overwriting unrelated commands; v1.27 adds opt-in JSON check, test and run reports with stable codes, captured output, summaries and source-mapped locations; v1.28 hashes immutable offline Python-cache payloads in package schema 2 while keeping schema 1 verifiable and excluding uv's mutable bookkeeping records. v1.29 adds a versioned JSON API report for Veritas functions, records, and declared Python calls. v1.30 adds an installed-product docs-and-diagnostics repair acceptance test. v1.31 makes the shipped Python acceptance suite resolve its workloads and helper modules from the installed release. v1.32 adds machine-readable build results and compile diagnostics. v1.33 adds machine-readable package creation and verification reports. v1.34 adds machine-readable dependency sync results with both lock fingerprints. v1.35 gives dependency setup typed failure categories for resolver, lock, and managed-runtime errors. v1.40 observes newly resident macOS dyld images after Python bridge calls and exposes paths in structured run reports; it does not add dynamic observations to package inventories. v1.41 records native files in extracted offline uv wheel-cache distributions separately from active runtime images and verifies their `RECORD` ownership, architectures, and hashes. v1.42 stages Python dependency updates and commits the new environment only after resolution, installation, and interpreter checks succeed; failures preserve the prior lock files and environment. v1.43 ships the pre-1.0 compatibility and migration policy, including the artifact reader range and current host restrictions. RC69 passes all 31 installed acceptance checks, relocation, package verification and launch, and an offline update-and-run from the relocated packaged CLI. The stdlib evaluation ledger is shipped; cross-host targets and independent recommendation evidence remain open.

## Latest north-star evidence

The v1.48 figures below are the historical VM baseline. The active north-star sprint now includes a Cranelift 0.135.0 JIT for verified numeric functions, with checked integer overflow fallback to the VM, Float64 operations, direct calls between acyclic native functions, exact per-instruction fuel accounting, and visible native coverage. In the latest fresh-process probe on this host, source Mandelbrot is 19.19 ms versus Python 42.48 ms and optimized C 3.70 ms; the 200,000-iteration, four-task `parallel_map` is 14.47 ms versus Python serial 84.01 ms and threads 88.69 ms. These author-run macOS arm64 medians meet the sprint's proposed 2× Python threshold on these two probes, but they do not establish recommendation superiority or broad performance. Three substantive development applications now cover reconciliation, restartable API work, and package-backed analysis; their follow-up edits, additional host evidence, and independent recommendation comparisons remain open. Raw release report: `evaluation/results/native-backend-probe-cranelift-0.135.0.json`; running log: `docs/north-star-sprint-log.md`.

## Ownership and boundaries

Implementation root: `work/veritas-foundation/`. Commands in this plan run from that root. Source repositories and original uploads remain unchanged. Internal steps are engineering dependencies, not separate user approval gates.

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

- [x] Inventory existing generic prototype components against the revised spec; identify copy-with-tests, adapt, and exclude decisions. The local Planes/Decan/Liminate-dev evidence and bounded review limits are recorded in `docs/reuse-inventory.md`; no source code was copied. Exclude invoice/phone-specific preludes and development `.rlib` discovery.
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
- [x] Add typed JSON construction and immutable updates so a source program can create persisted JSON state without relying on a Python writer.
- [x] Add typed missing-file errors and atomic text replacement; test artifact format gating and persistence from both source and source-free bytecode.
- [ ] Implement bounded parallel workers, backpressure, ordered results, task identity and joined scope exit. On failure, stop scheduling, cancel pending work, join running workers and report the triggering error plus any secondary failures.
- [ ] Poll cancellation in language execution and integrate timeouts with host calls. Terminate/reap stalled Python workers where supported; do not imply rollback of completed side effects.
- [ ] Test CPU parallel work separately from overlapping I/O. Include failure while siblings run, cancellation before/after effects, saturated queues, repeated groups and cleanup. Avoid brittle hard speedup assertions on machines with different core counts.
- [x] Expose known runtime-mediated operations and opaque foreign calls in human and JSON inspection; include transitive reachability through direct calls and parallel workers, and do not present declared annotations as verified confinement. See checkpoint v1.47.

Acceptance: concurrency is callable from ordinary source and has bounded resource behavior and defined failures. The report states which work ran concurrently and which actually used multiple CPU cores.

## Work package E — usable CLI, documentation, tooling and packages

Files: `crates/cli/src/commands/`, `docs/reference/`, `distribution/packaging/`, `crates/cli/tests/`, `tests/acceptance/`.

- [ ] Implement new/check/fmt/test/run/build/package/sync/inspect/doc/doctor against the same public compiler/runtime path. Formatter preserves meaning and comments; `fmt --check` reports drift. Tests discover the specified test declarations and isolate state.
- [x] Add current-host `package`: compile source-free bytecode, copy the matching runtime and project policy, include only explicitly declared resources, create a launcher, record artifact/runtime/resource hashes, stage output before installation, and reject package-owned resource paths.
- [x] Bundle managed CPython, pinned `uv`, locked Python bridge dependencies and the offline cache; verify Python-assisted apps after moving them and running twice without source or external Python setup. Evidence covers macOS ARM64 with `feedparser`, `orjson`, and NumPy.
- [x] Verify package target, artifact/runtime/resource hashes, managed Python tree, pinned resolver and lock before launching; schema 2 additionally verifies immutable offline-cache payload hashes, preserves schema 1 compatibility, and rejects cache corruption across the launcher boundary.
- [x] Inventory distributions in the locked project environment and managed interpreter from wheel metadata; preserve CPython, uv, Tcl/Tk and distribution-provided notices; verify copied license-file digests. This is an integrity/inventory feature, not a license compatibility or legal-compliance assessment.
- [x] Add `veritas doc --json` as a versioned machine-readable report of function types, record fields, and declared Python-call signatures; source-order parameter types are reported because the artifact does not retain Veritas parameter names.
- [x] Add `veritas build --json` with artifact identity, host/compiler versions, source digest when available, and structured compile diagnostics.
- [x] Add `package --json` and `verify-package --json` with package identities, metadata hashes, tamper failures, and structured package-source diagnostics.
- [x] Add `sync --json` with stable setup diagnostics, update status and fingerprints for both dependency locks.
- [x] Verify one installed-product repair loop using only the bundled CLI, reference, and host API docs: structured unknown-name diagnostic, documented fix, successful check, run, and source test.
- [ ] Ship version-matched local docs, executable examples, searchable public signatures, structured diagnostics/test reports and source runtime traces. Test repairing an error using only these shipped materials.
- [ ] Record compiler/artifact/host/schema versions, target, source/resource digests and dependency locks in build metadata. Reject incompatible artifacts rather than silently misexecuting them.
- [x] On macOS, scan Mach-O images in the app runtime, managed CPython, installed Python extensions and declared resources. Record `LC_LOAD_DYLIB` edges, image hashes, target architectures, bundled paths and system paths; fail packaging on unresolved or ambiguous load references; verify image hashes before launch. Runtime notices for CPython, pip, uv and Tcl/Tk are included.
- [x] Report newly resident macOS dyld images after each Python bridge call in `run --json`, including failed calls, and validate the end-to-end report with a dynamically compiled library. This is an execution observation, not a sandbox or a complete trace: libraries loaded and unloaded between call boundaries may be missed.
- [x] Inventory Mach-O files in extracted offline uv wheel-cache distributions separately from app runtime images. Record file digests, target architectures, load names, distribution names/versions, and the owning cache `RECORD`; verify each record and native-file digest at package verification.
- [ ] Keep runtime observations distinct from static package contents. Extend install/update support and path-with-spaces/Unicode coverage. Run the full test matrix on additional host targets when available.
- [x] Make the shipped acceptance suite runnable from an installed release root; bundle its helper module and resolve known workloads from installed examples. Verify the suite against the installed CLI.
- [x] Reject package output destinations that already contain any filesystem entry, including dangling symlinks, and publish the staged directory with an atomic no-replace rename on the supported macOS target. CLI coverage packages, verifies and launches from a path containing spaces and Unicode while the process runs from a different working directory; a publication regression simulates a competing destination and confirms neither it nor the stage is replaced.
- [x] Validate release archive paths before publishing: reject absolute/traversal names, duplicate members, unsupported entries, escaping or missing links, and entries beneath links. Suppress macOS AppleDouble metadata. Rewrite bundled Python virtual-environment links that point into staging to relative links within the final package.
- [ ] Exercise unavailable original caches/toolchains and additional relocation environments; validate failure behavior across launcher boundaries.
- [ ] Where available run an actual fresh target environment. Record lack of such a target as an unverified deployment claim, not a pass.
- [x] Add an explicit dependency-update workflow that stages lock resolution and Python installation, verifies the managed interpreter, then renames the bridge directory and atomically replaces lock metadata with rollback on metadata failure. Test forced install failure preserves the prior lock files and environment, then test a successful update remains runnable.
- [x] Document the pre-1.0 source/API boundary, accepted artifact and package schema ranges, pinned Python/uv expectations, current host support, and manual project/data migration steps; ship the policy and check its installed copy.

Acceptance: someone can create, understand, modify, test, run and deliver a project using the installed product. A temporary directory is labelled a relocation test; genuine clean-machine success requires separate evidence. Reproducible artifact bytes exclude or normalize volatile metadata if advertised.

## Work package F — broad development programs, maintenance and evaluation

Files: `evaluation/development/`, `evaluation/protocol/`, `evaluation/results/`, user-facing milestone report in the parent workspace's `outputs/`.

- [ ] Develop ordinary programs spanning file/data CLI work, concurrent network jobs, package-backed computation, and a persistent application. Include nontrivial modules, collection transformations, errors and changed requirements. Tests may use controlled inputs; production runtime behavior must remain general.
- [x] Add a multi-module persistent CLI app with a separate standard-library reference; verify first-run state creation, atomic replacement, source-free bytecode execution, exact output/status parity, and persisted bytes. This does not yet demonstrate transaction isolation or concurrent update merging.
- [ ] Label feed digest and Horizon-derived workloads as known development cases. If Horizon is copied, normalize observable output against a pinned reference revision; do not compare unspecified internal hashes or require Planes provenance machinery. No browser port is needed for this milestone.
- [x] Complete long-session allocation/output/lifecycle stress and paced responsiveness evidence. RC69 and RC70 packaged probes each performed 10,000 persistent NumPy/FFI burst calls plus 250 calls paced at 10 ms; they recorded host/runtime, warmups, input size, throughput, latency percentiles, close, post-close behavior and 50-ms Rust-process-tree RSS including the Python sidecar. Across 137 RC69 post-start samples the sidecar RSS remained 36.375 MiB and whole-tree RSS rose 112 KiB (sampled peak 40.25 MiB). Across 138 RC70 samples after two seconds the sidecar RSS remained 41.969 MiB and tree RSS rose 128 KiB (whole-run peak 45.844 MiB). These are author-run macOS arm64 samples, not a general leak bound or recommendation gate; the observed steady sidecar values differ between runs. Do not adopt Horizon's 50 ms as a universal language gate.
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

The detailed grammar, ABI limits and platform asset versions are closed during A/C before their dependent code. This is an integrated build plan, not a claim those specifications or release assets already exist. Rob approved this scope; the implementation and acceptance evidence remain unfinished.
