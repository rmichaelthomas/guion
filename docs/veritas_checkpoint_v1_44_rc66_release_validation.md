# Veritas checkpoint v1.44 — RC66 release validation

Status: RC66 is a validated macOS arm64 development release candidate. This closes release assembly and local product acceptance for the approved foundation milestone. It does not pass the independent recommendation gate or claim general-purpose replacement of Python.

## Build and shipped tests

- Release: `veritas-0.1.0-darwin-arm64.tar.gz` (RC66), 75,795,559 bytes, 5,642 archive entries.
- SHA-256: `c23d8f72d69236c1249a0b5f0885c0397e82fffdffa99098718c21fd8db7c8b2`.
- Archive inspection found zero AppleDouble entries and zero absolute symlink targets.
- The installed acceptance suite from the bundled evaluation kit passed 23/23 tests on macOS arm64. It covers parity with Python, bounded concurrent loopback HTTP, source-free artifacts, package/relocation behavior, Python ABI behavior, diagnostics repair and the shipped compatibility guide.
- Workspace verification from the preceding RC65 code build passed 125 Rust/Python tests, formatting, strict Clippy, and `git diff --check`. RC66 adds the v1.43 policy documentation and its installed acceptance check; no Rust implementation changed after that workspace run.

## Release-boundary exercises

- Extracted RC66 under a path containing spaces and Unicode. Bundled `doctor --json` reported macOS/aarch64, native bytecode VM, and managed uv 0.13.4. Two app runs produced identical output: `15`, `9007199254741000`, `3`, `python-requests/2.32.5`.
- A newly created task-ledger package reported schema 2, passed `verify-package --json`, and launched from a path containing spaces and Unicode; `run list` returned `No tasks.`
- From the relocated bundled CLI, a zero-dependency Python bridge project completed `sync --update --json` offline using the bundled pinned uv and managed CPython, then ran successfully.
- RC66's macOS test suite compiles a C dynamic library, loads it through Python `ctypes`, and checks that `run --json` reports its resident path after both successful and failed Python calls. The package integration test verifies NumPy native files and their wheel `RECORD` owner in the offline cache. The relevant tests passed in the 125-test workspace run.
- The RC66 shipped bundle's Python interop example reports 34 active native images and 22 cached native images with zero unresolved load edges.

## Boundaries and remaining work

- This validates one host: macOS arm64. No Linux, Windows, or macOS x86_64 claim follows.
- Python native-image reporting is a dyld boundary snapshot; a library loaded and unloaded entirely within one bridge call can be missed.
- Dependency update stages and rolls back on ordinary command failures, but a machine crash between replacing the bridge environment and lock metadata is not transactionally recoverable; locked execution rejects a mismatch.
- The package is unsigned. SHA-256 reports detect accidental changes but do not authenticate a publisher.
- Application-specific persistent data migration remains the application's responsibility.
- The assistant recommendation goal is still unproven. Independent unseen tasks/builders, a frozen comparative protocol, a fair current-Python workflow, and multi-class outcomes remain required before recommending Veritas over Python.

## Next build phase

Proceed to the comparative recommendation gate without pausing for another concept checkpoint: freeze a preregistered multi-class task protocol; prepare equal-entry workflows for Veritas and current Python; collect cold and warm time, correctness, deployment, maintenance and intervention costs; then run unseen briefs when independent builder sessions are available. If those sessions are unavailable, publish the evaluation kit and author-run evidence with that limitation. In parallel, prioritize additional target support and relocation/error-path validation, not feature count for its own sake.
