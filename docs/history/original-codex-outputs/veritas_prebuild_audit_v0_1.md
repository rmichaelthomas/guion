# Veritas pre-build audit

Date: October 22, 2026. Scope: review of the design and implementation plan, existing prototype interfaces, and selected Planes/Horizon source and reports. No product code or language repositories were modified; no implementation or benchmark was run for this review.

## Verdict

The v0.1 plan was not ready to build. Its broad direction fits the north star, but several technical promises were missing from the implementation, its evaluation would favor its own development examples, and some project-specific choices had become language requirements without justification. Executing it as written could produce another impressive demonstration without making Veritas a credible recommendation.

The revised v0.2 documents replace that plan. They retain a large integrated build: a usable language and toolchain, real dependency access, bounded concurrency, deployment, documentation, and maintenance exercises. They remove the mandatory Horizon browser port and add requirements that directly affect whether a builder should choose Veritas. They remain proposals awaiting Rob's approval.

## Findings and corrections

### 1. Horizon became a new product mandate without sufficient justification — high

Design v0.1 line 44 made a browser port the second mandatory workload immediately after Rob suggested mining Horizon as context. That repeats the NowLookHere mistake: a useful example starts determining the architecture and delivery scope. The same design calls WebAssembly a future candidate at line 93 while plan Tasks 9–10 require it now.

Correction: preserve Planes's general-purpose intent and Horizon's actual application history. Carry forward lifecycle, host separation, output delivery, long-session resource behavior, and realistic workload lessons. Use a copied Horizon computation as an optional stress case after baseline coverage; a full browser worker port is a subsequent target decision. Browser deployment is not evidence of superiority to Python for ordinary CLI/data work.

### 2. Planes-specific semantics were being imported into Veritas — high

The proposed universal snapshot/replay API, exact semantic hashes, and retention-related tests came from Planes's provenance/world model. Veritas has not adopted that model. A language need not serialize every stack, file descriptor, Python object, or in-flight request to support ordinary persistent applications.

Source check: `planes/test_crossing_port.py` hashes captured scene-output lines; it does not prove identity of the entire internal state. Its seed is fixed, and its two event sequences are valuable known cases, not randomized generalization evidence. The kernel benchmark reports 10,000 steps driven back-to-back, not a real-time scheduler holding 30 Hz. The earlier plan blurred these distinctions.

Correction: explicit initialization, repeatable function calls, output delivery, and close/cancel behavior belong in the runtime. Applications own serializable state and replay contracts. Compare normalized observable behavior across languages; compare internal hashes only when a shared canonical representation is specified. Report throughput runs separately from paced latency tests.

### 3. New syntax and numeric semantics appeared without a product reason — high

Plan Task 1 silently changed the existing indentation syntax to braces and mandatory semicolons and changed `Int` from the prototype's i128 representation to i64. Neither was Rob's decision. Missing rules included division, negative remainder, equality, map order, Unicode indexing, aliasing, and generic type inference.

Correction: label technical defaults as proposals. Retain indentation as the initial readability baseline; propose arbitrary-precision `Int`, explicit `Float64`, immutable collection values with explicit replacement, and typed failures. Write one semantic contract before implementation and exercise it across every engine. No automatic claim that these defaults are inherently better; report their memory/performance costs. Public source examples must make ergonomics reviewable.

### 4. The backend choice was presented as a conclusion — high

Bytecode can remove a user-installed Rust requirement, but it adds a VM, verifier, memory model, debugging, and artifact compatibility burden. It does not demonstrate better speed. Native compilation can reuse an established backend; it does not require inventing an optimizer. A backend-neutral IR alone does not guarantee cheap backend replacement.

Correction: retain bytecode as an explicit first-release proposal for an integrated runtime, with native execution speed still unproven. Specify value semantics, calls, host ABI, and source maps independently. Measure startup, CPU time, peak memory, and tail latency. Record a backend decision before coding and revisit it if complete workloads expose material performance disadvantages. Do not promise native-speed superiority in this build.

### 5. “Typed Python bridge” had no type source — blocking

Plan Task 7 imports a function by name but does not say where its signature comes from. A dynamic import, JSON payload, and declared module dependency do not establish a static type guarantee. Arbitrary Python objects, keyword arguments, callbacks, arrays, generators, and context managers do not fit the proposed boundary.

Correction: an explicit external declaration supplies parameter/result schemas. Veritas checks its caller statically and validates values at the runtime boundary. The Python implementation itself remains dynamically checked. Support positional and keyword calls and document unsupported object behavior. Include both a useful value-based package and a compiled-wheel compatibility challenge; count adapters, copying, and failures in the score. A successful pure-Python example cannot establish NumPy/Pandas compatibility.

