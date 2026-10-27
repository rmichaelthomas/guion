# Guion real-task and Richards execution plan

**Goal:** Execute the approved real-task, AllHereHTML, install, Richards, and agent-choice sprint against the merged portable VM core.

**Spec:** `docs/superpowers/specs/2026-10-26-guion-real-task-and-richards-sprint.md`

**Baseline:** `9d57434`, the merge of the shared VM core. Work in the canonical Guion checkout, reuse one build directory, and keep generated scratch inside the repository. Preserve failures and report exact limits; never mark an unrun check as passing.

- [x] Preflighted the toolchain, disk use, canonical checkout, and merged VM-core baseline `9d57434`; reused the existing Cargo target directory.
- [x] Preserved a provenance-pinned AllHereHTML fixture, checked its 19 tests, 78 functions, exact generated output, and failure-preservation cases; fixed measured text allocation behavior and checked three document sizes.
- [x] Verified all nine matched applications against 127 frozen cases per language, then recorded output-checked, randomized runtime, cache, package, setup, and memory comparisons.
- [x] Built and verified the final macOS ARM64 package. The current Linux ARM64 and x86_64 target matrices are assigned to the PR CI chain; they are not claimed as locally verified.
- [x] Used same-process and repeated Richards comparisons to support the small-list-descriptor allocation fix; reran full workspace tests, five short-source comparisons, AllHereHTML, memory guard, and lint/format checks. The 0.60 Richards stretch target was not reached.
- [x] Ran three sequential, isolated, post-use agent evaluations. All chose Guion; the small, history-exposed sample is exploratory, not a general adoption result.
- [x] Updated product and agent documentation, generated references, CI regression gates, and retained measurement evidence. The final workspace log and local install archive are saved.
- [ ] Complete the sequential macOS ARM64 → Debian ARM64 → Debian x86_64 CI target matrix and ordinary PR checks; resolve any failures and attach final CI evidence before treating cross-platform validation as complete.

**Latest local verification:** `cargo test --workspace --release --locked --offline` — 359 passed, 0 failed across 36 harness invocations; `cargo clippy --workspace --all-targets --release --locked --offline -- -D warnings`; `cargo fmt --all -- --check`; the performance harness (34 tests), first-source harness (5 tests), generated agent reference, switch-condition ledger, AllHereHTML gate, and Node WebAssembly smoke all passed. See `evaluation/completion/results/real-task-sprint/verification.json` and the saved `workspace-tests.log`.
