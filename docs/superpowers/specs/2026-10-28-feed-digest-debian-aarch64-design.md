# Feed Digest on Debian ARM64: Design

## Goal

Validate the existing format-44 Feed Digest managed package on Debian 12 GNU/Linux ARM64 (`aarch64`) using the existing Colima VM's native ARM64 execution path.

## Acceptance

- Use Debian 12, Rust 1.97.x, CMake, CPython 3.14.8, uv 0.13.4, feedparser 6.0.14, and feedparser-sgmllib 2.1.0.
- Confirm `uname -m` and the package report identify `aarch64`; record Colima use and no Rosetta translation.
- Preserve the source-free format-44 package, exact loopback-only HTTP grant, complete dependency license records, and zero native extensions from the pure-Python parser dependencies.
- Relocate the package to a path with spaces and Unicode; pass all six source cases and all six relocated-package cases with byte-for-byte parity against the Python reference.
- Run dependency resolution and acceptance offline except for the loopback fixture server.
- Write a separate Markdown report and JSON record under `evaluation/completion/results/real-task-sprint/`.

## Resource and cleanup boundaries

- Reuse the already running Colima default VM; do not start another VM or install Rosetta.
- Use a Linux/arm64 builder, maximum four container CPUs and 6 GiB for acceptance, and two CPUs for the Guion release build.
- Keep target scratch and the ARM64 Python/runtime cache in `target/perf-session/guion-feed-digest-debian-aarch64/`.
- Reuse the existing repo-local pure-Python uv wheel cache. Keep architecture-specific Python, uv executable, Rust target output, and builder image separated by target.
- Remove temporary package/project/Cargo scratch on success. Retain only the reusable ARM64 Python cache and builder image; do not prune unrelated Docker images or caches.

## Limits

This validates one managed package on one Linux distribution and architecture. It is package compatibility evidence, not a speed result or broad Python-package compatibility claim.
