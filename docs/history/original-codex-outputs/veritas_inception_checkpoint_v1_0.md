# Veritas
## Inception checkpoint v1.0 — a testable direction for a Python-scale language ambition

**Date:** October 22, 2026  
**Status:** INCEPTION CHECKPOINT — territory and evidence recorded; product and language design remain open  
**Domain prefix:** `veritas` (provisional, pre-vault)  
**Purpose:** Combine the initial Veritas concept and starter files, independent verification, the current language landscape, and evidence from Planes, Liminate, Liminate-dev, and Decan. This is a foundation for the next design milestone, not a build specification.

## How to read this checkpoint

- **Verified:** observed in the supplied files, inspected repositories, or linked primary sources.
- **Inferred:** a conclusion drawn from evidence, open to revision.
- **Aligned direction:** the broad approach Rob accepted, without pretending every design choice is settled.
- **Open:** a question needing a decision or test corpus before implementation.

The name **Veritas** was suggested by someone else. Rob does not dislike it, but has not selected it. This document uses it as a working label only.

There is no earlier Veritas checkpoint chain, established section numbering, or verified historical chronology. This is a new inception record. The supplied chronology is a claim to investigate, not primary history.

---

## Part I — The ambition and the actual question

### 1. The ambition

Rob's starting point is intentionally large: make a language that could, over time, replace Python as the default choice for ordinary programming. The motivation is not simply to make a faster interpreter. It is to keep the directness and approachability people value in Python while reducing execution cost, deployment friction, concurrency constraints, and the difficulty of moving between application code and native extensions.

That is an ambition, not a launch claim. Python's libraries, teaching material, tools, installed base, and community habits are part of the problem. A language can be technically elegant and still fail to displace a useful incumbent.

### 2. The question beneath the ambition

The design question is not “What is Python's definitive list of flaws?” There is no single unanimous community autopsy, and Gemini's initial answer overstated both consensus and novelty. The productive question is:

> What specific everyday programs should feel materially easier, safer, faster to ship, or easier to trust in this language—and what evidence would prove that advantage?

A replacement-scale ambition needs a small first proof. Veritas should first become more compelling than Python for a defined class of real work, then earn the right to broaden.

### 3. Current alignment

Rob said “Agreed” to the previous working direction: explore a general-purpose language with Python-level approachability, stronger type guarantees, native execution, an integrated toolchain, and a practical path to Python libraries. Treat this as **provisional alignment on the direction**, not a lock on syntax, type rules, memory management, compiler architecture, compatibility, or the first audience.


### 3a. Goal clarification — earning the recommendation

Rob clarified after this checkpoint's first draft: the intended goal is for Veritas to be what an informed assistant or software developer would recommend over Python when a builder brings a real project. He wants to reach that point as quickly as evidence permits. Rob clarified that “future-forward-but-back” means anticipating how the language could be used in the future and deciding how to build for those uses now. It is a future-use design lens, broader than working backward from a finished present-day builder experience. No fixed long-term schedule is accepted or implied.

This sharpens the near-term objective: establish a defensible recommendation for meaningful real projects. Broad default-language adoption remains a separate, larger outcome. The presence of Codon, Mojo, or other prior art establishes competition; it does not establish that the desired builder experience has already been delivered.

An initial recommendation should account for the whole job: getting started, accessing needed libraries, implementing and changing behavior, diagnosing errors, testing, deploying, and maintaining the result. Runtime speed alone is insufficient. For assistant-assisted development, discoverable authoritative documentation, runnable examples, stable interfaces, and actionable structured diagnostics are also product requirements to investigate. These can improve an assistant's ability to use a new language without waiting for a future model to have encountered it in training; their actual effectiveness must be measured.

Recommended evidence: select a small set of real project briefs from Rob's work, establish a competent Python baseline with current tooling, and implement equivalent outcomes in Veritas. Record setup effort, total time to a correct deployed result, human interventions, dependency gaps, diagnostic usefulness, and the cost of a subsequent change. Give both routes their best supported tooling and repeat with independent builders or agents where practical. The exact projects, thresholds, and study design remain open.

A first win on this corpus supports a scoped recommendation; it is not proof of general superiority. The corpus is the first test of the broad ambition, not a permanent restriction on the language's purpose.

---

## Part II — What the starter pack proves

### 4. Supplied artifacts

The referenced “Verify Rust Ideas” conversation supplied three files: `veritas.py`, `veritas_ultimate.py`, and `veritas_concept_to_build_chronology.md`. The Python files were copied into an isolated temporary directory for execution. The uploaded originals were not modified.

