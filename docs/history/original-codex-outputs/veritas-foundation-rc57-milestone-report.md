# Veritas RC57 milestone

RC57 is a packaged macOS ARM64 candidate with machine-readable dependency setup reports and typed failure categories. The build stays in the Veritas foundation worktree; the original language repositories and NowLookHere were not changed.

## What changed

`veritas sync --json PROJECT` reports success or failure as versioned JSON. Success includes whether resolution changed and SHA-256 fingerprints of both `veritas.lock` and the managed Python bridge's `uv.lock`. Setup errors now carry typed codes instead of being classified by searching the diagnostic text. Codes distinguish configuration errors, missing or mismatched `uv`, invalid lock metadata, resolution failures, installation failures, and unavailable managed Python. The default human-readable output remains intact.

## Verification

- Rust workspace: **117 passed, 0 failed**.
- Installed acceptance suite: **21 passed, 0 failed**.
- Formatter, strict Clippy, installer relocation, package integrity, and repeat-launch checks passed.
- The installed CLI synchronized its bundled Python interop project in offline mode and returned both lock fingerprints.
- A missing-`uv` check against the packaged CLI returned `E_UV_NOT_FOUND` and left no partial environment.
- Archive SHA-256: `e16215fb7abb3b2fc35544608b5318141fe34667dd451d331e56931d2b64bc7c`.

## Remaining work

This advances assistant-facing setup and failure reporting; it does not pass the “recommend Veritas over Python” gate. The approved build still needs wider language and tooling coverage, native provenance for dynamic loading and cached wheel images, additional host targets, and independent assistant evaluations. RC57 was validated on macOS ARM64 only.

The build continues against the approved plan; this checkpoint is not a request to narrow or reapprove it.

## Files

- [RC57 macOS ARM64 release archive](veritas-foundation-rc57/veritas-0.1.0-darwin-arm64.tar.gz)
- [Archive SHA-256 file](veritas-foundation-rc57.sha256)
- [v1.35 checkpoint](veritas-foundation-rc57-checkpoint-v1-35.md)
- [Machine-readable evaluation results](veritas-foundation-rc57-results.json)
