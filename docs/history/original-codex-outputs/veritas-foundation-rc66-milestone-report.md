# Veritas RC66 — release milestone

**RC66 is built and passed its installed acceptance suite on macOS arm64.** It gives us a source-free, relocatable application package with managed Python interop, native dependency provenance, structured runtime observations, and staged dependency updates. This is a meaningful foundation release; it does not yet justify recommending Veritas over Python.

## Evidence

- 23/23 installed acceptance tests passed, covering Python parity, loopback concurrency, source-free artifacts, package relocation, diagnostics repair, and shipped compatibility policy.
- Fresh extraction under a Unicode-and-spaces path passed `doctor`; repeated app runs matched exactly.
- A new task-ledger package verified and launched after relocation.
- The relocated CLI updated a Python bridge project offline with bundled uv 0.13.4 and then ran it.
- C dynamic-library loading through Python `ctypes`, success/failure image observations, and NumPy wheel-cache ownership passed in the workspace tests.
- Archive validation found 5,642 entries, no AppleDouble entries, and no absolute symlink targets.

## Download

- [RC66 macOS arm64 release archive](veritas-foundation-rc66/veritas-0.1.0-darwin-arm64.tar.gz)
- [SHA-256 file](veritas-foundation-rc66/veritas-0.1.0-darwin-arm64.tar.gz.sha256)
- [Machine-readable result](veritas-foundation-rc66-results.json)
- [Source tree archive](veritas-foundation-source-rc66.tar.gz)
- [v1.44 checkpoint](veritas-foundation-rc66-checkpoint-v1-44.md)

Archive SHA-256: `c23d8f72d69236c1249a0b5f0885c0397e82fffdffa99098718c21fd8db7c8b2`

## Limits and next work

Validation covers macOS arm64 only. Native-image observation is a boundary snapshot, an interrupted update can leave a mismatch that locked execution rejects, the package is unsigned, and project data migration remains app-owned. The independent recommendation gate remains open.

The next build phase is the comparative recommendation evaluation: freeze multi-class tasks and measures, give Veritas and current Python fair entry workflows, account for setup and warm-use costs, then evaluate unseen tasks with independent builders when available. In parallel, continue target and relocation coverage.