### 5. Verified findings

**`veritas.py` is not currently executable.** It has a syntax error in `def peek(() -> Token:` at line 113. Other parser method signatures are malformed: `consume` lacks `self`, and `parse(list)` is written as a method while its body uses `self`. The C generator also has a `display values` path referring to an undeclared `values_val`. The syntax failure prevents reaching those later problems.

**`veritas_ultimate.py` runs its built-in example, but it is not the VM described in the chronology.** Its demo returns 40. `VeritasVM.execute` walks AST statements using a dictionary-backed environment; it does not compile to opcodes or execute a stack machine. The demo prints 30 tokens; the chronology says 29.

**The generated C file is a narrow, hard-coded demonstration.** It compiles as a dynamic library, and generic exported C functions can be called through `ctypes`. The exported calculation agrees with the demo for tested arguments. But changing the Veritas source filter from `above 5` to `above 2` changes the Python evaluator's answer from 40 to 52 while the C export still returns 40. The C output is not generated from the source AST. Linking it as a standalone executable fails because it has no `main`. There is no Python extension module, Python-object marshaling, or demonstrated source-to-native compilation pipeline.

**The chronology is not verified history.** It describes an AST-to-opcode pass, stack VM, recursive parser, and validation suite that the supplied code does not contain. Without dated source notes, commits, or matching artifacts, it should be treated as a proposed narrative rather than a reliable chronology.

### 6. What is salvageable

The starter pack is useful as a sketch of the desired territory: readable source, parsing, execution, and a native boundary. It does not establish a sound architecture or validated semantics. The cleanest route is to preserve interesting examples and intent, then write a small explicit specification and rebuild a coherent vertical slice. Fixing syntax alone would leave the semantic and pipeline gaps untouched.

---

## Part III — What the previous language projects teach us

The repository review was used to learn from Rob's working method, not to assume Veritas should become a combination of those languages. The projects have different aims and levels of maturity.

Repository anchors checked for this checkpoint (short commit IDs are GitHub-resolvable):

