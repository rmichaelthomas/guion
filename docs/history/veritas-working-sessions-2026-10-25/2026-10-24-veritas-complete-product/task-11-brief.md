### Task 11 — Fresh builder evidence and one final handoff

**Files:** `evaluation/completion/builder/{protocol.md,assignments.json,reports}/`, `docs/completion-report.md`, README and shipped documentation; no edits to frozen independent-v1/v2/v3 records.

**Consumes:** final installed release and evidence from Tasks 1–10. **Produces:** a bounded fresh evaluation, explicit recommendation scope, and a maintainable release handoff.

- [ ] Before dispatch, record three unfamiliar application briefs, one per target class, with correctness/change/delivery criteria and a fixed time/resource budget. They must differ materially from the nine known tasks. The implementing builder sees shipped docs and a brief, not a prewritten solution or comparative benchmark conclusions. Preserve assignments and failures.
- [ ] Run one fresh matched Python/Veritas implementation and one follow-up change per brief, sequentially, in the existing environment. This is three matched pairs, not another 18-pair campaign. Match permissions, runtime provisioning, available dependencies and delivery requirements. No author rescue is hidden; record every intervention and required product change.
- [ ] Give each language builder a separate fresh context, use the same recorded model/reasoning/tool configuration and budget, randomize execution order before starting, and prevent the second builder from seeing the first solution. A separate final assessment can inspect both outputs. Record setup time, build/repair time, installed execution time and human interventions as different measures.
- [ ] Ask each fresh builder/reviewer for an explicit recommendation **after** examining correct installed applications and observed setup, repair, delivery and maintenance effort. Record time, intervention count, failures, artifact footprint and runtime performance separately. Reward neither a faster wrong program nor familiarity alone.
- [ ] For each target class, require successful Veritas initial/change/relocation behavior and a reasoned preference supported by at least one observed user benefit without an undisclosed material regression. If Python is preferred, map the specific reason back to Tasks 2–10 and implement the missing capability before re-evaluating that class with a new brief. Do not repeat votes against an unchanged product or turn this small sample into a statistical adoption claim.
- [ ] `cargo xtask closeout` verifies G1/G2/G3 evidence links, final hashes, all required application passes, stage equality, release target parity and open-defect status. A material regression, skipped required case, Python-dependent core feature or unsupported recommendation leaves the corresponding goal open.
- [ ] Produce one final report with exact achieved scope, command sequence, source/seed/runtime/stdlib hashes, supported platforms, remaining nonblocking limitations, installation/update/removal instructions, reproducible verification commands and maintenance ownership. Commit the completed source/evidence as one release state without discarding prior work; publication or signing still needs explicit authorization.

**Verify:** `cargo xtask closeout` reports all three goals satisfied for the declared scope. If not, its output names the failing task and next implementation action. It must never say the full build is complete because only the compiler or benchmark checkpoint passed.

## 7. Review focus and tests that own it

| Failure likely to escape happy-path examples | Required owner/check |
|---|---|
| Compiler silently accepts wrong types, private imports, or unsupported syntax | Task 2 negative conformance and cycle/visibility cases; Task 3 no-fallback installed compiler test |
| Moved package reads the wrong directory or unexpectedly needs developer Python | Tasks 7–10 offline dependency, external caller data and blocked-executable relocation checks |
| Bad record destroys an existing output/index or aborts an entire batch | Task 4 recoverable JSON/JSONL cases; Task 5 rollback tests; Task 9 malformed-input application runs |
| Cancellation/deep calls leak resources or crash the host | Task 6 ordinary-thread deep-call, native fallback, trickle-response and lifecycle checks; Task 5 handle rollback |
| Formats are subtly wrong: timezone ambiguity, Unicode collisions, TOML no-op drift, signature bytes | Task 5 pinned data/conformance vectors and Task 9 full applications; dependency limitations explicitly tested |

## 8. Work order and recovery rules

