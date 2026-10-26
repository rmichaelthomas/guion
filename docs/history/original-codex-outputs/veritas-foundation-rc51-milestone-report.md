# Veritas foundation RC51 — installed evaluation kit

**Status:** Built and verified on macOS ARM64. The current candidate remains experimental; the recommendation gate is open.

## What changed

The release now carries an evaluation suite that can run from the installed release root. Tests resolve known workload projects from the bundled examples tree when the source checkout is absent. The release includes the benchmark helper imported by those tests, and the evaluation README documents the installed invocation. The suite also includes a repair test that reads only shipped documentation, parses an assistant-facing diagnostic, fixes a typo, and checks/runs/tests the program.

## Evidence

- The first installed RC50 suite run failed with 18 errors because it expected source-checkout paths and a helper module omitted from the release. The failure was not treated as a pass.
- After fixing path resolution and including the helper, the RC51 installed release suite passed **21/21** Python acceptance tests from the installed release root, using the installed CLI and packaged workload examples.
- Fresh Rust workspace run: **114 passed, 0 failed**. Formatting, strict Clippy, and whitespace checks passed.
- The installer path included spaces and Unicode. The installed suite covered shipped-doc repair, known task-ledger/feed parity, dependency packaging and Python interop.
- Archive SHA-256: `Archive checksum is recorded in the adjacent `.sha256` file.`.

## Limits and remaining work

These acceptance cases are known author-run workloads. They do not substitute for independent task briefs or fresh assistant sessions. Veritas has not earned a general recommendation over Python. Only macOS ARM64 was exercised; cross-platform deployment, broader comparative evaluation, native libraries loaded dynamically, and independent recommendation evidence remain open.

## Artifacts

- Release directory: [`veritas-foundation-rc51`](/Users/rmichaelthomas/Documents/Codex/2026-10-22/referenced-chatgpt-conversation-this-is-an/outputs/veritas-foundation-rc51)
- Archive: [`veritas-0.1.0-darwin-arm64.tar.gz`](/Users/rmichaelthomas/Documents/Codex/2026-10-22/referenced-chatgpt-conversation-this-is-an/outputs/veritas-foundation-rc51/veritas-0.1.0-darwin-arm64.tar.gz)
- Checkpoint: [`veritas-foundation-rc51-checkpoint-v1-31.md`](/Users/rmichaelthomas/Documents/Codex/2026-10-22/referenced-chatgpt-conversation-this-is-an/outputs/veritas-foundation-rc51-checkpoint-v1-31.md)
- Machine-readable result: [`veritas-foundation-rc51-results.json`](/Users/rmichaelthomas/Documents/Codex/2026-10-22/referenced-chatgpt-conversation-this-is-an/outputs/veritas-foundation-rc51-results.json)
