# Veritas checkpoint v1.39 — release archive validation

Status: implemented and validated in the installed RC62 release.

The release archive now receives a pre-publication structural check. It rejects absolute paths, parent traversal, duplicate names, unsupported special files, files that traverse non-directory entries, links that escape the single package root, and links whose targets are missing. The packaging script runs adversarial unit cases before building and validates the actual tar before generating its checksum. `COPYFILE_DISABLE=1` prevents macOS from adding AppleDouble metadata files to the archive.

The validator exposed two real defects in the previous archive: 5,642 AppleDouble entries appeared beside the intended files, and the bundled `.venv/bin/python` symlink pointed to the temporary package-staging directory. Packaging now resolves absolute bridge-environment links while staging still exists, confirms their targets remain inside the package, and rewrites them as relative links. A regression test confirms the rewritten link resolves to the bundled interpreter.

The validator's four adversarial tests pass. The full Rust workspace passes 120 tests, including managed Python integration and relocated compiled-wheel packages. Formatting and strict Clippy pass. The final RC62 archive contains 5,637 entries, zero AppleDouble files, and zero absolute symlinks; all archive links resolve within the package. Its checksum verifies. The installed 22-test suite passes, as do installer relocation and bundled app verification. After extracting the release into a fresh Unicode-and-spaces path, `doctor` passed and the bundled Python app launched twice offline with identical output.

Dynamic loader event tracing remains unverified. `DYLD_PRINT_LIBRARIES` emitted no records on this host, so the report continues to describe static load-command coverage only. The recommendation evaluation, cross-platform packaging, and a clean-machine test remain open.
