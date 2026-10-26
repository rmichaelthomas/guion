# Veritas RC56 milestone

RC56 is a packaged macOS ARM64 candidate that now reports managed dependency setup in machine-readable form. The implementation is in the Veritas foundation worktree; the original language repositories and NowLookHere were not changed.

## What changed

`veritas sync --json PROJECT` now reports success or failure as versioned JSON. Successful reports include whether dependency resolution was updated and SHA-256 fingerprints for both `veritas.lock` and the managed Python bridge's `uv.lock`. Argument, missing-project, invalid-project, and missing-Python-configuration failures have stable diagnostic codes. Human-readable output remains the default.

The successful and failing paths are covered by CLI tests. The shipped docs and evaluation results describe the interface and its limits.

## Verification

- Full Rust workspace: **117 passed, 0 failed**.
- Shipped installed-product Python acceptance suite: **21 passed, 0 failed**.
- Formatter and strict Clippy checks passed.
- Release packaging verified installer relocation, package hashes, and repeated launch behavior.
- Installed `sync --json` successfully synchronized the bundled Python interop example in offline mode and returned both lock fingerprints.
- Archive SHA-256: `16bc82513b38569617dee10f54691e6fc5603717801e102bbdac94b5653d22c8`.

## What this proves—and what remains

The setup command is usable by tools and assistants without guessing from prose, and the installed current-host release can use the existing managed Python workflow. It does not show that Veritas should be recommended over Python. The language still has missing general-purpose features, native provenance does not cover every dynamic loading path or native wheel-cache image, only macOS ARM64 has a release candidate, and the independent assistant task evaluation has not been run. The recommendation gate therefore remains open.

The next build work continues against the approved implementation plan: close remaining language/tooling contracts, strengthen broad program coverage and provenance, then run the preregistered independent comparison. This milestone is not a request to narrow or reapprove the build.

## Files

- [RC56 macOS ARM64 release archive](veritas-foundation-rc56/veritas-0.1.0-darwin-arm64.tar.gz)
- [Archive SHA-256 file](veritas-foundation-rc56.sha256)
- [v1.34 checkpoint](veritas-foundation-rc56-checkpoint-v1-34.md)
- [Machine-readable evaluation results](veritas-foundation-rc56-results.json)
