# Veritas Foundation Checkpoint v1.4 — Records

**Date:** 2026-10-22  
**Status:** Approved foundation build in progress; this is an implementation milestone, not a release recommendation.  
**Continues:** [Checkpoint v1.3 — integer arithmetic](veritas_checkpoint_v1_3_integer_arithmetic.md), the [inception checkpoint](../../../outputs/veritas_inception_checkpoint_v1_0.md), and the approved [north-star design](../../../outputs/veritas_north_star_build_design_v0_2.md) and [implementation plan](../../../outputs/veritas_foundation_implementation_plan_v0_2.md).  
**Build root:** `work/veritas-foundation/`. Original language repositories, NowLookHere, and uploaded starter files remain read-only.

## North star

Veritas is intended to become a general-purpose language and toolchain that an assistant can recommend over Python for suitable real work. The short-term proof must show that developers can create, understand, change, test, run, and deliver ordinary projects with materially less friction or better dependability. No current milestone demonstrates that recommendation case.

The future-forward-but-back principle remains: design the project experience for assistant-mediated software work as it may evolve, and implement the necessary trust, repair, reproducibility, observability, and deployment properties now. Rust remains an implementation choice, not a Veritas identity requirement.

## This milestone

Records are now part of the executable language subset:

```veritas
record Point:
    x: Int
    y: Int

fn shift(point: Point, dx: Int) -> Point:
    return Point(point.x + dx, point.y)
```

Field order is declaration order; constructors are positional. Fields have exact declared types, access names are checked during compilation, and equality is structural between values of the same record type. Schemas and record values are encoded in artifact format 13. The VM verifies record indices, field indices, constructor arity and field types, and checks record constants against their schemas. Records are not yet supported by the Python bridge.

The addition began with an end-to-end source fixture that failed before support because the lexer rejected `.`. Compiler, artifact encoding/decoding, runtime, artifact verifier, example, and reference documentation now cover the record path. Negative coverage includes unknown field/type, duplicate field, wrong constructor type, invalid record instruction indices, and a forged schema that refers to an unknown record type.

## Verification evidence

- `cargo test --workspace --offline`: **66 Rust tests passed** across CLI, compiler, model, runtime, and Python bridge.
- `cargo fmt --all -- --check`: passed.
- `cargo clippy --workspace --all-targets --offline -- -D warnings`: passed.
- `python3 -m unittest discover -s evaluation/tests -v`: **2 tests passed**.
- The standalone `records.vr` example compiled and ran through the CLI, printing `5` and `3`.
- With the pinned offline uv/CPython integration enabled, the full Rust suite passed, including managed CPython dependency locking and execution.
- The extracted RC21 bundle passed `doctor`, ran the records example, built format-13 bytecode, and ran that bytecode after hiding the copied source file. The release archive has a matching SHA-256 sidecar, which verifies.
- The package is still an unsigned macOS ARM64 evaluation archive, and this extraction test is not a clean-machine or cross-platform deployment claim.

These tests verify implementation behavior. They do not establish semantic completeness, language-level performance advantage, broad usability, clean-machine portability, or recommendation superiority over Python.

## Current boundaries and work still required

The candidate still lacks multi-file module imports/source maps, general `Option`/`Result` matching, HTTP and byte-oriented APIs, complete resource accounting, syntax-aware formatting, relocatable Python-assisted bundles, Windows/Linux/browser packaging, and broad independent evaluation. Python interop still has conversion limits and foreign calls remain opaque effects. Mutable sharing is not supported; file writes can leave partial output after an OS failure.

Highest-value next build step: implement and verify a real module graph with explicit relative imports/exports, cycle and collision diagnostics, and per-file source attribution. This closes a risk seen during Planes/Horizon work: one-file success can hide missing behavior in imported files. Then finish matching and empty-map construction, resource accounting, host/API gaps, and the integrated project lifecycle. Keep benchmark and recommendation claims separate from feature completion.

## Prior-language evidence and boundaries

Planes was intended as a general-purpose language and was exercised in the browser game Horizon; it makes repeated use, module graphs, latency, and memory behavior relevant. Liminate contributes bounded, observable parsing and stable diagnostics. Decan contributes explicit capability and conversion-loss reporting across consumers. These are engineering lessons from read-only review, not imported language semantics. See [prior-language-build-lessons.md](prior-language-build-lessons.md) and [reuse-inventory.md](reuse-inventory.md).

## Resume point

Continue the approved integrated build in `work/veritas-foundation/`, from this checkpoint, the [implementation plan](implementation-plan.md), and `.superpowers/sdd/implementation-plan/progress.md`. The source now has format-13 record support and the checks listed above. Continue the full work plan; do not mark the goal complete or claim general recommendation until module graphs, ordinary project acceptance, platform/deployment limits, independent comparative evidence, and the approved final report are addressed.