Default sequence: **1 → 2 → 3 → 4 → 5 → 6 → 7 → 8 → 9 → 10 → 11**. A small byte primitive needed by Task 3 may be pulled forward from Task 4 with its tests. If full compiler operation exposes a host-stack failure, pull forward Task 6's explicit guest-frame work rather than enlarging the stack workaround. Record the dependency in this plan. Compiler/host/library changes are updated in both compiler paths until stage zero can remain a frozen seed. Rebootstrap after a compiler change, not after every unrelated documentation edit.

The largest implementation work is full compiler semantics, native library/resource services, and installed toolchain integration. Do not describe them as already finished or as a few trivial patches. Existing code and mature native libraries reduce work; they do not remove it. This plan sets no unsupported elapsed-time promise.

At any interruption, update only the active task/status record and the commands/evidence necessary to resume. A failed targeted check keeps its owning task active. Do not restart all tests or re-audit the machine. The user receives checkpoint updates and material architectural findings; implementation continues under the existing authorization.

### Ready-to-use execution instruction

> Implement `docs/superpowers/plans/2026-10-24-veritas-complete-product.md` in the existing Veritas checkout. Begin at the first unchecked task and preserve all current changes. Deliver the full self-hosted compiler, native standard library, installed core workflow, nine real applications, both supported target packages, and final performance/builder evidence. No core feature may delegate to Python or the Rust source compiler. Use one workspace, bounded processes, retained pinned toolchains and one build cache. Run meaningful checks for each changed subsystem and the three defined checkpoints; do not start another RC/review/18-pair cycle. After compaction resume the recorded next command. Do not mark any goal complete on subset evidence, and do not stop after a checkpoint while authorized implementation remains.

## 9. References and dependency caveats

- Local authority: `language/spec.md`, `language/host-api.md`, `docs/north-star-design.md`, `docs/compatibility-and-migrations.md`, and the source files named above.
- Recommendation evidence: `evaluation/results/independent-v3/choice-summary.json` and all 18 recommendation JSON files. Existing brief requirements are in `evaluation/briefs/independent/`.
- [rusqlite official API](https://docs.rs/rusqlite/latest/rusqlite/) supplies typed SQLite access. The final lock must bind an exact compatible version and bundled SQLite identity.
- [toml_edit official documentation](https://docs.rs/toml_edit/latest/toml_edit/) documents comment-preserving editing but also dotted-key ordering and final-newline limitations. Task 5 explicitly tests and covers those gaps rather than assuming perfect preservation.
- [RustCrypto HMAC](https://github.com/RustCrypto/MACs/tree/master/hmac) is the selected HMAC implementation family; use its verification API and pin a version compatible with the selected SHA-256 crate.
- [ICU case folding documentation](https://unicode-org.github.io/icu/userguide/transforms/casemappings.html) distinguishes full case folding from lowercase conversion; [ICU4X case mapping](https://docs.rs/icu_casemap/latest/icu_casemap/) supplies the native implementation family. Pin its data version.
- [libical upstream](https://github.com/libical/libical) and its [timezone component API](https://libical.github.io/libical/v4/apidocs/icalcomponent_8h.html) provide the calendar integration direction. The final version/build/API/license must be recorded; these reference URLs are not an instruction to depend on a moving development branch.

## 10. Plan self-review

- G1 is implemented by Tasks 1–3 and revalidated after Tasks 4–9; normal CLI dependency removal and full artifact emission prevent subset-only self-hosting claims.
- G2 is implemented by shared runtime work and Task 10 against the final installed build, with correctness, baseline parity and raw measurements required.
- G3's product gaps each map to Tasks 4–8, real consumers to Task 9, and fresh evidence to Task 11. The old negative choices are retained and used to drive implementation.
- The existing lock filename, large-stack workaround, Python-based distribution scripts, current module semantics, missing metadata, native library/data packaging and TOML preservation limitations have explicit owners.
- No test fixture, module path, host library, example or benchmark is allowed to substitute for an ordinary installed workflow. Old release cleanup and further research are not on the critical path.
- All named new interfaces and files are planned, not claimed present. Each task has a checkable deliverable, concrete negative cases and one verification entry point. New dependency versions are resolved and pinned once during implementation; none are silently asserted installed today.
