# Veritas future-use test — initial results

**Date:** October 22, 2026  
**Status:** Evidence-gathering pass complete; Veritas/Python comparison awaits a Veritas implementation  
**Purpose:** Exercise the three future-use hypotheses against available project evidence and runnable checks: human/agent collaboration, portability, and increased need for checking as code generation becomes cheaper.

## Result at a glance

The available evidence supports several early foundations for Veritas: explicit contracts, machine-readable diagnostics and program information, deterministic behavior, portable interface boundaries, and tests that cover real consumer cases.

The strongest runnable portability result is from a bounded Planes experiment: the same two workload results match between native Rust and WebAssembly. The strongest user-facing evidence is Decan's conformance and consumer test suite: 109 tests passed across 39 test files, with TypeScript typechecking clean.

**This did not test whether Veritas should be recommended over Python.** No usable Veritas compiler, interpreter, or package workflow exists yet. No agent built or modified the same project in Veritas and Python. Those comparisons cannot be inferred from tests of Planes or Decan.

## Test 1 — Human and agent collaboration

### Hypothesis

As humans and agents co-create and change programs, it becomes more important to expose what code means, what it can affect, what rules it must satisfy, and what changed.

### Evidence exercised

In the local Decan checkout, the full Vitest suite passed: **39 files, 109 tests**. This included public CLI, MCP interface, and documentation-example tests; consumer-evidence corpus checks; conformance, adapter, deterministic hashing, snapshot, materialization, and fail-closed behavior. TypeScript typechecking also passed with no emitted output.

This supports one design principle: the machine-facing surface can be tested as carefully as the human-facing language. Examples can remain runnable, consumer cases can be executable, unsupported conversions can fail explicitly, and the same semantic core can support CLI and MCP entry points.

### What it does not establish

These tests do not show that an AI agent can build or safely change a substantial application with Decan. They do not measure review effort, number of human corrections, or whether structured errors make an agent more reliable. The Decan checkout tested locally is at revision 7733c25 from August 27, 2026; the remote repository had advanced through October 16. The result describes that local snapshot, not the remote HEAD.

### Implication for Veritas

Make source, public interfaces, diagnostics, and validation results accessible through stable machine-readable forms. Test the agent workflow directly later: give a fresh agent a project brief, require a nontrivial change, and score correctness, regressions, human interventions, and reviewability. Do not claim agent-readiness from a compiler test suite alone.

## Test 2 — Movement between hosts and targets

### Hypothesis

If software must move between local machines, servers, and specialized targets, its core meaning and external dependencies need explicit boundaries.

### Evidence exercised

The isolated Planes Rust experiment passed:

- 6 Rust runtime tests for exact decimal parsing, one-time invoice rounding, half-away-from-zero behavior, checked overflow, and immutable record updates.
- 1 fixture test confirming the committed invoice input matches its deterministic generator.
- 4 Node-driven WebAssembly checks confirming output agreement with the native Rust result and rejecting malformed or oversized invoice input.
- cargo fmt check.

The WebAssembly check used an already-built wasm32-unknown-unknown artifact and Node's WebAssembly runtime. It did **not** exercise a WASI component, a second operating system, a different CPU architecture, or hardware acceleration.

The experiment's existing results.md reports parity across five implementations for its H7 workloads and records browser measurements. Those benchmark figures were not rerun here; the benchmark harness referenced by its README is not present in this separate checkout. The report itself says the Rust spike implements two selected workloads directly and does not parse or run Planes source. It is evidence for those bounded operations only.

### Implication for Veritas

Portability starts with semantics and interface boundaries, not a promise to compile everywhere. Define which behaviors are deterministic, how input/output and host services enter the program, and how unsupported host capabilities are reported. Start with one target beyond the primary runtime and require output parity. Treat WebAssembly components as a candidate deployment path; choose them only if an initial user workload benefits.

## Test 3 — Checking and maintaining generated code

### Hypothesis

If code becomes cheaper to generate, teams need dependable ways to check behavior and understand the consequences of changes.

### Evidence exercised

The Decan suite's consumer corpus, hash stability, canonical strictness, fail-closed conversions, replay/materialization, and public example checks show how explicit evidence can be made part of a project. The Planes Rust runtime tests demonstrate that narrow numeric and mutation guarantees can be captured with small boundary-focused tests. Together they support testing exact contracts and hostile or unsupported inputs, rather than relying only on a successful demo.

### What it does not establish

Passing tests do not prove that a program matches the developer's intent. A type checker, a test suite, static effect analysis, and formal verification each establish different things. A language cannot infer missing requirements just because it has a compiler. Adding proof obligations also costs authors time and can make ordinary programming harder.

### Implication for Veritas

Start with robust type checking, explicit failure behavior, a strong example/test workflow, and dependency/build records. Design compiler outputs so editors and agents can inspect interfaces, diagnostics, and effects. Add richer contracts or formal verification only for properties that users repeatedly need to state and check.

## Cross-test conclusion

The strongest future-facing foundations already have present-day value:

1. Stable, machine-readable interfaces and diagnostics.
2. A testable semantic model with deterministic behavior where practical.
3. Explicit external capabilities and failure behavior.
4. A corpus containing realistic consumer programs and negative cases.
5. Evidence that can be inspected by humans and consumed by tools.

The experiments also caution against overreach: the Rust/WebAssembly Planes spike is portable for two preselected computations, not for Planes generally. Decan's automated suite validates its local snapshot, not agent collaboration at application scale. Neither result establishes that Veritas has a distinct advantage over Python.

## What remains for the actual recommendation test

Once Veritas has a coherent small implementation, choose three real project briefs from Rob's work and build equivalent outcomes in Python and Veritas. Include at least one ordinary CLI/data task, one project with meaningful external interfaces, and one substantial follow-up change made by a fresh agent. Record setup time, time to a working result, human interventions, library gaps, diagnostic usefulness, correctness, deployment friction, and change review effort. Give Python a competent current toolchain. Repeat with more than one builder or agent where practical.

The outcome should be a scoped recommendation decision. For example, “recommend Veritas for this class of CLI and data tools under these constraints” is testable. “Veritas is better than Python” requires a much broader body of evidence.

## Execution record

- Decan: npm test -- --cache=false — passed, 39 test files / 109 tests.
- Decan: npm run typecheck — passed.
- Planes Rust experiment: cargo test --test runtime — passed, 6 tests.
- Planes Rust experiment: cargo fmt -- --check — passed.
- Planes Rust experiment: invoice fixture check — passed, 1 test.
- Planes Rust experiment: WebAssembly output check — passed, 4 tests.
- Existing worktrees remained clean after these checks.
- Codon, Mojo, Nim, and Wasmtime were not available in this environment; no comparison against them was run.

