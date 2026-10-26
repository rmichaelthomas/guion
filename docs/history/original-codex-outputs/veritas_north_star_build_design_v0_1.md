# Veritas — North Star Build Design v0.1

**Date:** October 22, 2026  
**Status:** Superseded by v0.2 after pre-build audit; historical proposal  
**Working name:** Veritas  
**Build scope:** Establish the first general-purpose Veritas toolchain and recommendation-worthy project experience.

## The north star

Veritas should become a language an informed software assistant recommends when a builder asks it to make a real program, because the complete experience makes the program easier to get right, change, check, and deliver than the best practical Python route for that job.

That is a product claim to earn with evidence. It is not a claim that Python is obsolete, that every task needs a new language, or that Veritas should win on raw speed alone.

The time horizon is **future-forward, built backward**: imagine programs increasingly written and maintained by people and software agents, then build the present-day interfaces, semantics, and evidence that make those programs understandable and dependable. Future capabilities must grow from stable boundaries now; they should not be bolted on as ad hoc features later.

## Who it serves and what they should feel

The first users are builders working with an assistant on small-to-medium real software: command-line tools, data processing, automation, and network-backed services. They should be able to ask for a project, receive ordinary readable Veritas source, run the same simple Veritas commands on a fresh machine, understand precise errors, and ship a reproducible result without manually coordinating a compiler language, a scripting language, and a collection of tools.

The assistant is a first-class user of the toolchain. Source documentation, compiler results, project state, tests, dependencies, capabilities, and failures must have stable machine-readable forms as well as clear human output. The language must remain understandable and reviewable by the person responsible for the result.

## Why this is a real opening

The initial diagnosis cannot be “Python has no concurrency or reproducible tooling.” CPython has had optional free-threaded builds since 3.13; unsupported extension modules may turn the GIL back on. Python also has established async and executor APIs. Current `uv` workflows resolve and lock dependencies, synchronize environments, and run projects against locked versions. These narrow individual criticisms are therefore weak foundations for a new language.

