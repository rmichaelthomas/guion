# Veritas Foundation Build Checkpoint v1.1

**Date:** 2026-10-22  
**Status:** Approved foundation build in progress. This checkpoint records the implementation continuation and is not a release or recommendation verdict.  
**Relationship:** Continues [Veritas inception checkpoint v1.0](../../../outputs/veritas_inception_checkpoint_v1_0.md), incorporates the approved [north-star build design v0.2](../../../outputs/veritas_north_star_build_design_v0_2.md) and [foundation implementation plan v0.2](../../../outputs/veritas_foundation_implementation_plan_v0_2.md), and records subsequent implementation evidence. The original inception checkpoint remains unchanged.  
**Build root:** `work/veritas-foundation/` inside the current Codex workspace.  
**Source preservation:** Planes, Liminate, Decan, NowLookHere, and the three uploaded Veritas originals are read-only inputs; this build has not modified them.

## 1. The aim

Build a general-purpose programming language and complete toolchain that a capable assistant would recommend to a developer over Python for suitable real work. The durable ambition is broad adoption. The near-term proof is narrower: show, across several real task classes, that creating, checking, changing, running, and delivering a Veritas project is materially better or more dependable than doing the same job in well-equipped Python.

The reason to build forward from the future is to anticipate how people and assistants will actually use code: the answer should arrive as a project that is understandable, testable, reproducible, repairable, and easy to run. We should build that experience into today's language and tools instead of promising it as a later layer.

“Recommended by an assistant” is a testable hypothesis, not a branding claim. A recommendation must include the task classes where evidence supports Veritas and the ones where Python remains preferable.

## 2. What Rob brought to the build

- Veritas began as a playful Rust experiment. Rust is a starting implementation choice, not an identity requirement.
- The supplied chronology and Python sketches are starter material to verify and grow, not a specification whose claims are presumed correct.
- The most concrete current friction is generated instructions that say `python` when the system requires `python3`. A Veritas entry command can remove that naming decision from the normal user path.
- A phone-deployment script in NowLookHere was offered as a possible workload. It remains an example, not the fixed benchmark or a reason to touch that repository.
- Rob has built Planes, Liminate, and Decan, wants their actual development history considered, and is willing to take a substantial swing. He has asked to avoid canned examples, hardcoded demonstrations, thin reports, and repeated approval gates for internal engineering steps.
- Planes was intended as general purpose and was exercised in Horizon, a browser game. Horizon makes repeated use, module graphs, interaction latency, and memory behavior relevant to Veritas acceptance work.

## 3. What prior language work contributes

These are observed engineering lessons from read-only repository inspection, not claims that Veritas should inherit those languages' semantics.

### Planes

The core-sufficiency report found that a one-file check missed a required `when` expression in the imported lexer. Horizon and rendering investigations used repeated workloads and before/after measurements. Veritas therefore needs complete module-graph tests, repeated and long-running use, and performance reports with tails and memory trends. One successful one-shot demo cannot establish language readiness.

### Liminate

The pipeline makes parsing and interpretation visible, with bounded syntax, canonical output, structured errors, and tests that serve as grammar artifacts. Veritas should reject unsupported syntax clearly, consume complete input, and treat diagnostics, documentation, and correction flow as core product behavior. Liminate's prose syntax and domain packs are not transferred.

### Decan

Decan reports a capability matrix, distinguishes exact support from partial support, reports losses at conversion boundaries, and uses several real consumer cases. Veritas should publish clear platform/interop limits and use varied consumer programs before making recommendation claims. Decan's temporal semantics and no-execution boundary are not transferred.

The fuller read-only findings are in [prior-language-build-lessons.md](prior-language-build-lessons.md), and prototype reuse decisions are in [reuse-inventory.md](reuse-inventory.md).

## 4. Competitive reality

The proposed combination is not empty space. Current tools already cover substantial parts of it:

