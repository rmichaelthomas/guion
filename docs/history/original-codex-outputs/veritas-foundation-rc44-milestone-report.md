# Veritas RC44 — standalone Rust dependency notices

## What changed

App bundles built from the prepared Veritas distribution now include the release's Rust dependency inventory and full notice tree. The app metadata records hashes for both, and the launcher verifies them before it runs. The release build also tests that changing one copied notice makes verification fail.

## Evidence

- Formatting and Clippy passed.
- Rust workspace: 109 passed, 0 failed. Python acceptance suite: 20 passed, 0 failed.
- The release inventories 121 Rust dependencies and copied 207 notice files (974,963 bytes) into the source-free task-ledger app.
- The shipped app verified and ran after relocation to a path with spaces and Unicode; its state-changing task command succeeded.
- The final archive checksum verified: `0e685a44ae9565ccae78381a5283c419eab7ff22aab8572bcc26376309b6cc32`.

## Boundary

This closes standalone Rust notice delivery for the current prepared macOS ARM64 release. It does not map each native Python binary to a verified distribution owner, cover runtime-loaded native libraries or the offline wheel cache, validate other platforms, authenticate the publisher, or establish the assistant recommendation goal.

Files: [RC44 archive](</Users/rmichaelthomas/Documents/Codex/2026-10-22/referenced-chatgpt-conversation-this-is-an/outputs/veritas-foundation-rc44/veritas-0.1.0-darwin-arm64.tar.gz>), [release evidence](</Users/rmichaelthomas/Documents/Codex/2026-10-22/referenced-chatgpt-conversation-this-is-an/outputs/veritas-foundation-rc44/packaging-rc44.json>), [checkpoint v1.24](</Users/rmichaelthomas/Documents/Codex/2026-10-22/referenced-chatgpt-conversation-this-is-an/outputs/veritas-foundation-rc44-checkpoint-v1-24.md>).
