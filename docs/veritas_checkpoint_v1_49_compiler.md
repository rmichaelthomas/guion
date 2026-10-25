# veritas_checkpoint_v1_49_compiler.md

# CANONICAL CHECKPOINT DOCUMENT
## Veritas
### v1.49 — compiler: artifact and bootstrap state

**Status:** LOCKED — EXTENDS Veritas checkpoint v1.48
**Date:** October 24, 2026
**Author:** Rob Thomas / R. Michael Thomas and Codex (analytical and implementation partner)
**Domain prefix:** veritas (provisional, pre-vault)
**Session type:** Verification checkpoint for the complete-product implementation plan
**Relationship to prior checkpoints:** Direct continuation of [Veritas checkpoint v1.48: benchmark evaluation](veritas_checkpoint_v1_48_benchmark_evaluation.md) (October 22, 2026). The local predecessor uses unnumbered headings and has no open-question table; this checkpoint starts a structured sequence at Part I / §1 and records one current open question. No historical Part/§ or question counts are inferred.

This is a verification checkpoint, not a new architecture proposal. The prior evaluation established that interpreted bytecode missed the north-star performance hypothesis and identified a native backend as later work. The complete-product plan then asked whether the source tree could become an operational, self-hosted product. The live implementation now has a documented artifact format, a checked compiler seed, a default CLI that uses the Veritas compiler, and passing three-stage and full-corpus checks. The first normal-stack C2/C3 corpus run exposed a guest-call stack limit; the observed gap was closed with explicit heap-backed guest frames, pulled forward from Task 6 under the plan's dependency rule.

## HOW TO READ THIS DOCUMENT

This is Veritas checkpoint v1.49. It locks the verified Task 3 compiler and runtime state and records the exact self-host boundary. It does not claim the complete product goals G1, G2, or G3. Tasks 4–11 remain work in the implementation plan, and Task 9 must refresh the full bootstrap after the compiler and library additions.

## PART I — ARTIFACT CONTRACT

**Decision: The checked artifact contract is format 34, emitted by the Veritas compiler and independently parsed and verified by the native reader. LOCKED.**

### §1 — Canonical bytes and verification

`language/artifact-format.md` records field order and encoding for artifact headers, source graphs, records, externals, functions, resource ceilings, spans, types, constants, and instructions. `language/self_host/artifact.vr` owns the Veritas-side encoder; bounded byte primitives construct the payload, while artifact parsing and verification remain native. The format contract documents the 64 MiB artifact ceiling, bounded counts, nesting ceiling, append-only opcodes, version compatibility, and rejection of malformed or trailing data.

Malformed-input coverage now includes truncated artifacts, oversized string and count fields, unknown opcodes, forged function references, and invalid branch targets. Compiler-generated C1, C2, and C3 artifacts load through the same native reader and runtime verifier as user artifacts. Focused model and runtime tests passed; the full model suite passed 13 tests and the runtime execution suite passed 99 tests.

## PART II — SELF-HOSTED COMPILER

**Decision: The normal CLI uses the checked Veritas compiler seed; Rust stage zero remains an explicit bootstrap tool and is not a default runtime dependency or fallback. LOCKED.**

### §2 — Seed identity and regeneration

`language/self_host/compiler-seed.json` records schema version 1, artifact format 34, host ABI 1, the production compiler entry, the ordered compiler source files, and a reproducible stage-zero regeneration command. Its verified identities are:

| Identity | SHA-256 |
|---|---|
| Compiler source graph | `23b8a762dd64a80ec078473d24788914f6c1f6aa5eb377a407b1ff0e71b2df2d` |
| Checked compiler artifact | `891a352a360c10aae24c62fa61007c6c7c0eed4ea495711fa4216ff9d783575d` |
| Artifact schema | `1a8ebaf116436c44a82bdd42a36bc3fe8e5886d7042dacbd9d8169f3a59cd15c` |

The artifact and schema hashes were recomputed from their checked files and match the manifest. `cargo run --locked --offline -p veritas-bootstrap --bin veritas-stage0 -- seed .` regenerates the seed and manifest. The default CLI verifies embedded seed identity and refuses mismatches. Tests prove it works without stage-zero files or compiler source and has no Rust source-compiler fallback; the dependency graph excludes `veritas-compiler` from the normal CLI/runtime dependencies.

### §3 — C1, C2, C3 and corpus boundary

The three-stage bootstrap test built C1 from the Rust reference compiler, compiled the complete compiler to C2, then used C2 to emit C3. C2 and C3 canonical artifact bytes were equal without source-root normalization because both stages used the same relative source graph and omitted build metadata. The native reader and runtime verifier accepted all three stages.

The stage2/stage3 conformance run passed all 192 positive, negative, and interaction cases. The general bootstrap/self-host corpus also passed all 192 cases, comparing acceptance, diagnostic codes and locations, runtime results, standard output, standard error, and recorded effects. The 35 compiler-specific cases passed.

## PART III — GUEST EXECUTION DEPENDENCY

**Decision: Guest-to-guest calls use explicit heap-backed VM frames and enforce the configured call-depth limit without relying on a large host thread stack. LOCKED.**

### §4 — Stack overflow resolved from observed corpus evidence

