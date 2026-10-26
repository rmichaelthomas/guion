# Veritas foundation RC48 — offline Python cache integrity

**Status:** Built and verified on macOS ARM64. This is an implementation milestone, not evidence that Veritas is ready to replace or be recommended over Python generally.

## What changed

Python-assisted packages now use `PACKAGE.json` schema 2 to record the SHA-256 digest and size of immutable uv offline-cache payloads. The verifier checks that inventory before launch and rejects altered or added payload files. Since uv mutates lock, HTTP and msgpack bookkeeping during a normal offline launch, those records are excluded. Package schema 1 remains verifiable.

## Verification

- Rust workspace: **113 passed, 0 failed**.
- Python acceptance/parity suite: **20 passed, 0 failed**.
- `cargo fmt --all --check` and strict workspace Clippy: passed.
- Native-wheel integration: package relocated to a path with spaces and Unicode; app launched twice offline without source files or external Python setup; altered cache rejected with `offline Python cache integrity check failed`.
- Installed-release acceptance: installed under a path with spaces and Unicode; installed CLI verified the new schema-2 package; the app ran twice with an empty inherited environment; cache tampering was rejected.
- Legacy compatibility: the new verifier accepted the retained RC47 schema-1 Python package.
- Archive SHA-256: `9a55943f59903608170654e630502073146bdbc27033e982ded17b0d002fc5f9`.

## Limits

This check detects corruption; there is no publisher signature. Mutable uv metadata is outside the hash, and native binaries stored only in the offline cache remain outside the Mach-O audit. The release was exercised on macOS ARM64 only. Cross-platform packaging, dynamic loader discovery, broad independent workload evaluation, and the assistant recommendation gate are still open.

## Artifacts

- Release directory: [`veritas-foundation-rc48`](/Users/rmichaelthomas/Documents/Codex/2026-10-22/referenced-chatgpt-conversation-this-is-an/outputs/veritas-foundation-rc48)
- Archive: [`veritas-0.1.0-darwin-arm64.tar.gz`](/Users/rmichaelthomas/Documents/Codex/2026-10-22/referenced-chatgpt-conversation-this-is-an/outputs/veritas-foundation-rc48/veritas-0.1.0-darwin-arm64.tar.gz)
- Machine-readable result: [`veritas-foundation-rc48-results.json`](/Users/rmichaelthomas/Documents/Codex/2026-10-22/referenced-chatgpt-conversation-this-is-an/outputs/veritas-foundation-rc48-results.json)
- Source checkpoint: [`veritas-foundation-rc48-checkpoint-v1-28.md`](/Users/rmichaelthomas/Documents/Codex/2026-10-22/referenced-chatgpt-conversation-this-is-an/outputs/veritas-foundation-rc48-checkpoint-v1-28.md)
