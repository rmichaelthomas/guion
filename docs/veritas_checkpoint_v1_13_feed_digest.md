# Veritas checkpoint v1.13 — feed digest vertical application

Date: 2026-10-22  
Status: implementation checkpoint; build scope remains active  
Foundation: `work/veritas-foundation/`  
Previous source snapshot: RC28, artifact format 19

## Purpose

Exercise one useful developer workflow across the current language rather than demonstrate isolated syntax: accept runtime input, fetch independent feeds concurrently under an explicit network policy, use a real managed Python library through a typed boundary, produce deterministic output, and preserve useful errors. This is a known workload for development. It is not independent evidence that an assistant should recommend Veritas over Python.

## Built in this checkpoint

- Added `evaluation/development/feed_digest`, an ordinary source project that accepts two or more feed URLs at run time, fetches with bounded `parallel_map` and `http_get`, calls managed `feedparser==6.0.14` through a typed `List[Map[Text, Text]]` declaration, deduplicates canonical links, and writes `digest.md`.
- Kept the app's committed network permission to the exact host `127.0.0.1` for the loopback test fixture. A user copying the app for real feeds must grant only the feed hosts that project needs.
- Added an independent Python implementation using `ThreadPoolExecutor`, the standard library HTTP client and the same pinned parser, with the same declared limits and output contract.
- Added six loopback acceptance tests: Python parity, canonical-link deduplication and Unicode, stable output under reversed URL order, malformed feeds, partial success after a 404, changed content on the next run, host denial, and actual overlap of two requests in both implementations. Concurrency is asserted by observing two server requests active at once, not by a fragile speed threshold.
- Fixed the Python sidecar import path. It still runs in isolated mode, but now explicitly adds the Veritas project root so local adapter modules can be imported while third-party packages come from the managed environment.
- Added target-specific Rust dependency license collection to current-host package assembly. For macOS ARM64, the generated inventory covers all 121 Cargo-resolved third-party crates and copies their shipped license/notice files.
- Updated the reference, Python ABI, host API, app guidance, development workload guide, and package contents.

Duplicate records resolve independently of input order: a record with a publication date outranks one without; records with the same date-presence status are resolved by lexicographically earlier title. This rule is explicit and tested. It does not claim to identify the semantically newest record across differently formatted publication dates.

## Evidence

Managed sync succeeded with pinned uv 0.13.4, CPython 3.14.8, and `feedparser==6.0.14`. The compiler checks the feed-digest project successfully.

The loopback acceptance command completed **6 tests, 0 failures**. Both the Veritas app and Python baseline showed overlapping feed requests. The tests use a fresh temporary copy of the project for every scenario, so they do not leave output files in the checked-in project.

The complete Rust workspace completed **88 tests, 0 failures**: CLI unit and integration tests, Python dependency tests, compiler pipeline tests, model/artifact tests, runtime execution tests, HTTP tests, and Python bridge tests. `cargo fmt --all -- --check` and `cargo clippy --offline --workspace --all-targets -- -D warnings` passed.

The integration run found and fixed three concrete gaps before passing:

1. The draft source tried to concatenate an integer into text and called a map-key helper absent from the language. It now prints the integer separately and uses the supported deterministic map iteration.
2. Duplicate-link output changed when input URL order changed. The app and baseline now implement the same stable tie rule.
3. An isolated Python sidecar could not import a module stored beside the Veritas project. A Rust regression test first reproduced `ModuleNotFoundError`; the sidecar now exposes only the project root as an explicit import location, and the regression test passes.

No public internet feed was contacted. The Python baseline comparison uses the same local responses and declared body/time limits; it is a correctness and concurrency comparison, not a throughput benchmark. No independent builder sessions, broad task classes, cold-install time study, or cross-host package test were run in this checkpoint.

The current-host RC31 archive was extracted into a fresh temporary directory. Its `doctor` command passed, its pinned Python environment synchronized in locked offline mode from the already populated local cache, and all **6 acceptance cases passed again against the packaged Veritas executable**. This proves extracted-path behavior on this machine with a warm dependency cache; it does not prove a cold download, clean-machine installation or relocation to another OS/architecture. The archive packages at `/Users/rmichaelthomas/Documents/Codex/2026-10-22/referenced-chatgpt-conversation-this-is-an/outputs/veritas-foundation-rc31/`.

## Remaining gaps and next work

- The recommendation north star is **not met yet**. Evidence is author-run and concentrated in a single integration workload plus the earlier sales aggregation probe. A preregistration draft for the independent comparison now lives at `evaluation/protocol/README.md`; it still needs independent evaluator review and real runs. A recommendation verdict needs frozen tooling and independent, varied briefs, with Python given comparable modern project tooling.
- HTTP remains a bounded UTF-8 GET surface. There are no general methods, headers, request bodies, binary responses or streaming. DNS rebinding and OS-level egress confinement are outside the runtime; a blocking request can delay cancellation until its deadline.
- The current-host archive contains uv, Veritas, docs and locked examples. It does not contain a relocatable managed CPython or wheel environment; a target machine must synchronize them. Current-host package smoke and relocation evidence must be kept distinct from clean-machine installation.
- Only macOS ARM64 has a pinned uv distribution in this candidate. Linux, Windows, browser and cross-host targets remain unsupported here.
- Native extension dependencies, process RSS accounting, compiler compatibility policy, broad language maintenance experience, cold startup, and high-volume long-session behavior need additional evidence.
- The package contains a target-specific Rust license inventory; Python wheels are acquired later by sync and are not bundled into this archive.

## Decision

Continue the build. The architecture is feasible for an ambitious general-purpose language experiment: the vertical path now composes compiled Veritas source, bounded concurrency, default-deny HTTP, managed Python packages, a local typed adapter, and user-visible output. Keep Veritas framed as an experimental candidate and use Python when a project needs mature general tooling or portability beyond the supported target. The next milestone should expand one materially different program class and freeze the evaluation protocol before any assistant-recommendation claim.