The initial full stage corpus overflowed the ordinary host stack during guest calls. Following the explicit dependency exception in Task 3, the guest-frame work from Task 6 was pulled forward. `Runtime::invoke_inner` now starts an explicit frame loop; calls push frames into a heap-backed vector, and return or propagated-result paths unwind that vector while preserving call-site traces. Recursive functions continue to use VM fallback when the native backend cannot compile them.

The runtime regression test passes 400-deep recursion with a depth limit of 512 and rejects the first call beyond that limit with matching error values and traces in VM-only and native-enabled modes. Tests also confirm that recursive functions are excluded from JIT compilation. The 64 MiB test-thread stack workarounds were removed. The remaining Task 6 resource lifecycle, cancellation, and HTTP work has not been pulled forward.

### §5 — Gate state and evidence boundary

`cargo xtask conformance --compilers stage2,stage3` exited 0 and produced a `passed` macOS ARM64 report. The report records the checked seed hash. The stage2/stage3 corpus and full general corpus passed after the frame change.

Every configured check in `cargo xtask bootstrap --stages 3` passed: self-host compiler parity, seed regeneration, C2/C3 equality, embedded CLI operation, and the default CLI dependency graph. The report is `incomplete` with exit status 1 only because Task 3 was still marked `in_progress` when the command ran. It is not represented here as a passed bootstrap report. Task 9 will rerun the bootstrap after the final compiler and library additions, with Task 3 already recorded complete.

## WHAT IS LOCKED

- Artifact format 34, native reader/verifier parity, and the current deterministic compiler seed identities.
- The acyclic Rust stage-zero → Veritas C1/C2/C3 seed workflow and no-fallback default CLI boundary.
- Exact C2/C3 artifact equality and the passing 192-case stage2/stage3 corpus.
- Explicit guest frames as the runtime call model required for normal-stack operation, with configured depth-limit and trace parity.
- The known nine briefs remain regression applications; no frozen campaign result is reclassified as fresh evidence.

## WHAT IS NOT LOCKED

- Tasks 4–11, including the data and service libraries, remaining runtime lifecycle work, nine-application delivery, release targets, performance evidence, and fresh builder evidence.
- G1/G2/G3 completion. No product-wide closeout claim is made.
- A final passed `bootstrap.json` report. The current report's individual checks passed, but its ledger guard remains incomplete until the final Task 9 bootstrap rerun.

## WHAT IS LOGGED

- The first slow compiler-conformance gate was shortened by using the source-identity-checked seed artifact in per-case fixtures; all 35 compiler-specific cases passed in under 20 seconds on the refreshed run.
- The full self-host/application parity check took 1,552.44 seconds after the frame change; the C1/C2/C3 equality check took 796.56 seconds. Their test output was quiet until completion because Rust's test harness buffers each test.
- The generic language tests now run serially in the runner so they share the compiler artifact cache rather than initializing it concurrently.
- The local v1.48 checkpoint had no Part/§ structure or open-question table. This checkpoint establishes Part I / §1 and one new open question without inferring a historical sequence.

---

## UPDATED OPEN QUESTIONS (v1.49 status)

| # | Question | Status |
|---|---|---|
| 1 | Do G1, G2, and G3 pass the final product gates? | Open — Tasks 4–11 and final closeout remain. |

---

## DOCUMENTS PRODUCED THIS SESSION

| Document | Type | Status |
|---|---|---|
| `veritas_checkpoint_v1_49_compiler.md` (this document) | Checkpoint | Complete, LOCKED |
| `docs/superpowers/plans/2026-10-24-veritas-complete-product.md` | Implementation plan | Task 3 checked complete; Tasks 4–11 open |

---

## RESUME PROMPT (v1.49)

*Resume the Veritas complete-product implementation from checkpoint v1.49 and plan `docs/superpowers/plans/2026-10-24-veritas-complete-product.md`. All decisions through the local v1.48 evaluation checkpoint remain in force; this checkpoint begins structured Part I / §1 because v1.48 used unnumbered headings. Task 3 is complete: artifact format 34 and its native verifier path are documented; the checked compiler seed is bound to source SHA-256 `23b8a762dd64a80ec078473d24788914f6c1f6aa5eb377a407b1ff0e71b2df2d`, artifact SHA-256 `891a352a360c10aae24c62fa61007c6c7c0eed4ea495711fa4216ff9d783575d`, and schema SHA-256 `1a8ebaf116436c44a82bdd42a36bc3fe8e5886d7042dacbd9d8169f3a59cd15c`; default CLI self-host tests, C1/C2/C3 equality, 35 compiler cases, and both 192-case language corpora passed. Guest calls now use heap-backed frames after a normal-stack overflow, and that Task 6 dependency has been pulled forward; remaining Task 6 work is still sequenced later. The bootstrap command's five checks all passed, but `bootstrap.json` is recorded as incomplete because the Task 3 ledger guard was still open at invocation; rerun the full bootstrap after Task 9's final compiler/library additions. Current task is Task 4, everyday data, text, and file APIs. Start with `cargo xtask stdlib --group data`, carry the existing repo state forward without resetting it, use the pinned offline Rust environment for required Linux ARM64 checks, and continue sequentially through Task 11. G1/G2/G3 remain open until the final closeout evidence passes.*
