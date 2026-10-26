# Veritas prototype build description v0.1
## A native invoice tool, an inspectable contract, and a portable computation kernel

**Date:** October 22, 2026  
**Status:** Proposed build scope for review; implementation has not started  
**Relationship:** Builds on Veritas Inception Checkpoint v1.0 and future-use test results v0.1.  
**Purpose:** Create the smallest real Veritas implementation that can support a meaningful, scoped comparison with Python.

## Intent

Build a small, usable general-purpose language prototype and use it to learn whether its design can support three future-facing needs:

1. A human or coding agent can understand a program's interface and make a requested change safely.
2. The same pure computation can compile to native code and to WebAssembly with identical results.
3. The compiler can expose enough checked behavior that generated code is reviewable and maintainable.

The build is a prototype. It does not establish that Veritas is broadly better than Python or ready to recommend for arbitrary programs. The resulting test can support a recommendation for a narrow class of programs if the evidence is strong.

## First program

Use an invoice CSV summarizer based on the existing Planes H7 workload and fixture. The Veritas program reads the existing Planes Rust experiment fixture format: each nonblank line has exactly two comma-separated integers, quantity and unit cents. It calculates the subtotal in exact rational cents, applies an exact 8% tax, rounds the final amount once to a whole cent using half-away-from-zero, and prints a stable result. This intentionally reuses the existing bounded H7 input and expected output (due 2854966.72); it does not claim general CSV support. Malformed rows must identify their row and problem. Arithmetic uses checked signed 128-bit intermediates and reports overflow rather than wrapping.

The follow-up change is fixed before implementation: add an optional --tax-rate argument, defaulting to 8%, accepting decimal percentages from 0% through 25% with at most two fractional digits, and preserving the current output when omitted. Reject malformed and out-of-range rates with a clear error. Capture the existing output, a different valid rate, zero, the maximum, and invalid inputs as expected cases. This tests whether a builder or agent can find the relevant code, understand its behavior, make the change, and preserve existing cases.

The application is intentionally modest. It exercises functions, records, collections or iteration, conditions, parsing, errors, and input/output. It does not claim to be a complete business application.

## Language prototype

### Surface and semantics

For this prototype, use a Python-readable, indentation-based surface with explicit function signatures and inferred local types. This is a reversible syntax experiment, not a final syntax lock.

Support only what the invoice example and its tests require:

- integer, boolean, text, and a cents-based Money value;
- records, lists, function calls, immutable local bindings, conditionals, and loops;
- explicit returned errors for invalid input;
- a small documented standard surface for reading text, writing output, splitting the bounded fixture rows, and parsing exact integers/percentages;
- precise source locations and structured diagnostics for syntax, type, and runtime failures.

Do not silently coerce text to numbers. Reject malformed input with the row and field identified. Arithmetic and rounding behavior must be defined and covered by boundary cases.

The compiler emits a machine-readable inspection document for the source program: declared functions and types, required host capabilities, and external effects. Compute effects transitively from a closed call graph; annotate built-in host operations such as input and output with their capabilities. Reject unresolved dynamic or foreign calls in this prototype. Also provide human-readable diagnostics. The inspection document is a prototype interface, not a claim that the compiler can infer arbitrary developer intent.

### Compiler and targets

Implement the compiler in Rust. For this prototype, lower the typed language into Rust source and invoke the installed Rust compiler for output. Keep the generated Rust as an inspectable intermediate artifact. This is a deliberately pragmatic way to test parsing, typechecking, diagnostics, and native execution before committing to a custom machine-code backend.

Provide:

- a run command that compiles and executes the source with the same language semantics as build;
- a native build command for the invoice CLI;
- a WebAssembly build for the same pure invoice calculation function, taking the same source-defined Text input as the native path through a UTF-8 pointer/length wrapper and returning the same structured success or error result, with no ambient file or network access;
- output parity checks using the same deterministic values for native and WebAssembly computation.

The WASM proof targets the installed wasm32-unknown-unknown toolchain and Node's WebAssembly runtime. It does not target WASI, browsers beyond Node-based execution, or arbitrary hardware. Veritas must name the limits of each target.

The prototype may require Rust to be installed and may use the host compiler as a backend. This is acceptable for learning. It does not satisfy the later goal of a self-contained Veritas toolchain or single-file deployment.

## Evaluation

### Stage A — language correctness

- Run a corpus containing valid invoice programs, malformed source, malformed CSV, boundary numeric values, rounding cases, and invalid type combinations.
- Require deterministic results and actionable source/row/field diagnostics.
- Confirm the inspection document reports the declared interface and host capabilities.

### Stage B — build and portability

- Compile and run the CLI natively.
- Compile the pure calculation function to WebAssembly.
- Feed equivalent inputs to native and WebAssembly paths and require identical results.
- Confirm unsupported I/O is rejected from the WebAssembly target with a clear diagnostic.

### Stage C — builder and agent workflow

