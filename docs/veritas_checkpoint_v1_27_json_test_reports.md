# Veritas checkpoint v1.27 — machine-readable test reports

## Change

`veritas test FILE.vr --json` emits one JSON object on stdout. Schema version 1 identifies the report kind, input source, total/passed/failed counts, and ordered test results. Runtime failures include a stable diagnostic code, message and source-mapped trace. Invalid test signatures are reported with `E_TEST_SIGNATURE`. The process exits successfully when all tests pass and with status 1 when any fail; human-readable output remains the default when `--json` is omitted.

`veritas check FILE.vr --json` emits either a versioned check-success report or a `veritas_diagnostic` object with the stable compiler code, message, source, line, and column. Compiler failures return status 1 and do not write human text to stderr.

`veritas run --json FILE.vr [ARGS...]` puts the flag before the path so all following arguments remain program arguments. Its versioned report captures program output, host-tool stderr, and the program's exit code. Compiler and runtime failures include structured diagnostics and return status 1. The default `run FILE.vr [ARGS...]` still streams program output and maps `main`'s `Int` return to the process exit status.

This gives editors and assistants structured compiler, test, and run results they can act on without scraping terminal text. File lookup, project setup, and package/build failures remain human-readable text.

## Evidence

- Integration tests first failed because the flag was not implemented; after implementation they parse valid JSON for successful and failing check, test, and run executions.
- Tests confirm schema versions, ordered test names, counts, captured program output, process status, stable assertion/compiler/runtime codes, source paths and line/column, empty stderr, and nonzero failure statuses.
- `cargo fmt --all --check` and strict workspace Clippy passed for the combined RC47 candidate.
- Rust workspace: 113 passed, 0 failed. Python acceptance: 20 passed, 0 failed.

## Limits

The JSON schema is versioned but has not yet been frozen through an external tooling review. File lookup, `build`, and project setup failures remain human-readable text. The assistant recommendation evaluation remains open.