- Python has optional free-threaded CPython builds starting with 3.13. They are not the default, and some extension modules can re-enable the GIL, so package compatibility remains part of the problem. [Python free-threading guide](https://docs.python.org/3/howto/free-threading-python.html)
- Mojo imports Python packages through CPython and is aimed at performance and accelerator work. Its own docs say Python-to-Mojo migration is incomplete, so it is a serious adjacent alternative rather than a finished universal replacement. [Mojo interoperability](https://docs.modular.com/stable/mojo/std/python/), [Mojo compatibility](https://docs.modular.com/stable/mojo/why-mojo)
- Nim already compiles through C and supports other backends; generated C is target-specific and Nim's package ecosystem is separate work. [Nim compiler guide](https://nim-lang.org/docs/nimc.html)
- Julia's compiler and package-image system reduce repeated load and compilation costs, but its own docs still identify time-to-first-execution and package-loading latency as concerns to measure. [Julia performance tips](https://docs.julialang.org/en/v1.12-dev/manual/performance-tips/), [Julia AOT compilation](https://docs.julialang.org/en/v1/devdocs/aot/)
- Go already provides an integrated toolchain and module workflow; Cargo provides Rust builds, dependency locks, and package distribution. “One command” and managed dependencies are table stakes, not unique differentiators. [Go toolchains](https://go.dev/doc/toolchain), [Go modules](https://go.dev/ref/mod), [Cargo overview](https://doc.rust-lang.org/stable/cargo/index.html), [Cargo dependency resolution](https://doc.rust-lang.org/stable/cargo/reference/resolver.html)

The viable opening is whether one coherent, assistant-friendly workflow can combine readable typed authoring, explicit failures, real parallel work, existing-library access, dependable project delivery, and clear repair paths with less friction than these alternatives. The build has not established that advantage yet.

## 5. Current design and implementation boundary

The approved build plan is one integrated foundation release candidate. Rust and versioned bytecode are current implementation choices because they provide a self-contained runtime path without requiring users to install Rust. The engine can change if comparative workloads justify it. Python interoperability is a typed supervised bridge, not transparent source compatibility; foreign calls remain explicit effects. No signing/notarization detour is required to evaluate the local build.

The source and package currently run on macOS ARM64. This is an evaluation target, not a portability claim. Python-assisted packages acquire their locked CPython and wheels on the target host; the current archive does not bundle a relocatable interpreter or support other operating systems.

The implemented language includes typed functions, inferred locals, control flow, lists, immutable `Map[Text, T]` values with deterministic iteration and update operations, arbitrary-precision integers, JSON and filesystem operations, `Result`, `Option`, bounded `parallel_map`, Python declarations, source diagnostics, versioned bytecode, and a command-line project lifecycle. See [language/spec.md](../language/spec.md), [reference.md](reference.md), and [implementation-plan.md](implementation-plan.md) for the exact executable contract and remaining acceptance criteria.

## 6. Verified evidence as of this checkpoint

- Workspace tests: **55 passed, 0 failed** after the Map and Python dictionary conversion implementations, including the real pinned uv integration previously verified with CPython 3.14.8, `requests`, and compiled `orjson` wheels.
- `cargo fmt --all -- --check`, Clippy with warnings denied, and the Python benchmark-harness tests passed.
- The known sales-report workload produced identical `12` and `1524` output in Veritas and Python. It is labelled a known development case, not independent evidence.
- A macOS ARM64 archive was checksum-verified and exercised from paths containing spaces and Unicode. It ran the sum, JSON, parallel, file-copy, Option, and managed Python examples. The copied file matched byte-for-byte.
- A version-10 artifact containing a Map literal round-trips through encode/decode and executes. Tests cover homogeneous typed map arguments, immutable replacement and insertion, sorted key iteration, membership, duplicate-key diagnostics, missing-key source traces, and list/map length.
- Typed Map arguments and results cross the Python sidecar as dictionaries with string keys. `builtins.dict` passed both a direct bridge round-trip and a compiled Veritas-source integration test.
- A host-target lock test failed before the target field was validated and passed afterward. The Option integration test failed before Option support and passed after compiler, verifier, bytecode, and VM changes. The first Map integration test failed at the lexer before implementation and passed after the compiler, verifier, artifact and VM support landed.
- The version-10 macOS ARM64 archive passed checksum verification and smoke tests with the bundled CLI: `doctor`, Map execution, build/run after removing the source file, and managed Python sync/run using `requests` and `orjson`.

These results establish a running foundation and current-host package workflow. They do not establish broad general-purpose completeness, independent developer preference, multi-host reliability, or a reason to recommend Veritas over Python in general.

## 7. Decisions and open boundaries

### WHAT IS LOCKED

- North star: earn assistant recommendations over Python for specific task classes through comparative evidence.
- Build general project behavior and reporting; do not replace it with canned output or a single tailored workload.
- Preserve the original language repositories, uploads, and NowLookHere.
- Do not require signing credentials to prove the current engineering hypothesis.
- Treat portability, package conversion, foreign effects, performance, and ecosystem compatibility as measured capabilities.
- Continue the approved integrated plan without approval prompts between routine engineering tasks.

### WHAT IS A CURRENT BUILD CHOICE

- Rust-hosted VM and versioned bytecode.
- Indentation-based typed source with local inference.
- `uv` and a managed CPython sidecar for declared Python calls.
- A macOS ARM64 evaluation archive.

These choices remain revisable when a real task shows a material cost.

### WHAT IS NOT ESTABLISHED

- Multi-file module imports, records, floating-point arithmetic, `Result`/`Option` matching, HTTP, bytes, empty-map construction, and broad resource accounting.
- Cross-host packaging, relocatable Python-assisted apps, full Python object/array compatibility, editor/LSP support, or public distribution readiness.
- Real independent task selection, unseen follow-up editing, comparative maintenance costs, and assistant preference.
- A unique speed, concurrency, or deployment advantage. Current timing is one machine and one known workload.

## 8. Next build push

Continue the integrated implementation plan in this order of dependency, without turning each item into a separate approval checkpoint:

1. Finish the language foundation needed for ordinary programs: module graph and source mapping, records, `Float64` and exact integer division, and general `Option`/`Result` matching. Map literals, immutable replacement, key membership, sorted key iteration, lookup, and typed Python dictionary conversion now have executable coverage; empty-map construction remains open.
2. Complete host operations and resource behavior, including bounded HTTP and truthful cancellation/partial-effect reporting.
3. Complete project lifecycle and packaging: update recovery, source/resource locks, clean-cache/offline behavior, and target-aware packages.
4. Build several unhandpicked consumer programs, including a persistent application and changed requirements, then run long-session and resource stress.
5. Freeze the candidate and run the pre-registered comparison against appropriately equipped Python and the closest existing-language alternatives. Preserve adverse cases and only recommend Veritas where the evidence earns it.

This ordering is a working route, not a permission gate. If a measured implementation result changes the architecture or the north-star hypothesis, update the checkpoint with the evidence and cost.

## 9. Resume prompt

Resume the approved Veritas build in `work/veritas-foundation/` from this checkpoint, the detailed [implementation plan](implementation-plan.md), and its progress ledger at `.superpowers/sdd/implementation-plan/progress.md`. Preserve all original language repositories, uploaded files, and NowLookHere. Continue toward the full integrated foundation candidate. The latest verified code state is a 55-test workspace with managed uv/CPython/`requests`/`orjson`, `Option[T]`, immutable Maps with typed Python dictionary conversion, target-checked Python lock metadata, and a tested version-10 macOS ARM64 package. The current release is not yet a general-purpose recommendation candidate: modules, records, floats, matching, HTTP, full resource accounting, multi-target packaging, and independent evaluations remain. Do not claim recommendation or finish the goal until those acceptance gaps are addressed and the evidence report is complete.
