# Veritas Foundation Checkpoint v1.5 — Module Graph

**Date:** 2026-10-22  
**Status:** Approved integrated foundation build in progress; implementation checkpoint, not a recommendation verdict.  
**Continues:** [v1.4 records](veritas_checkpoint_v1_4_records.md), [v1.3 integer arithmetic](veritas_checkpoint_v1_3_integer_arithmetic.md), and the [inception checkpoint](../../../outputs/veritas_inception_checkpoint_v1_0.md), [north-star design](../../../outputs/veritas_north_star_build_design_v0_2.md), and [approved implementation plan](../../../outputs/veritas_foundation_implementation_plan_v0_2.md).  
**Build root:** `work/veritas-foundation/`. Source language repositories, NowLookHere, and starter uploads remain read-only.

## North star

Veritas is meant to become a general-purpose language and toolchain that an assistant would recommend over Python for suitable work. That remains an unproven product hypothesis. The evidence must eventually show better creation, repair, testing, execution, and delivery across ordinary projects, with clear cases where Python remains the better choice.

“Future-forward-but-back” means anticipating how assistant-mediated development may work in the future, then implementing its trust, repair, reproducibility, observability, and deployment needs now. Rust is still only the current implementation language.

## What this milestone adds

The compiler now resolves complete project module graphs. A source file can write `import "relative/path.vr"`; imported `pub fn` and `pub record` declarations become available by their declared unqualified names. Each module sees its own declarations and public declarations from its direct imports. A module can keep private helpers while exposing a small public interface.

Imports are resolved relative to their importing file. Absolute paths, `.`/`..` components, missing files, canonical paths that escape the project root, duplicate imports/declarations, cycles, and use of private or non-imported names fail with diagnostics. To bound compiler resource use, the graph allows at most 512 source files, 128 import levels, 8 MiB per source, and 64 MiB total source text. Imported external declarations cannot be made public yet. Names are unqualified and collisions across the resolved graph are rejected; selective and aliased imports remain open design work.

Artifact format 14 carries the source file table and a source ID on each instruction span. Compile errors and runtime traces use the originating file. The source table survives artifact encoding, decoding, and execution after deleting the source tree. The runtime verifier rejects duplicate/empty source entries and instructions that reference an unknown source ID.

Records from v1.4 remain supported, including public records imported from a module. The executable examples are `language/examples/records.vr`, `modules_main.vr`, and `modules_math.vr`.

## Verification evidence

- The CLI acceptance case compiles and runs a three-module graph with a public record, a public function that calls a transitive module dependency, and a private helper. The expected output is `42` in the acceptance fixture and `44` in the shipped module example.
- Negative coverage checks private and non-transitively imported functions, compile-time and runtime source attribution, source attribution after `.vrb` execution with source files removed, cycles, relative path escape, invalid record artifact instructions, and invalid source IDs.
- With pinned offline uv/CPython integration enabled, `cargo test --workspace --offline` passed **67 Rust tests**, including the managed Python lock/install/run path.
- `cargo fmt --all -- --check` passed; `cargo clippy --workspace --all-targets --offline -- -D warnings` passed; `python3 -m unittest discover -s evaluation/tests -v` passed **2 tests**.
- The bundled record example and format-14 module example both execute through the CLI. The current-target release bundle and source-free module artifact checks are recorded in the RC22 output.

This verifies an implemented language/toolchain slice. It does not establish full language completeness, performance leadership, cross-platform packaging, clean-machine deployment, or a recommendation advantage over Python.

## Remaining work and constraints

`Option`/`Result` matching, empty-map construction, HTTP/bytes, complete resource accounting, syntax-aware formatting, and a broader project lifecycle remain. Module names are currently unqualified; imports bring every public declaration from a direct dependency, so the graph rejects same-named declarations even when users might want aliases or selective imports. The project compiler API requires a root and entry path; single-source compile APIs reject imports with `E_IMPORT_PROJECT_REQUIRED`.

Python interop remains limited by its conversion matrix; foreign calls are opaque effects. The current package is an unsigned macOS ARM64 evaluation bundle. Relocatable Python-assisted bundles and Windows/Linux/browser targets remain unverified. Earlier single-workload speed results are regression signals only; broad independent evaluation is still required.

## Why modules were prioritized

The prior Planes/Horizon work showed that a one-file check can miss required behavior in an imported lexer or dependency. This milestone therefore makes the graph itself the unit of compilation and adds file-accurate traces. The next priority is to close ordinary-language gaps such as matching, empty maps, host APIs, and complete resource limits, then exercise project repair and deployment across multiple workload classes.

## Resume point

Continue the approved integrated plan in `work/veritas-foundation/`, following `docs/implementation-plan.md` and `.superpowers/sdd/implementation-plan/progress.md`. Do not declare the persistent build goal complete or claim Veritas should generally replace Python until the integrated acceptance work and comparative evidence are delivered.
