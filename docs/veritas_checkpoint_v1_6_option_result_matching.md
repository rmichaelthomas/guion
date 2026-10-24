# Veritas Foundation Checkpoint v1.6 — Option and Result Matching

**Date:** 2026-10-22  
**Status:** Approved integrated foundation build in progress; implementation checkpoint, not a recommendation verdict.  
**Continues:** [v1.5 module graph](veritas_checkpoint_v1_5_module_graph.md), [v1.4 records](veritas_checkpoint_v1_4_records.md), and the [inception checkpoint](../../../outputs/veritas_inception_checkpoint_v1_0.md), [north-star design](../../../outputs/veritas_north_star_build_design_v0_2.md), and [approved implementation plan](../../../outputs/veritas_foundation_implementation_plan_v0_2.md).  
**Build root:** `work/veritas-foundation/`. Source language repositories, NowLookHere, and starter uploads remain read-only.

## North star

Veritas is meant to become a general-purpose language and toolchain that an assistant would recommend over Python for suitable work. That remains an unproven product hypothesis. The evidence must eventually show better creation, repair, testing, execution, and delivery across ordinary projects, with clear cases where Python remains the better choice.

“Future-forward-but-back” means anticipating how assistant-mediated development may work in the future, then implementing its trust, repair, reproducibility, observability, and deployment needs now. Rust remains only the current implementation language.

## What this milestone adds

The source language now has a statement-level `match` for `Option[T]` and `Result[T, E]`:

```veritas
match calculation:
    Ok(value):
        print(value)
    Err(message):
        print(message)
```

An Option match requires exactly one `Some(name)` arm and one `None` arm. A Result match requires exactly one `Ok(name)` arm and one `Err(name)` arm. Arms may appear in either order. The expression being matched is evaluated once, and each bound name is local to its arm. Exhaustiveness, duplicate variants, and variant-family mismatches are checked during compilation with stable diagnostics.

The compiler lowers these statements to existing bytecode operations for variant checks, value extraction, and branches. No artifact format bump was needed: this uses the existing version-14 instruction set and source maps. Matching therefore works through source compilation, artifact encoding/decoding, verifier loading, and VM execution.

The shipped `language/examples/matching.vr` demonstrates both Option and Result, both arm orders, and the use of errors as ordinary typed values. The current reference and language contract list the supported syntax and limits.

## Verification evidence

- Before the parser/compiler change, the new positive acceptance test failed with `E_STMT_EXPR`, confirming the syntax was not previously implemented.
- The end-to-end test compiles and executes Option and Result matches through a version-14 artifact, covers both arm orders, and confirms execution continues with a correct mutable value after a match.
- Negative tests verify `E_MATCH_NONEXHAUSTIVE`, `E_MATCH_DUPLICATE`, and `E_MATCH_VARIANT` for missing, repeated, and wrong-family arms.
- With the checked-in pinned uv and managed CPython integration enabled, `cargo test --workspace --offline` passed **69 Rust tests**.
- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --offline -- -D warnings`, and `python3 -m unittest discover -s evaluation/tests -v` passed; the Python harness contains **2 tests**.
- The command-line matching example printed `42`, `no value`, `17`, and `calculation failed` as expected.
- Release packaging and extracted-bundle checks are recorded in the RC23 output once completed.

This is evidence that one ordinary control-flow feature works across the language pipeline. It does not establish completeness, general-purpose readiness, performance leadership, or a recommendation advantage over Python.

## Limits and next work

Patterns currently support only the two exact variant families above. Wildcards, nested patterns, matching other types, destructuring, and match expressions are unsupported. General constructor inference also remains limited; constructors depend on declared function return context. Full resource accounting, empty-map construction, HTTP/bytes, richer project lifecycle, multi-platform and relocatable Python-assisted packaging, and broad independent project evaluation remain active work.

The release candidate is an unsigned macOS ARM64 evaluation bundle. It is not evidence of a clean-machine or cross-platform installation experience. No recommendation claim is warranted yet.

## Resume point

Continue the approved integrated plan in `work/veritas-foundation/`, following `docs/implementation-plan.md` and `.superpowers/sdd/implementation-plan/progress.md`. The next language-core work should address ordinary program gaps and resource limits, then validate project creation, repair, testing, execution, and delivery against independent workload classes. Keep the user’s original repositories and starter uploads read-only.
