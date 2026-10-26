# Veritas RC69 — evaluation-ready release

**RC69 is packaged and passes all 31 tests in its installed acceptance kit on macOS arm64.** It includes the first usable comparison ledger for the assistant recommendation goal, while correctly leaving the recommendation verdict open because no independent runs exist yet.

## What passed

- 31/31 installed acceptance tests, covering Python parity, loopback concurrency, source-free bytecode, package relocation, diagnostics repair, compatibility guidance, and the evaluation ledger.
- Extracted under a fresh path containing spaces and Unicode: `doctor` passed, the task-ledger bundle verified and launched, and offline dependency `sync --update` plus a subsequent run passed.
- The comparison ledger tests passed 8/8. It rejects incomplete or mismatched pairs, reused model sessions, unequal hardware/budgets, missing failure reasons, and malformed acceptance counts. It never chooses a winning language.
- RC69 archive validation found 5,647 entries, zero AppleDouble entries, and zero absolute symlink targets. Its Python interop bundle records 34 active and 22 cached native images with no unresolved load edges.
- Existing runtime integration evidence covers C dynamic-library loading through Python `ctypes`, native-image reporting after successful and failed Python calls, and NumPy wheel-cache ownership.

## Downloads

- [RC69 macOS arm64 release archive](veritas-foundation-rc69/veritas-0.1.0-darwin-arm64.tar.gz)
- [SHA-256 file](veritas-foundation-rc69/veritas-0.1.0-darwin-arm64.tar.gz.sha256)
- [Source tree archive](veritas-foundation-source-rc69.tar.gz)
- [Machine-readable results](veritas-foundation-rc69-results.json)
- [v1.45 checkpoint](veritas-foundation-rc69-checkpoint-v1-45.md)

Archive SHA-256: `5b7721afceb2760d6c068fd35aa288f93753e18efae42df1d3b253cc14530f18`

## What this means for the north star

The build now has an evaluation kit that can reject unfair or incomplete comparisons and preserve failures. Running it on an empty ledger reports `insufficient_independent_evidence`. Veritas has not earned an assistant recommendation over Python yet; the next required evidence is the preregistered independent cohort across three task classes. Host support also remains macOS arm64 only.
