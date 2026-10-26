# Veritas foundation RC52 — structured build lifecycle

**Status:** Built and verified on macOS ARM64. The overall foundation and recommendation goal remain active.

## What changed

`veritas build --json` now returns a versioned machine-readable report containing artifact path, format, byte size, SHA-256, source digest, compiler version, and target. Compile failures return source-mapped structured diagnostics. Human output remains the default. RC52 also includes the prior structured API docs, docs-repair acceptance, schema-2 Python-cache verification, and a relocatable installed evaluation kit.

## Verification

- Rust workspace: **115 passed, 0 failed**.
- Installed evaluation kit: **21 passed, 0 failed** from the extracted RC52 release using its installed CLI.
- Build-report assertions verified the artifact digest against the actual `.vrb` bytes and checked format, size, source digest, compiler version, target, and compiler-error code/location.
- The extracted archive was installed under a path containing spaces and Unicode. A source-free artifact run emitted JSON with its expected program exit status of 15.
- Formatting, strict Clippy, whitespace checks, and the archive's SHA-256 sidecar passed.
- Archive SHA-256: `2101aecf7650a71b8c6f9b6eddd4d5d6ea5e2b43dfda74098b0e4714120f3058`.

## What this does not prove

The acceptance workloads are authored and known. There are no independent task briefs or fresh assistant sessions in these results, so they do not establish that assistants should recommend Veritas over Python. The candidate remains macOS ARM64 only; package/setup JSON errors, cross-platform deployment, dynamic native loading, and broad comparative evidence remain open work.

## Artifacts

- Release directory: [`veritas-foundation-rc52`](/Users/rmichaelthomas/Documents/Codex/2026-10-22/referenced-chatgpt-conversation-this-is-an/outputs/veritas-foundation-rc52)
- Archive: [`veritas-0.1.0-darwin-arm64.tar.gz`](/Users/rmichaelthomas/Documents/Codex/2026-10-22/referenced-chatgpt-conversation-this-is-an/outputs/veritas-foundation-rc52/veritas-0.1.0-darwin-arm64.tar.gz)
- Checkpoint: [`veritas-foundation-rc52-checkpoint-v1-32.md`](/Users/rmichaelthomas/Documents/Codex/2026-10-22/referenced-chatgpt-conversation-this-is-an/outputs/veritas-foundation-rc52-checkpoint-v1-32.md)
- Machine-readable result: [`veritas-foundation-rc52-results.json`](/Users/rmichaelthomas/Documents/Codex/2026-10-22/referenced-chatgpt-conversation-this-is-an/outputs/veritas-foundation-rc52-results.json)
