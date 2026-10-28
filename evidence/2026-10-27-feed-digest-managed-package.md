# Guion current-format Feed Digest package verification

## Result

The current-format Guion Feed Digest application built a source-free, relocatable managed package on macOS ARM64. The package uses the pinned pure-Python feedparser dependency and passed the same six application acceptance cases as its independent Python reference.

## Verified package

- Artifact format: 44.
- Managed CPython: 3.14.8.
- Bundled uv: 0.13.4.
- Dependencies: feedparser 6.0.14 and feedparser-sgmllib 2.1.0.
- Native extensions from these Python dependencies: 0.
- Both dependency license records are complete.
- Uncompressed package directory: 172,422,430 bytes (164.4 MiB, 2,946 files).
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

Raw evidence: 2026-10-27-feed-digest-managed-package.json. All run scratch is under target/perf-session/feed-digest-managed-package and is removed on exit; only the small dependency cache is retained. No VM was started for this macOS ARM64 result.
