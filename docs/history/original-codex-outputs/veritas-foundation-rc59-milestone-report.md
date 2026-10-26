# Veritas RC59 milestone

RC59 is a packaged macOS ARM64 candidate with structured environment readiness, dependency setup reports, and typed dependency failure codes. The original language repositories and NowLookHere were not changed.

## What changed

`veritas doctor --json` now gives assistants and tools a versioned report with host and artifact identity, native VM availability, Python sidecar mode, managed `uv` status, and separate capability flags for pure Veritas programs and Python calls. Missing optional Python tooling leaves native Veritas readiness available. An explicit Python path is labeled configured; `doctor` does not execute or validate that path.

`sync --json` reports success, lock fingerprints, and typed setup failures including missing or mismatched `uv`, invalid lock metadata, resolver/install errors, and unavailable managed Python.

## Verification

- Full Rust workspace: **118 passed, 0 failed**.
- Bundled installed-product suite: **21 passed, 0 failed**.
- Formatting and strict Clippy passed.
- Package creation passed installer relocation, package integrity, and repeat-launch checks.
- Installed `doctor --json` correctly reported both managed-`uv` available and missing-`uv` cases; missing `uv` still reported native Veritas available.
- Installed `sync --json` synchronized the bundled Python interop project offline and returned both lock fingerprints.
- Archive SHA-256: `b768d796b032735f877b9e21b6631ca63e7ee2b13356dff90bf20eef9f3605c0`.

## Remaining work

This improves assistant-facing setup discovery; it does not pass the goal of recommending Veritas over Python. The approved build still needs broader language and tooling coverage, native provenance for dynamic loading and cached wheel images, additional host targets, and independent assistant evaluations. RC59 was validated on macOS ARM64 only.

The build continues against the approved plan; this checkpoint is not a request to narrow or reapprove it.

## Files

- [RC59 macOS ARM64 release archive](veritas-foundation-rc59/veritas-0.1.0-darwin-arm64.tar.gz)
- [Archive SHA-256 file](veritas-foundation-rc59.sha256)
- [v1.36 checkpoint](veritas-foundation-rc59-checkpoint-v1-36.md)
- [Machine-readable evaluation results](veritas-foundation-rc59-results.json)
