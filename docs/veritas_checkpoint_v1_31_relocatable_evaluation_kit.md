# Veritas checkpoint v1.31 — relocatable evaluation kit

## Change

The shipped Python acceptance suite now resolves its known workloads from the installed package's `examples/` tree when the source checkout's `evaluation/development/` tree is absent. The distribution includes the benchmark-summary helper imported by the suite. The evaluation README documents how to run the suite from the installed release root with its matching CLI and pinned Python toolchain/cache.

The kit contains both workspace-oriented benchmark probes and acceptance checks. Its tests make their source-vs-installed expectations explicit; the release check is run from the installed root against the installed CLI.

## Evidence

- The first full installed RC50 suite attempt exposed missing project directories and one missing helper module. Eighteen test cases errored because the package scripts still assumed the workspace layout.
- After path fallback and package inclusion fixes, all 21 Python acceptance tests passed against the installed RC49 CLI using the current shipped-kit scripts, from outside the source repository.
- RC51 was installed under a path with spaces and Unicode. All 21 tests passed from the installed release root against its installed CLI, using the packaged examples and helpers.
- The refreshed Rust workspace suite passed 114 tests; formatting, strict Clippy, and whitespace checks passed.

## Limits and next work

These are author-run known workloads. The suite includes no independent briefs or fresh assistant sessions, and therefore cannot establish recommendation advantage. Cross-host packaging remains unverified.