The opening is the **joined experience**: readable source, useful static guarantees, predictable concurrency, a single coherent build/test/package workflow, accessible existing libraries, and evidence an assistant can inspect. Veritas must make those pieces fit together with less total friction on an identified class of project. [CPython free-threading documentation](https://docs.python.org/3/howto/free-threading-python.html), [Python concurrency executors](https://docs.python.org/3/library/concurrent.futures.html), [uv project guide](https://docs.astral.sh/uv/guides/projects/), [uv lock and sync](https://docs.astral.sh/uv/concepts/projects/sync/).

Existing languages already cover many ingredients. Mojo targets Python-oriented high-performance and AI workloads; Rust offers native performance, strong typing, and a mature integrated toolchain. Veritas is not differentiated by combining those nouns. It has to demonstrate a more useful default workflow for assistant-built applications.

## What the user's existing language work contributes

Planes was also intended as a general-purpose programming language, and Rob tried to build with it before the current Veritas effort, including putting it into Horizon, a browser-based game. Whether Planes reached general-purpose readiness is open to evaluation; the attempt and the problems found are direct evidence about what a successor needs to handle. The inspected local projects suggest methods and product instincts, not syntax or architecture to copy:

- **Planes** shows both the ambition and the pressure test: a general-purpose language needs real applications, and its semantics can be checked through multiple implementations, cross-implementation agreement, inspectable behavior, and machine-readable output. Horizon is a meaningful consumer, not a decorative example.
- **Liminate and Liminate-dev** show how a deliberately bounded language can be built from concrete example sentences and grow into runnable tools and products. Its examples and test suite are extensive, but Veritas must be a general-purpose programming language rather than another prose DSL.
- **Decan** shows how to state semantics, publish supported subsets, make conversions fail honestly when meaning would be lost, and validate against real consumer cases.

Veritas should inherit the evidence discipline: define behavior before implementation, make examples executable, separate supported behavior from aspiration, and test real consumer work. It should not inherit a hard-coded demo as its product definition.

### What Horizon adds to the design

The local Horizon evidence is especially useful because it moves beyond one-shot scripts. The project runs a persistent simulation in a browser worker, takes inputs through a message boundary, advances a fixed-step world, saves and restores state, and checks deterministic replay across Python and JavaScript implementations. Its reports document real integration issues found while building: the one-pass program calling convention did not fit a persistent runtime loader; the runtime exposed no general output-drain channel; a persistence implementation was tied to one subject shape and Node; packaging omitted nested assets; and long-session garbage collection created severe tail pauses even when typical steps were fast. Several were repaired and re-tested in the Horizon build; others remain explicit limitations.

Those findings add concrete requirements for Veritas: make program loading versus persistent execution explicit; design a host interface for events, outputs, snapshots, and replay; test deterministic behavior across fresh runs; and measure long-session tail latency and memory growth. Horizon should be a **second, distinct acceptance workload** in the first complete proof. Port the reusable simulation/kernel slice into a browser-worker host and compare it with its existing Python/JavaScript behavior. Keep the separate new-project task as the main assistant-recommendation comparison, so the language is evaluated both as a builder tool and as a persistent embedded runtime. Do not make the whole Horizon game the language definition.

## Product promise for the first build

Given a new project brief, an assistant and builder should be able to use one Veritas installation to:

1. create a project with a clear manifest and source layout;
2. resolve and record dependencies reproducibly;
3. check source and report actionable errors before running;
4. run the project's tests and program;
5. build and package a result that can be handed to another machine;
6. inspect dependencies, effects, target requirements, and build outcomes in structured form.

The user's routine invocation should never require Rust commands or an unqualified `python` command. Rust is a compiler implementation choice. Python compatibility is an explicit interoperability boundary, not a claim that Veritas source can import every Python package with no adaptation.

## Technical direction proposed for approval

### Compiler and execution model

Implement the compiler and primary CLI in Rust. Keep parsing, name resolution, type checking, capability analysis, and lowering defined against a stable, backend-neutral typed intermediate representation (IR). Do not keep adding language features directly to example-specific code generation.

The current prototype's Rust-emission backend is evidence that a compiler slice is possible, but it means users need Rust installed and does not define Veritas's long-term runtime. The first production-shaped build should replace that dependency with a Veritas-owned execution and build path packaged by the Veritas installer. Its backend must be replaceable without changing the source language or CLI contract.

**Recommended first execution path:** compile to a versioned Veritas bytecode format and run it with a runtime shipped inside the Veritas distribution. Use the same typed IR for test and run. This gets a real compiler and a portable artifact moving without prematurely maintaining a custom native optimizer. The first milestone must not claim native-speed performance. A native backend becomes the next expansion once the workload corpus shows which operations merit it.

### Language shape

Use a conventional, readable general-purpose syntax with inferred local types and explicit types at public boundaries. Preserve familiar control flow and data structures. Make errors explicit in function signatures and behavior. Modules and packages are ordinary project concepts, not one-file demonstration conveniences.

Concurrency is a language-level design requirement, but the first deliverable should establish its safety model before exposing unrestricted shared mutation. The proposed direction is structured tasks with explicit task groups and compiler-checked values crossing task boundaries. Avoid silently treating asynchronous I/O as CPU parallelism.

The precise grammar and semantic rules remain to be written as a conformance spec before code. The uploaded syntax sketches are reference material only.

### Python ecosystem access

Treat library access as a required strategic pillar and a major engineering risk. Start with a typed, explicit boundary to CPython: declared Python dependencies, named imported modules/functions, documented value conversions, visible runtime requirements, and clear diagnostics when a package or value cannot cross the boundary. Prefer preserving Python packages where they provide meaningful value over rewriting those libraries in Veritas.

Do not promise arbitrary Python object transparency or a self-contained executable containing every Python package in the first milestone. Measure bridge cost, compatibility, deployment size, and failure modes. This is a go/no-go concern for a broad assistant recommendation and must be tested with useful packages, not only a toy function.

### Integrated toolchain

Ship a single user-facing `veritas` command with consistent `new`, `check`, `test`, `run`, `build`, `package`, and `inspect` commands. Use a project manifest and committed lockfile. Diagnostics should carry stable codes, source locations, plain-language explanations, and machine-readable JSON. Builds should record the compiler version, target, dependency identities, and capabilities used.

The first package format may include the Veritas runtime when needed. “One command” means one coherent product surface; it does not mean every application must fit into a universally portable one-file binary.

### Future-facing boundaries

Build capability and dependency information into the typed IR and artifact metadata now. The first capability set is file read/write, network read, console output, clock access, and Python calls. Treat a Python call as an opaque external effect unless its declared contract narrows it. That enables later tooling to explain file/network/process access, compare builds, run targets with different permissions, and give assistants reliable project context. Keep these reports descriptive until the runtime or operating-system boundary can enforce them. Do not label analysis as security enforcement.

Portable targets such as WebAssembly are future candidates, not a goal to claim from a bytecode demo. Target-specific behavior must be declared and unsupported conversions must fail visibly.

## First build: one broad vertical slice

Build a clean Veritas project workflow around a **new, general-purpose program from a written brief**, selected for meaningful file/data work and at least one real library dependency. The workload is chosen from the user's actual needs during corpus design, but it is not fixed to NowLookHere, invoices, a phone, or any previous demo. Keep existing prototypes as historical experiments and regression material; the new foundation should have its own coherent project layout and source of truth.

The slice must exercise the entire path: create project, resolve dependency, edit source, receive a compiler diagnostic from a deliberate mistake, run meaningful tests, execute, build/package, and open the result in a clean environment. It must also include one nontrivial follow-up change to reveal whether the workflow holds up after the first success. In the Horizon workload, exercise persistent worker operation, typed input events, output collection, snapshot/save/restore, deterministic replay, and long-session latency/memory reporting. Compare scene-intent/state results with existing implementations, and record browser rendering as a host integration result rather than a language benchmark.

This is not a benchmark-only script and not a sequence of isolated syntax features. It is the smallest complete product experience capable of exposing architecture failures across language, libraries, tooling, and delivery.

## Evidence gate: what earns an assistant recommendation

Once the slice works, compare Veritas against a strong current Python route, including current project tooling and a fair library setup. Use fresh, independently run project briefs and at least one substantial follow-up change. Include tasks not used to build the compiler. Record:

- correct result and regressions;
- setup time and human corrections;
- time to first successful run and time to a reviewed deliverable;
- dependency/library coverage and interoperability friction;
- compiler/runtime diagnostic usefulness;
- tests and review effort;
- build reproducibility, package size, clean-machine startup, and execution speed.

Pre-register thresholds before the comparison. A recommendation can be scoped: for example, Veritas is recommended for a defined type of CLI/data project under stated conditions. Do not turn success on one selected example into a general “better than Python” claim.

## Non-goals for this first build

- Replacing Python across all ecosystems or promising compatibility with every package.
- A novel syntax as the differentiator.
- A custom JIT/native optimizer before the language and workload contracts stabilize.
- A full package registry, global package-management service, or universal one-file packaging claim.
- Phone signing, device deployment, NowLookHere changes, or reproducing one prior demo as the acceptance case.
- Security guarantees based only on static effect reports.

## Risks and decision gates

1. **Python bridge:** if valuable Python libraries require fragile wrappers or make distribution worse than Python, the north-star claim fails for broad use. Measure early.
2. **Language/runtime scope:** a robust package and runtime model is substantial. Keep the language surface disciplined and artifact formats versioned.
3. **Performance:** a bytecode runtime may not meet the eventual speed goal. Do not claim it does. Gather representative profiles before choosing native code generation or JIT work.
4. **Assistant advantage:** structured output only matters if it reduces errors or review time. Measure actual assistant-built changes.
5. **Adoption:** a technically superior workflow may still lose to Python's existing ecosystem. Start with a defensible project class and grow from evidence.

## Approval and next move

If this design is accepted, the next build step is one implementation plan for the complete vertical slice, followed by a clean Veritas working copy. The user-facing CLI, project format, bytecode, test runner, package output, and Python bridge are designed as one joined workflow; the end-of-slice evaluation is prepared alongside implementation, not retrofitted after a successful demo.

## Sources checked for current language/tooling claims

- [Python free-threading support](https://docs.python.org/3/howto/free-threading-python.html)
- [Python concurrent execution](https://docs.python.org/3/library/concurrent.futures.html)
- [uv projects and lockfiles](https://docs.astral.sh/uv/guides/projects/)
- [uv locking and syncing](https://docs.astral.sh/uv/concepts/projects/sync/)
- [Mojo manual](https://docs.modular.com/mojo/manual/)
- [Mojo calling Python](https://docs.modular.com/mojo/manual/python/mojo-from-python/)

## Related evidence

- [Veritas inception checkpoint v1.0](veritas_inception_checkpoint_v1_0.md)
- [Prototype test results v0.1](veritas_prototype_test_results_v0_1.md)
- [Future-use test results v0.1](veritas_future_use_test_results_v0_1.md)
- [Phone deployment vertical proof results v0.1](veritas_phone_deploy_vertical_proof_results_v0_1.md)
