# Veritas checkpoint v1.34 — structured dependency setup reports

Status: implemented and validated in the RC56 installed release.

## Delivered

- `veritas sync --json PROJECT` and `veritas sync --update --json PROJECT` emit a versioned JSON report to stdout.
- Successful setup reports include the canonical project path, whether resolution was updated, a digest of `veritas.lock`, and a digest of the bridge `uv.lock`.
- Argument, missing-project, invalid-project, missing-Python-configuration, and dependency setup failures have machine-readable codes and messages.
- Default human-readable `sync` output remains unchanged.
- CLI tests cover argument errors, missing projects, projects without managed Python configuration, and the successful managed-Python sync report.

## Validation

`cargo fmt --all --check`, strict Clippy, and the full offline Rust workspace suite pass. The workspace reports 117 passed tests and 0 failures. All 21 Python acceptance tests pass from the packaged RC56 release. The release script also verifies installer relocation, package integrity, and repeat launches. The installed `sync --json` command succeeded against the bundled Python interop example in offline mode and returned both lock fingerprints.

## Limits

The report fingerprints lock files, but does not prove artifact provenance or publisher identity. Resolver diagnostics currently use a stable general setup-failure code except for argument, project and configuration errors. Independent assistant evaluations and non-macOS targets remain outstanding.