| Repository | Inspected revision | Why it matters here |
|---|---|---|
| [Planes](https://github.com/rmichaelthomas/planes/tree/de984c9) | remote HEAD `de984c9` (Oct. 22, 2026); local language checkout `0fab92a` is older | README, corpus, multi-implementation agreement; newer remote Rust experiment noted |
| [Liminate](https://github.com/rmichaelthomas/liminate/tree/263e9b9) | remote HEAD `263e9b9` (Oct. 19, 2026) | language examples and conformance tests |
| [Liminate-dev](https://github.com/rmichaelthomas/liminate-dev/tree/ec54905) | remote HEAD `ec54905` (Sep. 23, 2026) | monorepo product tests and case studies |
| [Decan](https://github.com/rmichaelthomas/decan/tree/3a5285d) | remote HEAD `3a5285d` (Oct. 16, 2026); inspected local checkout `7733c25` is older | boundary and conformance model; local README/spec only, with stale-checkout limitation recorded |

The separate local Planes Rust experiment checkout was at `bbf4195` (Oct. 22, 2026); it was used as evidence of a bounded parity-before-performance experiment, not as evidence about Veritas architecture.

### 7. Planes: agreement and workload evidence

The Planes README presents a general-purpose language organized around two questions: what effects a program can have without running it, and where a value came from. Its evidence practice is concrete: a grammar and machine-readable vocabulary; a 51-program corpus; Python, JavaScript, and self-hosted Planes implementations; byte-for-byte comparisons across 349 value shapes; and ordinary workload benchmarks with method and limitations documented.

The lesson for Veritas is **make claims falsifiable across implementations and realistic programs**. “The languages agree” is measured against declared outputs and error details, not asserted in prose. The README also publishes performance results showing its current tree-walking interpreter substantially slower than Python on several workloads. Correctness and language identity do not demonstrate speed.

Planes is not a template to copy. Its goals—effect surfaces, derivation/provenance, exact rationals, and an intentionally closed host interface—are specific design choices. Veritas should inherit one only if its own users and workloads need it.

### 8. Liminate and Liminate-dev: grow from a corpus and users

The Liminate repository has a progression of dogfood examples with paired actual/expected outputs, covering basic syntax, compositions, chaining, compound conditions, iterators, errors, parameters, branching, event-driven use, and realistic cases such as games, healthcare, and smart homes. Its suite covers lexer/parser/interpreter behavior, conformance and agreement corpora, fuzzing, grammar generation, adapters, packs, and end-to-end integration.

The Liminate-dev monorepo carries the language into product surfaces and case studies; its tests cover API behavior, data integrity, agreements, routing, security boundaries, packs, and product features. The development rhythm is: make the language useful in its own tools, record expected behavior, then expand against concrete user or product pressure.

Liminate began under the Inscript name with a human-first/prose-as-syntax thesis. That is a useful precedent for taking syntax seriously as user experience, but not evidence that prose-like syntax is right for general-purpose programming. Liminate is a domain-oriented language; Veritas must make everyday software legible without making ordinary code ambiguous.

### 9. Decan: define the boundary before promising behavior

Decan's current local checkout is clean but old (local `main` at `7733c25`, August 27); the remote repository had later work through October 16, so that checkout is not a complete view of today's repo. Its README and specification make the core boundary clear: Decan represents temporal intent and evidence; it is not a scheduler or authority to execute work. Adapters classify exact, lossy, or unsupported conversions instead of silently dropping meaning. Its tests are organized around explicit conformance stages, consumer evidence, and public interfaces.

The lesson is **make promises and non-goals explicit**, and make unsupported cases visible. The stale checkout is also a reminder to pin repository revisions when a future checkpoint makes detailed implementation claims.

### 10. Carried-forward method, not language design

These projects support the following practices for Veritas:

1. Start from a real purpose and representative programs.
2. Treat examples and expected behavior as a first-class corpus.
3. Test failures and boundaries, not just happy paths.
4. Use independent implementations or backends to expose hidden assumptions when warranted.
5. Compare semantic boundaries, errors, and serialization—not only final numeric output.
6. Measure ordinary workloads and report method, environment, and limitations.
7. Keep user-facing promises narrower than implementation ambition; make unsupported behavior explicit.
8. Preserve a decision ledger so experiments do not accidentally become language law.

---

## Part IV — Feasibility and prior art

### 11. Is it feasible?

**A new, useful language is feasible. A broadly adopted Python replacement is a long-range, high-risk outcome.** The hard work is not writing a lexer or emitting C. It is choosing semantics that remain coherent as the language gains modules, errors, libraries, concurrency, tooling, and platform support—and making the ecosystem worth adopting.

A credible sequence is to prove a small useful language first, then native compilation, then distribution and Python interoperability. Each step should preserve the behavioral guarantees of the previous step.

### 12. What is already done in the language landscape?

The broad ingredients are not novel in isolation:

| Desired property | Existing evidence | Veritas question |
|---|---|---|
| Python-like syntax with compiled execution | Codon is a Python-like compiler aimed at native performance and parallel execution. | Does Veritas have a clear advantage in ergonomics, compatibility, safety, tools, or workload fit? |
| Python libraries alongside compiled code | Mojo uses CPython for Python interop and targets high-performance CPU/GPU work. | Can Veritas offer a practical general-purpose path without runtime/deployment costs that undermine its goal? |
| Readable syntax and native compilation | Nim has long combined concise syntax with native compilation. | What would Veritas add beyond a new syntax and compiler? |
| Integrated dependency workflows | Python has mature packaging standards; tools such as uv integrate locking, environments, running, and building. | Which friction remains after using today's tools, and should Veritas solve it differently? |
| Parallel execution without the default GIL | CPython has optional free-threaded builds since 3.13; extension compatibility and overhead remain practical concerns. | What concurrency guarantees should Veritas make, and how will Python interop affect them? |
| Readable or prose-like surfaces | Liminate and other historical languages demonstrate that natural-language-inspired syntax is explored territory. | Is that surface clear, learnable, and maintainable for general-purpose code? |

The strongest direct comparisons are **Codon** and **Mojo**, with different goals and compatibility tradeoffs. Nim, Julia, PyPy, Cython, and Rust-based Python extensions are additional prior art. Gemini's original answer was directionally right that earlier projects faced adoption and ecosystem challenges, but flattened very different projects into a simplistic list of failures. It did not establish why those projects failed; those claims should not be repeated as fact without evidence.

Primary sources checked:

- [Codon documentation](https://docs.exaloop.io/)
- [Mojo: why Mojo](https://docs.modular.com/stable/mojo/why-mojo) and [Mojo/Python interoperability](https://docs.modular.com/mojo/std/python/)
- [Nim language](https://nim-lang.org/)
- [Python free-threading documentation](https://docs.python.org/3/howto/free-threading-python.html)
- [uv project locking and syncing](https://docs.astral.sh/uv/concepts/projects/sync/) and [building distributions](https://docs.astral.sh/uv/concepts/projects/build/)
- [Python Stable ABI](https://docs.python.org/3/c-api/stable.html)
- [PyO3 building and distribution](https://pyo3.rs/main/building-and-distribution.html)

These sources establish prior art and current constraints. They do not establish market demand or predict Veritas adoption.

### 13. What could be distinctive?

“Python, but faster,” “statically typed Python,” “a single binary toolchain,” “easy C interop,” or “friendly syntax” are not sufficient novelty claims. Each has close precedents or can be approximated by today's tools.

A defensible distinction would need to emerge from a combination users can feel in real work. Candidate axes to investigate include:

- unusually clear diagnostics and a low-friction learning curve;
- explicit, dependable behavior across run, build, and deployment;
- a type system that catches meaningful errors without making ordinary code ceremonious;
- safe parallelism with transparent costs;
- Python interoperability that makes incremental adoption practical;
- a standard-tool workflow that reduces, rather than renames, setup burden.

These are **candidate design axes**, not claims of uniqueness or locked requirements. A corpus of real tasks should decide which matters.

---

## Part V — Candidate thesis and technical posture

### Future-use design lens — clarified by Rob

Explore plausible future uses of programming, then ask which foundations Veritas should establish now to support them. Future scenarios are hypotheses to investigate, not predictions or automatically accepted requirements.

Candidate scenarios include sustained collaboration between humans and coding agents; programs running across local devices, servers, and heterogeneous hardware; and software that must remain inspectable as automated creation and modification increase. These examples come from the analytical partner and remain open for exploration.

For each scenario, record the likely user need, the language or tooling consequence, what would be costly to retrofit, what can remain an extension, and a small present-day experiment that could test the idea. Prioritize foundations that help current users and preserve future options. Avoid adding complexity solely because a speculative use might emerge.

### 14. Candidate product thesis — inferred, not locked

> Veritas could be a general-purpose language for people who value Python's directness but want stronger guarantees, dependable native execution, and a clear route through existing Python code.

This describes a value proposition to test, not a promise that Veritas has already solved performance, safety, portability, or compatibility.

### 15. Candidate technical posture

The current evidence supports this investigation order:

1. **Specify semantics before choosing a compiler.** Write a small grammar, value model, type rules, error behavior, and sample corpus.
2. **Build a transparent reference interpreter.** Use it to test what programs mean, not as a permanent performance answer.
3. **Choose one native route against acceptance criteria.** A small C backend or Rust-hosted compiler/runtime can prove a vertical slice; neither is yet justified as the final architecture.
4. **Add Python interop as an explicit boundary.** Measure imports, packaging, value conversion, exceptions, object lifetime, and GIL/free-threaded behavior. “Can call C” is not equivalent to “can use Python packages.”
5. **Design packaging around reproducibility and deployment evidence.** A single tool is useful only if lockfiles, builds, targets, native libraries, and upgrades behave predictably.
6. **Measure performance after parity.** Compare equivalent tasks, algorithms, inputs, and outputs, including startup and packaging costs.

### 16. Is Rust justified?

**Possibly for the compiler/runtime implementation; not because the language must be written in Rust.** Rust is a strong candidate for compiler, package-manager, and runtime components that need native performance and explicit memory/concurrency control. It brings complexity and does not by itself deliver Python-like ergonomics, package compatibility, fast startup, or an accessible language implementation.

The Planes Rust experiment is an instructive pattern: a bounded workload spike with correctness/parity gating before performance measurement. Veritas can use the same discipline. First establish program semantics and a parity suite in a simpler reference implementation; then compare Rust against another host only where the tradeoff matters. Do not let a host-language preference silently settle Veritas's user-facing semantics.

---

## Part VI — What is and is not decided

### 17. Aligned or established

- **User ambition:** pursue a language that could eventually replace Python as a general-purpose default.
- **User posture:** the ambition is a reach, and Rob is willing to pursue it.
- **Portfolio context:** Planes, Liminate/Liminate-dev, and Decan are relevant evidence about his language-making practice and should inform the work before implementation.
- **Working direction accepted in conversation:** explore general-purpose use, Python approachability, stronger typing, native execution, integrated tooling, and a practical Python path.
- **Starter-pack status:** concept material, not a working compiler/runtime; actual limitations are recorded above.

### 18. Not decided

- Whether the name is Veritas.
- The first user and first programs.
- The precise problem whose solution would make a user choose Veritas over Python, Rust, Go, or an existing Python compiler.
- Whether syntax is Python-like, prose-inspired, or a new compact surface.
- Static, gradual, or mixed typing; inference scope; runtime checks.
- Memory management, resource lifetime, exceptions/results, and object model.
- Concurrency model and guarantees.
- Interpreter/JIT/AOT strategy, intermediate representation, and backend.
- Python compatibility level and which direction interop should prioritize.
- Package format, dependency resolver, build story, supported targets, and binary distribution.
- Whether a standard library or compatibility layer is required in the first release.

---

## Part VII — Next milestone: choose the first proof

The next milestone should be a **design and acceptance-corpus checkpoint**, before a larger compiler build. It should produce:

1. A short statement of the first user's concrete problem.
2. A small corpus of ordinary programs that user would actually write.
3. Candidate syntax for those programs, compared for readability and precision.
4. A minimum semantic contract: values, functions, collections, errors, modules, and side effects required by the corpus.
5. Measurable acceptance conditions for run/build parity, diagnostics, native output, packaging, and Python interop (if it is in the first proof).
6. A fair comparison against Python plus uv, one directly comparable language/compiler, and any relevant extension approach.
7. An explicit decision on what to prototype and what evidence would make us stop, revise, or continue.

A useful first proof is likely a small command-line tool with file I/O, structured data, functions, collections, errors, and modules. That is an **inferred candidate**, not a locked scope. It would test ordinary programming more effectively than a calculator demo, while avoiding the requirement to solve the entire Python ecosystem at once.

Do not claim a VM, native compiler, Python compatibility, parallel execution, or one-binary ecosystem until an end-to-end test proves the particular claim.

---

## Lockbox

### What is locked

- This is a new inception record with no prior Veritas version chain.
- Describe the starter files and chronology according to verified behavior; unsupported architecture claims are not carried forward as fact.
- Prior repositories inform method and provide evidence; their domain-specific semantics do not automatically transfer to Veritas.

### What is provisionally aligned

- Explore a general-purpose language with Python approachability, stronger types, native execution, integrated tooling, and practical Python interoperability.
- Pursue recommendation over Python as soon as evidence supports it; prove that recommendation on real projects while retaining the broader general-purpose ambition.

### What is open

All product, audience, syntax, type, runtime, compiler, packaging, concurrency, interop, and naming choices listed in Part VI.

### What is logged

- The uploaded `veritas.py` has syntax and semantic/code-generation problems.
- `veritas_ultimate.py` runs its demo but is an AST walker, not the claimed stack VM.
- Its generated C path is not source-driven, standalone, or a Python extension pipeline.
- The claimed chronology is not supported by the supplied implementation.
- Existing tools and languages already cover many proposed ingredients; differentiation needs workload evidence.
- Rust is a plausible implementation host to test after semantic parity exists, not a reason to select the language architecture.
- The strongest immediate inheritance from the portfolio is its development and verification method.

---

## Resume prompts

### Product and audience

> Resume from Veritas Inception Checkpoint v1.0 (October 22, 2026). Do not start implementation yet. Help select the first user and real problem for which a Python-approachable, typed, natively executable language could be meaningfully better than Python plus current tooling. Keep market claims evidence-based; distinguish product ambition from the first release. Produce candidate workloads and a decision record.

### Syntax and semantics

> Resume from Veritas Inception Checkpoint v1.0 (October 22, 2026). Use the first-user problem and acceptance corpus as constraints. Compare concrete syntax candidates and define only the minimum semantics the corpus requires. Treat Planes, Liminate, Liminate-dev, and Decan as evidence about development method, not a mandate to merge their designs. Make ambiguities and rejected alternatives explicit.

### Compiler and runtime feasibility

> Resume from Veritas Inception Checkpoint v1.0 (October 22, 2026). First pin the language corpus and reference behavior. Then design one vertical slice whose run and native-build outputs must agree, with precise diagnostics and reproducible packaging. Compare a Python reference implementation with a Rust-hosted route only against those acceptance criteria. Do not call a C FFI demo a Python extension or source compiler.

---

## Document status and provenance

This document records the discussion and evidence available on October 22, 2026. The starter files came from the referenced “Verify Rust Ideas” ChatGPT conversation. The Python files were executed from isolated temporary copies; uploaded originals were not changed. Repository context was reviewed from available local checkouts and repository contents. The Decan local checkout was stale relative to the remote repository and is identified as such. The Planes README and corpus were reviewed; the available local Planes checkout predates the latest remote Rust experiment. Liminate and Liminate-dev were reviewed through repository file listings and documented examples/tests. No project-level instruction files were opened or changed.

Language and tooling comparisons link to official primary documentation. Repository-specific implementation claims should be rechecked against a pinned commit before a later build or release decision.

**This checkpoint is ready to guide the next design milestone. It does not specify a compiler build.**

