# Guion current-format Feed Digest package verification

## Result

The current-format Guion Feed Digest application built a source-free, relocatable managed package on Debian GNU/Linux 12 (bookworm) aarch64. The package uses the pinned pure-Python feedparser dependency and passed the same six application acceptance cases as its independent Python reference.

The run reused the existing Colima VM's native ARM64 path: `uname -m` reported `aarch64`, Rosetta translation was not used, and no new VM was started.

## Verified package

- Artifact format: 44.
- Builder image: guion-feed-digest-debian-aarch64:rust-1.97-cmake; Rust 1.97.1; CMake 3.25.1.
- Managed CPython: 3.14.8.
- Bundled uv: 0.13.4.
- Dependencies: feedparser 6.0.14 and feedparser-sgmllib 2.1.0.
- Native extensions from these Python dependencies: 0.
- Both dependency license records are complete.
- Uncompressed package directory: 128,874,649 bytes (122.9 MiB, 4,723 files).
- The source-free artifact has no bundled Guion source; its manifest remains to carry the project policy and exact loopback-only host grant.
- Package was verified before and after moving it to a path with spaces and Unicode.
- The relocated package passed all six loopback application acceptance cases with external Python and uv configuration removed from the Guion launch process.

## Application checks

- Every digest matched the independent Python implementation byte-for-byte.
- Cases cover duplicate links, Unicode titles, stable ordering, changed feed content on a fresh run, malformed input, HTTP 404 handling, denied hosts, and concurrent requests.
- Dependency resolution and the acceptance suites ran offline after the pinned packages were preloaded into the repository cache. The only network traffic during acceptance was to the local loopback fixture server.

## Limits

This extends evidence to one more pure-Python package and one real application shape. It does not establish broad package compatibility, native-wheel coverage, package-size advantages, or a speed result. Package assembly and launch phase durations are recorded in the adjacent JSON as operational observations, not benchmarks.

## Reproduction and hygiene

Reproduce the target run from the Guion repository with:

```sh
bash evaluation/completion/performance/run_feed_digest_debian_aarch64.sh
```

Raw evidence: [2026-10-28-feed-digest-managed-package-debian-aarch64.json](2026-10-28-feed-digest-managed-package-debian-aarch64.json). The run reused the repo-local pure-Python dependency cache, removed temporary project/package/Cargo scratch on exit, and retained only the ARM64 Python runtime cache and reusable builder image. No Docker cache pruning was performed.
