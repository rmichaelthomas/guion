# Veritas RC61 — packaging safety milestone

## Verdict

RC61 is a verified macOS ARM64 foundation build with a packaging correctness fix. It is not yet evidence that an assistant should recommend Veritas over Python generally. The product now has a substantial language/runtime/CLI path and a reproducible, locally installable release, while important portability and independent comparative evidence remain open.

## What changed

`veritas package` used a path check that followed symlinks. A dangling symlink could therefore be mistaken for an unused destination and replaced. The CLI now uses a no-follow existence check, preserves dangling links, and returns `E_PACKAGE_OUTPUT_EXISTS`. Final publication uses macOS `renameatx_np(RENAME_EXCL)` so another process cannot create a destination in the gap and have it overwritten.

Acceptance coverage packages a project from paths containing spaces and Unicode, verifies and launches the bundle from a different working directory, and simulates a destination appearing before publication. Both the competing destination and staged package survive the rejected publish.

## Evidence

- Full Rust workspace: **119 passed, 0 failed**, including managed Python integration and packaging relocation cases.
- Installed RC61 evaluation kit: **22 passed, 0 failed**, with the isolated CPython/cache provisioned and loopback fixtures enabled.
- `cargo fmt --all -- --check` and Clippy with warnings denied passed.
- The distribution script installed RC61 into a Unicode-named prefix, verified bundled task-ledger and Python interop apps, and confirmed repeated Python app launches had identical output.
- The source archive checksum verified; extracted under a Unicode-and-spaces path, it compiled all workspace test executables offline.
- The release archive checksum is `d573dbf22d443cfbb970515b67400bcbbb834103cced3c8ce07067b68fe8e353`.

## What this still does not prove

This is a single-host macOS ARM64 release. It does not cover archive path attacks or cross-platform atomic publication. The shipped evaluation is author-run; no independent unseen builder sessions were available, so the recommendation gate has not passed. Dynamic native loading/cache-wheel provenance and broader resource accounting also remain active engineering gaps.

## Continue from here

Next, close release archive path validation and dynamic native dependency provenance, then freeze the compiler and evaluation kit for independent Python-versus-Veritas tasks. Keep the evaluation honest about tasks where Python remains the better recommendation.

## Artifacts

- Installable release: `veritas-foundation-rc61/veritas-0.1.0-darwin-arm64.tar.gz`
- Editable source and evaluation kit: `veritas-foundation-source-rc61.tar.gz`
- Release checksum: `veritas-foundation-rc61.sha256`
- Source checksum: `veritas-foundation-source-rc61.tar.gz.sha256`
- Machine-readable results: `veritas-foundation-rc61-results.json`
- Detailed checkpoint: `veritas-foundation-rc61-checkpoint-v1-38.md`
