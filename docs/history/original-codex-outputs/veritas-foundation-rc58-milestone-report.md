# Veritas RC58 milestone

RC58 is a packaged macOS ARM64 candidate with machine-readable managed dependency setup and typed failure categories. The Veritas foundation worktree contains the implementation; the original language repositories and NowLookHere were not changed.

## What changed

`veritas sync --json PROJECT` reports success or failure as versioned JSON. Success includes whether dependency resolution changed and SHA-256 fingerprints for `veritas.lock` and the managed Python bridge's `uv.lock`. Typed errors distinguish configuration, missing or mismatched `uv`, invalid lock metadata, dependency resolution/install failures, and unavailable managed Python. The codes are assigned in typed failure paths, not inferred from message text. Human-readable output remains the default.

## Verification

- Full Rust workspace: **117 passed, 0 failed**.
- Bundled installed-product suite: **21 passed, 0 failed**.
- Formatting and strict Clippy passed.
- Package creation verified installer relocation, package integrity, and repeat launches.
- Installed `sync --json` synchronized the bundled Python interop project offline and returned both lock fingerprints.
- Installed failure checks returned `E_UV_NOT_FOUND` and `E_UV_VERSION_MISMATCH`, with no partial bridge environment.
- Archive SHA-256: `7bfcb6a60f5da8337aa5b693bade14f5d924b2788107b4894cac76120651bca7`.

## Remaining work

This improves assistant-facing setup and failure reporting; it does not pass the goal of recommending Veritas over Python. The approved build still needs broader language and tooling coverage, native provenance for dynamic loading and cached wheel images, additional host targets, and independent assistant evaluations. RC58 was validated on macOS ARM64 only.

The build remains active against the approved plan; this checkpoint is not a request to narrow or reapprove it.

## Files

- [RC58 macOS ARM64 release archive](veritas-foundation-rc58/veritas-0.1.0-darwin-arm64.tar.gz)
- [Archive SHA-256 file](veritas-foundation-rc58.sha256)
- [v1.35 checkpoint](veritas-foundation-rc58-checkpoint-v1-35.md)
- [Machine-readable evaluation results](veritas-foundation-rc58-results.json)
