# Guion current-format Feed Digest package verification

## Result

The current-format Guion Feed Digest application built a source-free, relocatable managed package on Debian GNU/Linux 12 (bookworm) x86_64. The package uses the pinned pure-Python feedparser dependency and passed the same six application acceptance cases as its independent Python reference.

## Verified package

- Artifact format: 44.
- Builder image: guion-feed-digest-debian-x86_64:rust-1.97-cmake; Rust 1.97.1; CMake 3.25.1.
- Managed CPython: 3.14.8.
- Bundled uv: 0.13.4.
- Dependencies: feedparser 6.0.14 and feedparser-sgmllib 2.1.0.
- Native extensions from these Python dependencies: 0.
- Both dependency license records are complete.
- Uncompressed package directory: 158,943,069 bytes (151.6 MiB, 4,718 files).
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

Run from the Guion repository with a feature-enabled CLI and the warmed repository-local package cache. The first run may use the preload option:

python3 evaluation/completion/performance/verify_managed_feed_digest_package.py --preload-dependencies --write-lock

Raw evidence: 2026-10-28-feed-digest-managed-package-debian-x86_64.json. All run scratch is under target/perf-session/guion-feed-digest-debian-x86_64/package-run and is removed on exit; only the small dependency cache is retained. The existing Colima VM was reused; Rosetta provided the x86_64 container path, and QEMU was not used.
