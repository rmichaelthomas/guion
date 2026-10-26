# Veritas foundation RC49 — assistant-readable API surface

**Status:** Built and verified on macOS ARM64. This improves one assistant workflow; it does not establish that Veritas should be recommended over Python generally.

## What changed

`veritas doc --json FILE.vr` now emits a versioned API report with function parameter types and results, record fields, and declared Python module/callable signatures. The prior human-readable `doc` output remains unchanged. RC49 also carries the RC48 schema-2 offline cache integrity work.

## Evidence

- Rust workspace: **114 passed, 0 failed**.
- Python acceptance/parity: **20 passed, 0 failed**.
- Formatting and strict workspace Clippy: passed.
- The new CLI test first failed because `--json` was misread as a filename, then passed after the implementation; it checks report kind/version, typed functions, records, Python parameter names, and retained human output.
- Current-host archive package and installer checks passed. Installed `doc --json` parsed against shipped source examples, the installed CLI verified its schema-2 Python bundle, and the bundled Python app ran twice offline with an empty inherited environment.
- Archive SHA-256: `3d5ec692e01d20aa093ef26a4ab29870d08c8cebfcff88c5d821d2f3aad8618f`.

## Limits and next build work

The bytecode artifact does not retain Veritas source parameter names, so those report entries contain ordered types only. Documentation errors are still text-based, and the report does not index a full package. Next in the approved build plan: make shipped docs and examples support a complete assistant-led error repair loop, including a test that uses only the installed materials. The broad independent task evaluation and recommendation gate remain open.

## Artifacts

- Release directory: [`veritas-foundation-rc49`](/Users/rmichaelthomas/Documents/Codex/2026-10-22/referenced-chatgpt-conversation-this-is-an/outputs/veritas-foundation-rc49)
- Archive: [`veritas-0.1.0-darwin-arm64.tar.gz`](/Users/rmichaelthomas/Documents/Codex/2026-10-22/referenced-chatgpt-conversation-this-is-an/outputs/veritas-foundation-rc49/veritas-0.1.0-darwin-arm64.tar.gz)
- Checkpoint: [`veritas-foundation-rc49-checkpoint-v1-29.md`](/Users/rmichaelthomas/Documents/Codex/2026-10-22/referenced-chatgpt-conversation-this-is-an/outputs/veritas-foundation-rc49-checkpoint-v1-29.md)
- Machine-readable result: [`veritas-foundation-rc49-results.json`](/Users/rmichaelthomas/Documents/Codex/2026-10-22/referenced-chatgpt-conversation-this-is-an/outputs/veritas-foundation-rc49-results.json)
