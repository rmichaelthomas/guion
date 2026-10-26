# Veritas north-star build design v0.2

Date: October 22, 2026. Status: revised proposal, awaiting build approval. Supersedes v0.1. Read with `veritas_prebuild_audit_v0_1.md` and `veritas_foundation_implementation_plan_v0_2.md`.

## Goal and recommendation hypothesis

Build a general-purpose language that an informed assistant can recommend to a builder over Python for real projects because creating, checking, changing, running, and delivering the result is materially easier or more dependable. Broad default-language adoption remains the ambition. The first release must establish a credible recommendation across several project classes and explicitly identify tasks for which Python is still the better choice.

Our proposed differentiator is a consistent, inspectable project lifecycle backed by a readable typed language. That is a hypothesis, not an established competitive advantage. We must compare against well-configured Python with current tooling. Language syntax, native compilation, structured concurrency, and an integrated CLI are not independently novel. Attribute improvements to language semantics, runtime behavior, or tooling; a better launcher alone does not show why switching languages helps. Planes remains its own general-purpose project, and Veritas is not assumed to replace it.

The first build is one substantial release candidate, without a guaranteed one-session completion: language core, project tooling, dependencies and interoperability, concurrency, debugging, documentation, distribution, and end-to-end evidence. Internal engineering tasks will not each be presented as a product milestone or require a new user approval.

## User requirements versus implementation proposals

User requirements: general-purpose ambition; earning assistant recommendations; anticipate future use now; take a substantial swing; general source execution rather than application-specific shortcuts; honest reporting; learn from Planes, Liminate and Decan; preserve original source repositories and uploaded artifacts; no signing detour; build only after the current review and approval.

Proposed choices, not prior user decisions: Rust implementation; bytecode as the first execution engine; indentation syntax; inferred local types; arbitrary-precision integers; bounded parallel work; a managed CPython sidecar; a first distribution for the current macOS arm64 host. Approval of v0.2 would authorize these defaults. They may need adjustment if implementation evidence contradicts them, with material scope or compatibility changes reported explicitly.

## What we inherit from previous work

Planes was intended as a general-purpose programming language and was used to build Horizon. That attempt deserves study as actual application work. Horizon informs initialization, persistent calls, host separation, output delivery, deterministic pure logic, memory growth, and latency tails. Its world snapshots, derivation graphs, scene hashes, and browser renderer are application-specific contracts; they are not automatically Veritas features.

Liminate and Liminate-dev inform executable examples, readable errors, and the work required to turn a language into useful products. Decan informs explicit support boundaries and honest conversion errors. Their specialized semantics and branding do not determine Veritas's semantics.

A clean workspace may reuse audited generic prototype components. Reuse requires an explicit inventory and conformance to the new contract. Application-specific preludes, canned outputs, and development-directory linkage must not enter the foundation.

## Complete builder experience

One installed `veritas` command owns `new`, `check`, `fmt`, `test`, `run`, `build`, `package`, `sync`, `inspect`, `doc`, and `doctor`. A new project includes a minimal entry point and test, a manifest, and links to the installed version's language reference. It does not contain a completed workload.

Users can find public APIs and types locally, receive stable diagnostic codes with source locations and repair guidance, inspect runtime stack traces in Veritas source, execute tests, and pass the project to another builder. Human output and structured JSON reports describe the same outcome. Failed checks, tests, subprocesses, and package launches remain failures through the CLI.

Separate reproducible resolution, reproducible artifact bytes, and repeatable behavior. Advertise each only after its own check. Locked run/check/build do not silently update dependencies; explicit sync/update owns changes. The first package format may contain multiple files, including runtime assets. A single command is the user experience; a universally portable executable is not promised.

## Language and runtime defaults

Use indentation and familiar functions, conditions, loops, records, collections, and explicit imports. Infer local types, annotate public boundaries, and make recoverable failures explicit. Before writing compiler code, publish small executable source examples and settle the complete initial grammar and semantics in the implementation specification.

Proposed value rules: arbitrary-precision `Int`; explicit `Float64`; no silent integer/float conversion; immutable collection values with explicit replacements; a distinct `Option` for absence and `Result` for recoverable failure. Define exact division/remainder rules, string indexing, map key types/order, equality, aliasing, and external numeric conversion in that specification. These choices trade some runtime cost for predictable authoring; measure the cost. Do not silently inherit i64, i128, financial arithmetic, or provenance semantics from earlier projects.

Use one typed lowering path and one runtime semantics. Rust implements the tooling. A versioned bytecode engine is the proposed first backend because it can travel with the tool and avoid runtime compilation with a user-installed Rust toolchain. This is a setup and implementation choice, not a speed claim. Keep calls, value semantics, source maps, and host boundaries specified independently; keep internal IR private and changeable. Mature native backends remain alternatives if workload profiles justify them. Replacing an engine will still require conformance and integration work.

