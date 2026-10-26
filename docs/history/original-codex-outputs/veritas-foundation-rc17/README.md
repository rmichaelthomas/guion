# Veritas foundation build — RC17

This is an intermediate macOS ARM64 evaluation build from the approved Veritas foundation plan. It is a real compiler/runtime/toolchain package, but it does **not** yet establish that an assistant should recommend Veritas over Python generally or for a validated task class.

## Package

- Archive: `veritas-0.1.0-darwin-arm64.tar.gz`
- SHA-256: `32a06a31387498c7098d9d9e39616e89cef151e34786da93615c0243d07039ba`
- Checksum file: `veritas-0.1.0-darwin-arm64.tar.gz.sha256`
- Target verified: macOS ARM64
- Included runtime/tooling: Veritas CLI and version-10 bytecode VM, local language/reference docs, examples, pinned uv 0.13.4 and its MIT/Apache-2.0 notices.

## New in this build

Typed immutable `Map[Text, T]` values now support non-empty homogeneous literals, key lookup, `map_has`, immutable `map_set`, length, sorted-key iteration, typed function arguments/returns, version-10 artifact serialization, and typed conversion to/from Python dictionaries. Duplicate literal keys fail during compilation; missing lookups report a source-traced runtime error.

Empty-map construction is not implemented. Python map conversion requires string keys and a value type supported by the current bridge. Records, floating point, module imports, general `Option`/`Result` matching, HTTP, broader resource accounting, relocatable Python environments, other OS targets, and independent recommendation evaluations remain open.

## Verification

- Rust workspace: 55 tests passed, including the pinned offline uv integration with managed CPython 3.14.8, `requests==2.32.5`, and compiled `orjson==3.13.0`.
- `cargo fmt --all -- --check` passed.
- `cargo clippy --workspace --all-targets --offline -- -D warnings` passed.
- Evaluation harness: 2 Python tests passed.
- Extracted package: checksum verified; `doctor` passed; included Map example printed `8`; a version-10 Map artifact ran after its source copy was deleted and printed `8`.
- The packaged Python interoperability project synced offline and ran with the bundled uv, printing `76` and `python-requests/2.32.5`.

This archive is an unsigned local evaluation package, not a public installer. The complete build source remains in the workspace at `work/veritas-foundation/`; the linked [build checkpoint](veritas_checkpoint_v1_1_foundation_build.md) records the north star, evidence, known limitations, and continuation path.