### 6. The transport protocol could corrupt data or deadlock — high

Raw JSON leaves numeric precision, non-finite floats, bytes, map keys, and tuples underspecified. Python normally accepts/emits non-finite numbers in its JSON implementation, and JSON object keys are strings. Package code may write to stdout, so stdout cannot safely double as protocol transport. Unbounded messages and blocked child output can stall the runtime.

Correction: use a dedicated framed protocol channel and separate bounded stdout/stderr capture. Version the schema; encode arbitrary integers as tagged decimal strings and bytes with an explicit representation; require text map keys and explicitly handle non-finite floats. Validate framing, sizes, nesting, runtime types, timeouts, child death, and cancellation. Avoid silent coercion or generic stringification. [Python JSON documentation](https://docs.python.org/3/library/json.html).

### 7. Python and uv were hidden deployment dependencies — blocking

“One Veritas installation” was promised, but the plan required a pinned uv and `python3` without tasks to acquire, verify, locate, package, update, or relocate them. Copying the CLI and bytecode does not package Python or native shared libraries. Copying a developer virtual environment is not a clean deployment plan.

Correction: use managed, versioned toolchain assets with checksums and license records. Launch the selected interpreter by its recorded path; do not depend on ambient `python` or `python3` resolution. Preserve uv's own full lock rather than reconstructing a weaker resolver. Distinguish online initial setup from offline running of a completed package. Test pure Veritas and Python-assisted bundles separately, including a native wheel. Advertise only validated OS/architecture targets. uv documents managed interpreter selection and locked sync; these are building blocks, not proof of Veritas distribution. [uv Python management](https://docs.astral.sh/uv/concepts/python-versions/), [uv locking and syncing](https://docs.astral.sh/uv/concepts/projects/sync/).

### 8. Concurrency was promised but omitted — blocking

The design proposed structured tasks and checked cross-task values. The plan had no task scheduler, cancellation semantics, backpressure, timeouts, error aggregation, or tests distinguishing concurrent I/O from CPU parallelism. A browser worker is not a general concurrency implementation.

Correction: add bounded structured parallel work over immutable inputs, with isolated worker runtime state, deterministic result ordering, explicit failure propagation, and cancellation/join behavior. Separate CPU and I/O evidence. Define limitations for blocking native/Python operations. Python already has task groups, so the advantage must be measured ergonomics and outcomes, not the existence of the feature. [Python task groups](https://docs.python.org/3/library/asyncio-task.html#task-groups).

### 9. Host capabilities and the application did not match — high

On-disk plan Task 3 still listed no network capability and Task 4 no HTTP host operation, while Task 8 required `NetworkRead`. A previous intended edit had not been applied to the plan. A declared Python effect contract was also allowed to narrow an opaque effect even though no enforcement justified that narrowing.

Correction: define one catalog of host operations, signatures, effects, availability, and errors consumed by checker, runtime, docs, and inspection. Distinguish observed, inferred, and declared effects. Keep foreign calls opaque even when an author supplies annotations; those annotations do not grant a sandbox guarantee. Include network timeouts, status errors, bounded responses, and process exit reporting where supported.

### 10. “Held out” was not held out — blocking for recommendation claims

Feedparser appeared in the bridge implementation task before the feed reader was called a held-brief application. The same author selected the workload, built the language to support it, and planned to judge the result. Fresh folders do not create fresh model context. Neither the model sessions nor reliable review timing were provisioned.

Correction: label all known applications as development cases. Freeze compiler/docs before evaluation; obtain independently chosen tasks and follow-up changes without feeding them into implementation. If independent authoring resources are unavailable, deliver an evaluation kit and label author-run evidence exploratory. Do not invent sessions, elapsed times, or independence. No delegation is initiated by this review.

### 11. The 20% rule could manufacture a win — high

Improvement on “at least one” measure among many invites selective reporting. Three repetitions, even if specified, would not automatically establish a general advantage. Correctness, documentation, library coverage, deployment, and maintenance can dominate a small runtime or setup gain.

Correction: freeze one primary outcome and non-negotiable guardrails for each task class before evaluation. Report all failures, attempts, spread, and cold/warm costs. Treat 20% as an optional practical-effect target, not a statistical proof or Rob-approved threshold. Include cases where Python should remain the recommendation. Add an existing-language/tooling alternative check before positioning any advantage as unique.

### 12. The assistant-facing product was incomplete — high

Structured diagnostics alone do not teach a model a new language. The plan omitted a formatter, discoverable version-matched docs, public API/type lookup, executable documentation, source-level runtime stack traces, and compatibility/migration policy. These are central to generated-code repair and maintenance.

Correction: ship formatter/check mode, concise local language/reference docs, structured API lookup, stable diagnostic identifiers and source maps, test reports, version compatibility checks, and a documented update path. Test an actual follow-up change after a fresh-context handoff when available.

### 13. Clean workspace was confused with clean machine — high

Removing Rust from PATH in a temporary directory leaves caches, dynamic libraries, Python installations, and network access available. It cannot prove deployment onto another machine.

Correction: test relocation with developer paths and caches unavailable, then a genuinely fresh target environment where available. If no second environment is available, explicitly withhold the clean-machine claim. Byte-for-byte artifact reproducibility and equivalent behavior are separate claims; test the one advertised.

### 14. The plan could not pass its own early checks — implementation defect

Task 1 created a conformance test at workspace `tests/conformance.rs` but invoked it as a package integration test, and demanded parsed AST agreement before the parser existed in Task 2. `Value` ownership across bytecode and runtime crates was unresolved, risking a dependency cycle. The direct interpreter “test mode” was unspecified and would not constitute an independent implementation. Later tasks used undeclared grammar/types such as `test fn` and Null. No executable/test working directories were pinned.

Correction: grammar cases are written first and run when their implementation exists; put tests in their owning crates. Give value/schema definitions one dependency-neutral owner. Use one public compile-and-run path and independent behavioral oracles, property tests, and targeted corruption tests. Define grammar for every tool command's source constructs before using it. Run all documented commands from a named workspace root.

### 15. A complete rewrite was assumed to be cleaner — medium/high

The old prototype contains generic source spans, ASTs, parsing, and checking, alongside demo-specific adapters and host-toolchain coupling. Starting a new directory does not require discarding all working generic components.

Correction: make a reuse inventory. Copy only independently useful components that satisfy the revised semantic contract and general conformance tests. Exclude application-specialized code generation and dependency searches through development build directories. Keep copied history/provenance visible; a clean foundation is defined by its contracts, not its lack of ancestry.

### 16. Tooling improvements were being credited to a new language — high

The user's concrete `python`/`python3` friction can be solved by reliable launch tooling. If Veritas's gains come entirely from wrapping existing Python tooling, the comparison does not show that changing languages contributed. Similarly, the plan did not establish what a separate Veritas language offers beyond extending Planes. Planes remains its own general-purpose project; calling Veritas its successor was an unjustified inference.

Correction: compare equivalent complete workflows, including a Python entry command that manages its environment. Attribute wins separately to language checking, runtime behavior, tool integration, and documentation. Record why a separate language enables the proposed typed semantics and execution model without implying that Planes is superseded. Keep the user's language ambition intact while allowing the evidence to challenge the claimed advantage.

### 17. “One milestone” concealed release and effort limits — medium/high

This is a substantial compiler/runtime/toolchain program. Grouping it into one milestone does not make dependencies, native-library redistribution, migration, and independent evaluation disappear. No evidence supports a guaranteed one-session completion or frictionless public macOS distribution. Local unsigned relocation tests do not establish downloaded-app trust or public installation behavior.

Correction: execute one integrated milestone without repeated micro-approval gates, track unfinished work honestly, and distinguish a local release candidate from a publicly distributed signed/notarized release. Do not chase user signing credentials for this language validation. Public distribution remains a later operational requirement, and its friction cannot be reported as already solved.

## Future-forward-but-back assessment

| Future use | What must exist now | Corrected boundary |
|---|---|---|
| Assistants write and repair more code | Source diagnostics, formatter, local docs, API discovery, repeatable tests | These are evaluated through actual builds and changes; no assumed model preference |
| Programs use more external libraries/services | Explicit interop schemas, managed dependencies, bounded host operations | Foreign code remains opaque; compatibility is measured package by package |
| Programs live longer and run concurrently | Isolated state, lifecycle, structured cancellation, resource accounting | No universal replay system or automatic transactional rollback |
| Programs move between hosts | Versioned language/host/artifact boundaries and explicit target matrix | First validated host ships; browser support is a later portability decision |
| Performance matters more | Whole-workload CPU, startup, memory and tail measurements | Bytecode is a proposed first engine; native performance remains an open engineering obligation |
| Multiple agents maintain code over time | Version compatibility, migration notes, dependency/build records, reproducible handoffs | Stable public contracts; internal IR stays free to evolve |

The original goal remains credible as an ambition. The next build can earn a defensible initial recommendation for several real project classes; it cannot establish general default-language adoption. The corrected plan advances toward that goal by testing the whole lifecycle and identifying where Python still wins. Shipping a compiler alone would not accomplish it.
