# Veritas RC47 — assistant-readable checks

RC47 adds an opt-in JSON interface to `veritas check`, `veritas test`, and `veritas run`. This lets an editor or assistant read compiler errors, test failures, program output, exit status, and runtime failures as structured data, follow source locations, and decide whether to repair or continue without parsing human-formatted terminal text. Human-readable output remains the default.

## Verified

- Formatting and strict Clippy passed.
- Rust workspace: 113 passed, 0 failed. Python acceptance: 20 passed, 0 failed.
- New integration tests first failed because the JSON report behavior was missing; they now pass for successful and failing checks and test runs.
- Check diagnostics include a stable code, source, line and column. Test reports include ordered test results, counts, stable runtime error codes and source traces. Test and compiler failures return status 1 while leaving stderr empty for these cases.
- The final archive checksum passed. After extraction and install into a path containing spaces and Unicode, the shipped CLI emitted valid JSON for successful check, successful tests, a successful program returning exit code 23, and a runtime failure after partial output. The installed CLI also verified and ran its bundled Python interop app twice offline with identical output.

## What this does not establish

JSON output currently covers `check`, `test`, and `run`; `build` and setup failures still use text. The report schema has not had external tooling review. The release remains unsigned and macOS ARM64-only, and the recommendation evaluation remains open. These improvements make Veritas easier for assistants to operate; they do not yet show an assistant should recommend it over Python.

Archive SHA-256: `ae9a41f9b24873ceff8f249589b4c7adf0f7485bf90bce3f9f56635338f9a6ed`.

Files: [installable RC47 archive](</Users/rmichaelthomas/Documents/Codex/2026-10-22/referenced-chatgpt-conversation-this-is-an/outputs/veritas-foundation-rc47/veritas-0.1.0-darwin-arm64.tar.gz>), [machine-readable evaluation](</Users/rmichaelthomas/Documents/Codex/2026-10-22/referenced-chatgpt-conversation-this-is-an/outputs/veritas-foundation-rc47/assistant-cli-rc47.json>), [checkpoint v1.27](</Users/rmichaelthomas/Documents/Codex/2026-10-22/referenced-chatgpt-conversation-this-is-an/outputs/veritas-foundation-rc47-checkpoint-v1-27.md>).
