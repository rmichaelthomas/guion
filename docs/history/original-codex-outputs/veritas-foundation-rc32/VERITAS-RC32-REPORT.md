# Veritas RC32 build report

**Verdict:** the foundation is feasible and fixable, and this build closes two concrete gaps that prevented an ordinary persistent CLI app. The assistant-recommendation goal is still unproven. RC32 is an experimental macOS ARM64 evaluation candidate, not a general Python replacement.

## What changed

- Artifact format 20 adds typed JSON constructors and immutable array/object updates. Programs can now create structured data and persist it without a Python writer.
- The CLI now maps `main() -> Int` to the process exit status and treats `main() -> Unit` as success.
- Added `task_ledger`, a multi-module local app written entirely in Veritas. It adds, lists, completes, removes, and summarizes tasks stored in `tasks.json`.
- Added an independent standard-library Python baseline. Tests compare output, stderr, exit status, and exact persisted bytes over commands executed as separate processes.
- Added the app and its documentation to the current-host package.

The integration pass found an actual CLI semantics gap: entry-point integer results were ignored. The Python parity test also exposed why that matters: Veritas printed the expected duplicate-task error but exited successfully. Propagating the integer result fixed the behavior. JSON construction has direct runtime tests for exact large integers, Unicode, wrong container kinds, resource ceilings, serialization round trips, and rejection under old artifact formats.

## Verification

- Rust workspace: **92 passed, 0 failed** across CLI, dependency, compiler, model/artifact, runtime, HTTP and Python bridge suites.
- `cargo fmt --all -- --check`: passed.
- `cargo clippy --offline --workspace --all-targets -- -D warnings`: passed.
- Task-ledger acceptance against development CLI: **3 passed**.
- RC32 extracted into a fresh temporary directory; packaged `veritas doctor` passed; packaged task-ledger acceptance: **3 passed**.
- Package SHA-256: `166a1f2ec05ebc552ffff38541840a926d3c8131c418f0077464096a6549cda8`.

The feed-digest suite did not reach its program checks in this session. The local cache no longer contains the locked managed CPython 3.14.8, and the locked offline setup stops before application execution. RC31's earlier six-case feed-digest result is retained as RC31 evidence only. RC32 therefore has not been re-verified against that Python-assisted workload.

## Limits and next work

- File writes are direct and non-transactional. Concurrent app writers can overwrite each other, and an interrupted write can leave partial state. Add an atomic/transactional file-update capability before using this pattern for important or concurrent data.
- The persistent app is a known development workload. Its Python parity result checks correctness and portability of the sample; it does not establish a Veritas advantage.
- Only macOS ARM64 is packaged. RC32 is unsigned and not notarized. Fresh extraction on this same host is not clean-machine or cross-platform evidence.
- Python-assisted package setup still requires obtaining the pinned CPython and wheels. The current warm-cache feed test could not be repeated here.
- No independent brief, builder session, recommendation evaluation, multi-class developer study, or general speed claim was produced.

The build supports continuing the project. It does not yet justify an assistant recommending Veritas over Python. The nearest next implementation gap for this app class is safe atomic state replacement; the broader plan still needs application diversity, deployment work and independent comparative evaluation.

## Files

- Package: [veritas-0.1.0-darwin-arm64.tar.gz](veritas-0.1.0-darwin-arm64.tar.gz)
- Package checksum: [veritas-0.1.0-darwin-arm64.tar.gz.sha256](veritas-0.1.0-darwin-arm64.tar.gz.sha256)
- Executable package directory: [veritas-0.1.0-darwin-arm64](veritas-0.1.0-darwin-arm64/)
- Machine-readable acceptance record: [task-ledger-acceptance.json](task-ledger-acceptance.json)
- Source checkpoint: [veritas_checkpoint_v1_14_json_state.md](veritas_checkpoint_v1_14_json_state.md)
