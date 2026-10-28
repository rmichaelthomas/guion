# Feed Digest on Debian ARM64 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Verify the existing format-44 Feed Digest managed package on native Debian 12 ARM64 and preserve reproducible evidence.

**Architecture:** Reuse the verifier and acceptance contract established for Debian x86_64. Add an architecture-selectable runner with thin x86_64 and aarch64 entry points, target-specific builder/runtime caches, and a shared pure-Python wheel cache; run a native arm64 container in the existing Colima VM.

**Tech Stack:** Rust 1.97, Debian 12, CMake, Guion CLI, uv 0.13.4, CPython 3.14.8, Python unittest, Docker/Colima.

**Spec:** `docs/superpowers/specs/2026-10-28-feed-digest-debian-aarch64-design.md`

## Global Constraints

- Use the existing `/Users/rmichaelthomas/guion` checkout, branch from merged `main`, and keep generated reports and scratch inside the repository.
- Require Linux/arm64 container execution and `uname -m` equal to `aarch64`; record `colima_vm_used=true` and `rosetta_translation_used=false`.
- Pin Debian 12, Rust 1.97.x, CPython 3.14.8, uv 0.13.4, feedparser 6.0.14, and feedparser-sgmllib 2.1.0.
- Acceptance uses at most four CPUs and 6 GiB; the release build uses at most two CPUs.
- Preserve format 44, source-free relocation, exact loopback grant, and offline dependency verification; do not change Guion syntax or artifact format.
- Keep target scratch under `target/perf-session/guion-feed-digest-debian-aarch64/`; share only the pure-Python uv wheel cache with the x86_64 target.

## Review Focus

- Architecture substitution: assert native `aarch64` at Docker server and container preflight, and reject a mismatched target in verifier tests.
- VM provenance: verify the existing Colima VM was reused and report Rosetta as false for this native run.
- Worker capacity: expose four acceptance CPUs so the app's four-worker request is valid; retain the two-CPU compiler limit.
- Relocation and source removal: verify package behavior after moving to a path with spaces and Unicode, with external Python configuration removed.
- Network policy: require offline dependency sync and allow only the loopback fixture during acceptance.

---

### Task 1: Add a shared architecture-aware Debian runner

**Files:**
- Create: `evaluation/completion/performance/run_feed_digest_debian.sh`
- Create: `evaluation/completion/performance/run_feed_digest_debian_aarch64.sh`
- Create: `evaluation/completion/performance/run_feed_digest_debian_x86_64.sh` (compatibility entry point)
- Rename: `evaluation/completion/performance/Dockerfile.feed-digest-debian-x86_64` to `evaluation/completion/performance/Dockerfile.feed-digest-debian`
- Modify: `evaluation/tests/test_managed_feed_digest_target.py`

**Interfaces:**
- Shared runner consumes `GUION_FEED_DIGEST_ARCH` with values `aarch64` or `x86_64`; wrappers set the value and `exec` the shared runner.
- The existing verifier consumes `--expected-arch aarch64 --colima-vm-used` and no `--rosetta-used` for the ARM64 run.
- ARM64 cache is architecture-specific except `managed/cache`, which points to the existing x86_64 pure-Python wheel cache.

- [x] Add `test_target_validation_accepts_debian_aarch64_without_rosetta` to assert Linux/aarch64/Debian 12 passes with `colima_vm_used=True`, `colima_vm_started=False`, and `rosetta_used=False`.
- [x] Run `python3 -m unittest discover -s evaluation/tests -p 'test_managed_feed_digest_target.py' -v`; all six target-validation tests pass.
- [x] Move the existing runner body into the shared runner; select `linux/arm64`/`aarch64` or `linux/amd64`/`x86_64`, target-specific scratch/image names, and target-specific Rosetta evidence from `GUION_FEED_DIGEST_ARCH`.
- [x] Keep the existing x86_64 command as a wrapper, add the ARM64 wrapper, and use the architecture-neutral Dockerfile for both.
- [x] Run `bash -n` on all three scripts, validate the shared-cache link, and run `git diff --check`; commit runner, Dockerfile, and test changes before the VM acceptance run.

### Task 2: Build and verify the Debian ARM64 package

**Files:**
- Create: `evaluation/completion/results/real-task-sprint/2026-10-28-feed-digest-managed-package-debian-aarch64.md`
- Create: `evaluation/completion/results/real-task-sprint/2026-10-28-feed-digest-managed-package-debian-aarch64.json`

- [ ] Verify the existing Colima VM is running, is ARM64, has no active containers, and has adequate disk space; do not start or reconfigure a VM.
- [ ] Build the derived Debian 12/Rust 1.97/CMake image for `linux/arm64` and require `uname -m` = `aarch64` before compiling Guion.
- [ ] Install native CPython 3.14.8 and uv 0.13.4 in ARM64 scratch, reuse the pure-Python wheel cache, and verify locked offline sync for feedparser 6.0.14 and feedparser-sgmllib 2.1.0.
- [ ] Build the feature-enabled CLI with no more than two CPUs; run source and relocated-package acceptance with four CPUs maximum and 6 GiB maximum.
- [ ] Require 6/6 source cases and 6/6 relocated-package cases, exact Python-reference output, zero parser native extensions, complete licenses, source-free package, exact loopback grant, offline sync, and Rosetta false.
- [ ] Confirm temporary project, package, Cargo, and image-context scratch cleanup; retain only the ARM64 runtime cache and reusable builder image; commit report files and any required run fixes.

### Task 3: Update status and close the target

**Files:**
- Modify: `README.md`
- Modify: `docs/status.md`
- Modify: `docs/pre-release-readiness.md`
- Modify: `evaluation/completion/results/real-task-sprint/summary.md`

- [ ] Update the Feed Digest target matrix with the Debian ARM64 result and its limits; preserve the x86_64 result unchanged.
- [ ] Run the focused target tests, JSON report checks, documentation-link validation, shell syntax checks, and `git diff --check`; commit the documentation.
- [ ] Review the branch diff, open the PR, merge it under the already-authorized workflow, delete the merged branch, and leave unrelated untracked site work untouched.
