# SDD ledger — plan: outputs/veritas_foundation_implementation_plan_v0_2.md

Status: implementation started after user approval.

## Pre-flight interface scan
- Task A -> B: A produces grammar/semantics/diagnostic/artifact/host/interop contracts consumed by compiler/runtime. Ruling: close the first supported subset before code; stage advanced surfaces behind explicit unsupported diagnostics. Risk: later syntax additions require compatibility/version updates.
- Task B -> C/D/E: B produces compiler/runtime public APIs consumed by bridge, host/concurrency and CLI. Ruling: use one `compile` -> artifact -> `Runtime.load` -> `Instance.invoke/close` path; CLI and embedded API call it. Risk: contract churn if the first API is too narrow.
- Task A/B -> F: language examples, semantics and release interfaces drive ordinary workload/evaluation. Ruling: no app-specific language built-ins; examples exercise ordinary APIs. Risk: early workload coverage will be smaller than final gate.

## Work record
- Build root: `work/veritas-foundation/` (fresh isolated Git repository; source repos untouched).
- Environment: Rust 1.97.1, Cargo 1.97.1, Python 3.14.7, clang available; `uv` unavailable at start.
- Ruling: begin with dependency-free Rust crates and a versioned portable bytecode artifact; avoid network/package assumptions until tool discovery and lock contract are explicit. Risk: missing established parser/CLI crates may slow delivery, but no external crate acquisition is required for the foundational compiler.

### Package A
- [ ] language and architecture contract
- [ ] reusable prototype inventory

### Package B
- [ ] compiler, verifier/runtime and source debugging

### Package C
- [ ] Python bridge and dependency manager

### Package D
- [ ] host APIs and structured concurrency

### Package E
- [ ] CLI, docs and packaging

### Package F
- [ ] representative programs, evaluation kit and report
