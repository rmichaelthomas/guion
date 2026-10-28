# Feed Digest managed package on Debian x86_64

**Goal:** Extend the current-format managed-package evidence for the real Feed Digest application from macOS ARM64 to Debian 12 x86_64, including source-free relocation and the same six output-checked application cases.

**Target:** The existing running Colima default VM, using its Rosetta-enabled `vz` profile to execute one Debian 12 x86_64 builder. The VM is ARM64-hosted; the package and tests must report an actual `x86_64` Linux target. Do not accept QEMU emulation as a substitute.

## Constraints

- Reuse the existing Colima VM and warm dependencies once for this target. Do not create a second VM.
- Rosetta is already installed on macOS and enabled in the existing Colima profile (`rosetta: true`); verify the amd64 container path before building. Do not reinstall it.
- Build from a small derived image based on `rust:1.97-bookworm` with CMake and native build tools installed once. Keep the image build context in ignored repository scratch and reuse the resulting image for later Linux package checks.
- No running Docker containers were present before this pass. Check CPU, VM disk, host disk, and Docker storage before building.
- Keep all temporary projects, the Linux package cache, Cargo target output, and logs under ignored `target/perf-session/` in the Guion repository.
- Pin Debian 12, Rust 1.97, uv 0.13.4, CPython 3.14.8, feedparser 6.0.14, and feedparser-sgmllib 2.1.0. Preload the required runtime and dependency cache once, then run acceptance offline.
- Preserve the macOS Feed Digest report and the existing x86_64 NumPy/orjson report. Write a separate Debian x86_64 Feed Digest report and raw JSON.
- Do not change Guion source syntax or artifact format. Keep the HTTP grant loopback-only and package output source-free.
- Remove only scratch created by this pass. Keep any small target-specific cache that will be reused by the next Linux package check; do not prune unrelated Docker images or caches.

## Steps

1. Verify Colima is running, Rosetta is configured, the Docker daemon has no running containers, and disk headroom is sufficient.
2. Add a Debian x86_64 builder image with native build tools, then preload pinned Guion build dependencies, uv, managed CPython, and the pure-Python Feed Digest dependencies into repository-local scratch.
3. Make the existing Feed Digest verifier record the target, VM reuse, and Rosetta use accurately, and write to a target-specific report path.
4. Build the feature-enabled Guion CLI in the amd64 Debian container and verify `uname -m` is `x86_64` before running the acceptance verifier.
5. Run managed sync once, locked offline sync, source-free packaging, integrity checks, relocation, and all six source plus six relocated-package acceptance cases with package dependency network disabled.
6. Update status, pre-release readiness, and the sprint log with the target-specific evidence and its limits. Run focused checks and verify scratch, processes, caches, and VM state.
7. Commit the target setup before the acceptance run where practical, then commit its results and documentation on this same branch.

## Acceptance

- The run is Debian 12 GNU/Linux on x86_64 under Rosetta, not ARM64 and not QEMU.
- The package contains no Guion or Python reference source, includes the exact loopback-only HTTP policy, and passes native dependency and license inspection.
- The source suite and relocated-package suite each pass 6/6; every report and stdout matches the independent Python baseline byte-for-byte.
- The new report records host architecture, artifact format, builder image and Rust/CMake versions, pinned runtime/dependency versions, package size/hash, source/lock hashes, offline status, cache path, Rosetta usage, and cleanup details.
- Strict local validation passes; no new GitHub Actions billing failure is treated as a source test result.
