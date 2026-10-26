# Veritas RC62 — release integrity milestone

## Verdict

RC62 is a verified macOS ARM64 foundation release with safer package publication and validated release archives. It is not yet evidence that assistants should recommend Veritas over Python generally; the independent evaluation gate remains open.

## What the archive audit found and fixed

Inspection of RC61 found 5,642 AppleDouble metadata entries and a Python virtual-environment symlink pointing back into a temporary staging directory. RC62 packaging now rewrites that interpreter link to a relative path within the bundle, disables AppleDouble generation, and validates the tar before writing its checksum.

The archive validator rejects absolute and traversal paths, duplicate entries, unsupported special files, links that escape the package root or point nowhere, and children written beneath symlink entries. Four adversarial validator tests pass.

## Verification

- Full Rust workspace: **120 passed, 0 failed**, including managed Python, compiled wheels, relocation, and the Python-link regression.
- Installed RC62 acceptance kit: **22 passed, 0 failed**.
- Formatting and Clippy with warnings denied passed.
- RC62 archive: **5,637 entries**, zero AppleDouble entries, zero absolute symlinks, all archive links validated, checksum verified.
- Extracted the archive into a fresh Unicode-and-spaces path; `doctor` passed and the bundled Python app launched twice offline with identical output: `15`, `9007199254741000`, `3`, `python-requests/2.32.5`.
- Source archive checksum verified; extracted source compiled all workspace test executables offline under a Unicode-and-spaces path.
- Release archive SHA-256: `30142712166733b7dcf87467075a25d3faec5d1586b6a495562cd1e26e97b959`.
- Source archive SHA-256: `916232ab43ad46d8cf959c5a7630a320e1cc8b19fc1d40306beb97a1ffc5bd6b`.

## Remaining work

Dynamic native-library loads remain unobserved; the `DYLD_PRINT_LIBRARIES` probe produced no events on this host, so the package report continues to claim static load-command coverage only. Cross-platform packaging, clean-machine validation, broader resource accounting, and independent Python-versus-Veritas builder sessions remain open. No broad assistant-recommendation claim is made.

## Artifacts

- Installable release: `veritas-foundation-rc62/veritas-0.1.0-darwin-arm64.tar.gz`
- Editable source and evaluation kit: `veritas-foundation-source-rc62.tar.gz`
- Checksums: `veritas-foundation-rc62.sha256` and `veritas-foundation-source-rc62.tar.gz.sha256`
- Machine-readable results: `veritas-foundation-rc62-results.json`
- Detailed checkpoint: `veritas-foundation-rc62-checkpoint-v1-39.md`