For persistent embedding, expose explicit load, invoke, output collection, and close/cancel operations. Applications serialize their own state; arbitrary runtime snapshots and exactly-once external effects are outside this release.

## Concurrency is implemented, not just described

Provide bounded structured parallel work over immutable inputs with isolated interpreter state for each worker. A group owns its tasks; returning from the group joins them. Results preserve input order; failures identify the task, stop new work, cancel pending work, and report completed effects. No unrestricted shared mutable variables cross workers.

Publish cancellation and timeout behavior for compute, I/O, and foreign calls. CPU workers must actually run in parallel where available; overlapping waits alone do not prove parallel computation. Define a bounded queue and a concurrency limit, and make cleanup part of the tests. A blocking foreign process may require termination; cancelling it does not undo effects already performed.

This bounded operation is the first concurrency surface, not a claim of a full async ecosystem. Python's existing concurrency facilities are a baseline to compare against.

## Python compatibility and dependencies

The first bridge is an explicit typed-call adapter, not transparent Python source compatibility. External declarations supply parameter and return schemas; Veritas statically checks the caller and validates arguments/results at runtime. Support positional and keyword arguments and document which values cross. Python function bodies remain dynamically checked.

Use a supervised CPython sidecar with a dedicated framed channel separate from package stdout/stderr. Version conversion rules, preserve integer precision using tagged decimal values, support bytes explicitly, and reject unsupported objects with actionable errors. Arbitrary callbacks, generators, context managers and object identity are not implied. Unsupported conversions must not stringify or silently lose data.

All Python calls remain opaque external effects in verified capability reports. Author declarations can document intent but cannot establish purity or confinement. Process separation helps lifecycle management; it is not an operating-system sandbox.

Use uv's resolver and native lock format as an implementation component. Veritas records the uv/Python versions, package lock digest, platform, artifacts and checksums without inventing a competing Python resolver. Bootstrap acquires the pinned tools into managed storage; programs use the recorded interpreter path. No user command depends on `python` versus `python3` naming. Initial acquisition may need network access; completed packages must meet their declared offline execution contract.

Test both pure-Python and native-wheel dependencies. Include an array/object-heavy package as a compatibility challenge and count adapter work and transfer cost. If important packages are impractical to use, document that as a reason to recommend Python. Passing a JSON-friendly parser is insufficient evidence of broad ecosystem access.

## Distribution and maintenance

First distribution target: current macOS arm64, with exact OS/runtime constraints recorded in the build report. Architecture permits later Linux, Windows and browser hosts without claiming they are supported now. No browser port or phone deployment is required for this release. This is a local release candidate; unsigned relocation tests do not establish public download trust, signing/notarization, or frictionless public installation. No user signing credentials are required for the language validation.

Pure Veritas packages include their runtime and resources. Python-assisted packages additionally assemble a relocatable managed interpreter and locked wheel dependencies for the supported target, including required native libraries and license records. Do not copy a developer virtual environment and label it portable. Unsupported dependency/platform combinations must fail packaging with an explanation.

Verify relocations and missing developer tools/caches. Run on a genuinely fresh target environment if available; otherwise state that clean-machine deployment remains unverified. Preserve meaningful version mismatches, artifact corruption errors, migration notes, and source-level debugging. Include dependency-update and application-change exercises.

## Evidence and future use

Development coverage spans: CLI/file/data tools; a concurrent network application; package-backed data processing including a compiled wheel; and a long-running application with explicit lifecycle. These are workload classes, not built-in runtime special cases. The feed reader and any copied Horizon computation are known development cases.

Before evaluation, freeze the compiler, docs and dependency policies. Independent tasks and change requests must not have been used to implement the language. Give Python competent contemporary tooling and the same brief and access to documentation. Record every failure and intervention. Where independent sessions or a second host are unavailable, deliver the reproducible evaluation kit and limit the claims rather than fabricating independence.

Choose one primary end-to-end measure per task class in advance, alongside correctness, setup, compatibility and maintenance guardrails. Report all outcomes, cold and warm costs, and variation. A 20% difference is an optional practical target, not statistical proof. More than one task class must support an initial multi-class recommendation; no small corpus proves universal default status. Known browser logic also needs a JavaScript comparison when browser claims are made.

Future-oriented commitments now: versioned external contracts, source-level diagnostics, executable docs, bounded host operations, structured concurrency, explicit foreign-code uncertainty, and upgrade/handoff records. Later work may add native optimization, richer Python object integration, additional targets, editor integration, or registry services as evidence requires. The north star requires ongoing ecosystem work beyond this first release.

## Remaining material risks

Bytecode speed, Python bridge usefulness, relocatable native dependencies, concurrency usability, and language learnability are unresolved engineering risks. None has been proven by the current prototype. The corrected build exposes these risks early within one integrated milestone. If evidence says the runtime or interop design cannot deliver a competitive experience, change that architecture rather than lowering the acceptance bar or declaring the task unsupported after choosing it.
