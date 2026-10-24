# Veritas checkpoint v1.33 — structured package reports

## Change

`veritas package --json PROJECT [--output DIRECTORY]` emits a versioned `veritas_package_report` with package directory, package schema version, target, artifact digest, package-metadata digest, resource count, and Python-runtime presence. Packaging failures return `veritas_diagnostic` JSON with stable package-level codes. Source compilation failures preserve their compiler diagnostic code, source, line, and column.

`veritas verify-package --json DIRECTORY` emits a versioned verification report containing package schema, target, directory, and metadata digest. Integrity failures return a nonzero exit and a structured `E_PACKAGE_VERIFY_FAILED` diagnostic. Human-readable output remains the default for both commands.

## Evidence

- The integration test first failed because `package --json` was treated as the project directory.
- The test verifies a package report against the actual `PACKAGE.json` and its digest, verifies a valid package, tampers with the artifact and observes structured rejection, checks existing-output and missing-project errors, and confirms a source compile error retains `E_NAME_UNKNOWN` and its line number.
- The package-report fixture supplies an explicit manifest entry; omitting it correctly produced the existing manifest-validation error during test development.

## Limits and next work

Dependency synchronization, installer, archive, and general filesystem failures do not yet all use structured JSON. A package report describes one current-host package; it does not authenticate the publisher. Cross-platform packaging and independent recommendation evidence remain open.