- Give three separate fresh coding sessions the same base-program brief and repository snapshot, with no prior conversation history. Each session implements the invoice tool, then receives the preselected follow-up tax-rate change.
- Repeat the same procedure in Python with three fresh sessions, using the same model/version, machine, fixture, tests, tool access, and 60-minute time limit per session.
- Record whether it discovers the right files, uses the inspection output and tests, changes the expected behavior, preserves prior behavior, and explains uncertainty.
- Have a human review the diff and record corrections.
- Repeat on an equivalent Python implementation using a competent current Python setup. Keep the task, data, acceptance cases, and evaluation method the same.

Capture setup time, time to a correct result, failed attempts, human interventions, correctness, diagnostic usefulness, dependency friction, deployment steps, and review effort. Do not use raw code length or runtime alone as the recommendation measure. Report the language versions, tool versions, machine, and exact task prompt.

### Exact session tasks and clean starting points

Each session receives the same fixture, language documentation, compiler or Python interpreter, and acceptance tests, but no completed invoice application. The Veritas clean workspace contains the compiler and a blank source file; the Python workspace contains a blank script. Neither receives the other implementation. The agent gets the same two tasks in sequence:

1. **Build task:** “Implement a command-line invoice calculator that reads the provided two-column rows (quantity, unit cents), validates input, applies an exact 8.00% tax by default, rounds the final amount once to cents using half-away-from-zero, and prints `due <amount>`. Match the supplied valid and invalid-input acceptance cases.”
2. **Change task:** “Add an optional `--tax-rate` decimal percentage from 0.00% through 25.00%, with at most two fractional digits. Preserve the default output when omitted. Reject malformed and out-of-range values clearly. Keep all existing cases passing.”

The fixture's expected default output is `due 2854966.72`; the pinned 10.00%, 0.00%, 25.00%, and 8.25% outputs are `due 2907836.47`, `due 2643487.70`, `due 3304359.63`, and `due 2861575.44`. These were computed from all 5,000 rows using integer cents and exact percentage arithmetic. Negative and rounding-boundary cases belong in the synthetic acceptance corpus as well.

### Recommendation threshold

Pre-register the comparison before either implementation is tested. Use three fresh, history-free coding sessions per language with the same model/version, task prompt, acceptance corpus, tools, and 60-minute time limit per session. Veritas must pass every correctness and cross-target case; have no more median human intervention/review minutes than Python; and show at least a 20% improvement in either median clean-checkout-to-correct-reviewed-result time or median deployment/setup time. Count Veritas' Rust compiler requirement as setup friction and report it separately from the language implementation time. If Python wins or results vary materially across sessions, do not recommend Veritas yet; record why and revise or expand the experiment. Passing this one task class only supports a narrow recommendation.


## Calls made for this prototype

These choices answer the build review's material planning questions. They are local to this experiment and may be revised after evidence:

- **Numeric rule:** exact rational cents, 8% default tax, one final cent rounding half away from zero, checked signed 128-bit intermediates. This matches the existing Planes H7 behavior and expected output.
- **Recommendation threshold:** three independent fresh sessions per language, each limited to 60 minutes; all acceptance cases pass; median human intervention/review minutes are no higher; median time to a correct reviewed result or deployment/setup time improves by at least 20%. Rust toolchain setup is counted and reported.
- **WASM boundary:** the same source-defined Text input is passed as UTF-8 bytes through a pointer/length wrapper. Both targets execute the same Veritas calculation and return the same structured result.
- **Effects in inspection output:** statically summarize the closed function call graph and annotate built-in input/output capabilities. No dynamic or foreign calls are supported in this prototype.
- **Python baseline:** use the same model/version in separate history-free sessions, with the same brief, fixture, acceptance cases, tools, and time limit. Build the baseline from the same clean starting point, not from a hand-tuned implementation.

## Non-goals

This prototype does not attempt:

- Python syntax or API compatibility;
- importing arbitrary Python packages;
- concurrency, async programming, or GPU execution;
- a general package manager or standard library;
- automatic proof that programs match unstated human intent;
- full CSV compliance with quoting, escaping, and embedded newlines unless the selected fixture requires it;
- a custom native code generator or VM;
- a claim that the prototype should replace Python across all domains;
- changes to Planes, Liminate, Liminate-dev, Decan, or the uploaded Veritas files.

## Implementation location and review

Build in a new isolated folder under this task's work directory, leaving the uploaded files and prior-language repositories untouched. Keep generated binaries and experiment data in that folder. Record design decisions, unsupported cases, commands, and outputs. At the end, run the three evaluation stages and publish a concise results report beside the inception checkpoint.

## Decisions the builder may make

The builder may choose parser/code-generation libraries, internal AST shape, Rust project layout, caching strategy, test framework, temporary build-directory layout, and exact CLI flags. The fixture is the existing two-integer row format; the builder must verify and pin its current output before coding. Report any toolchain limitation that changes the targets.

## Approval boundary

Approval of this description authorizes the prototype as scoped here. A future expansion to general Python package interoperability, concurrency, more target platforms, or a self-contained distribution is a separate design decision.

