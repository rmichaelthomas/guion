# Veritas checkpoint v1.19: integrated application

**Status:** active build checkpoint. This advances the north-star work; it does not establish that an assistant should recommend Veritas over Python.

## Objective

Test whether the already-built compiler, runtime, CLI, managed Python bridge, HTTP policy, persistent state, and artifact tooling work together in an ordinary multi-command program. Preserve Python as a fair reference and keep all development isolated from the user's existing language repositories and NowLookHere files.

## Implemented

- Added `args_from(start)` to return CLI arguments beginning at a zero-based index. It typechecks to `List[Text]`, reports a typed runtime index error for negative or past-end values, is charged against runtime allocation limits, and uses artifact opcode 86 in format 22.
- Built `feed_inbox`, a multi-command app with modules, atomic JSON persistence, read/unread tracking, canonical-link deduplication, deterministic conflict resolution, bounded HTTP refresh, multiple feed URLs through `args_from(1)` and `parallel_map`, and managed `feedparser` interop.
- Added `import-feed PATH` as an offline ingestion route so the feed parser, bridge, merge, and persistence path can be exercised without a live server.
- Added an independent Python CLI reference and parity cases. Both versions share the pinned parser dependency; state handling, commands, output, and persisted bytes are implemented separately.
- Updated the host packaging script to include the example project and its prebuilt format-22 artifact.

## Evidence

- `cargo check --workspace --offline`: passed.
- `cargo clippy --workspace --all-targets --offline -- -D warnings`: passed.
- Runtime execution tests: 42 passed, including argument slicing, artifact round-trip, index bounds, and existing runtime contracts.
- Feed inbox and task ledger parity suites: 9 passed. Feed inbox covers source and source-free artifact execution, parser-backed ingestion, read-state preservation, multiple denied URLs, command results, and exact saved JSON bytes.
- Veritas-language merge tests: 2 passed.
- The local sandbox denied binding a loopback server, so this checkpoint's live-fetch success is not re-run end-to-end. Earlier packaged feed-digest checks in RC33 provide the loopback HTTP/concurrency evidence. Feed inbox itself was tested with multiple inputs that are denied by the manifest before connection.
- RC38's packaged macOS arm64 CLI passed doctor. After extracting RC38 and removing the feed inbox `.vr` source and module files, the packaged format-22 artifact imported the sample feed offline, listed both entries, and reported the correct summary.
- Backward compatibility: the RC37 CLI built a format-21 task-ledger artifact; the RC38 packaged runtime loaded it and completed add/list operations while preserving state.

## Limits and open risks

- The manifest grants only `127.0.0.1`; users must grant their exact feed hosts before real refreshes.
- This acceptance uses one Apple Silicon machine and author-defined cases. It is not an independent assistant preference study, broad ecosystem comparison, or general performance claim.
- The Python reference shares `feedparser` with Veritas to isolate language/runtime behavior; parser correctness is therefore not independently compared.
- The app does not yet manage feed subscriptions, credentials, scheduling, or concurrent writers to one state file.
- The Veritas CLI remains a young toolchain. This checkpoint does not validate Windows/Linux packages, editor support, debugger experience, language server, or existing Python package compatibility.

## Next north-star step

Build an evaluation set from real developer tasks, including the user's command-line friction and the multi-feed inbox workflow. Have independent assistants choose a language/toolchain before implementation, then compare completion, failure recovery, packaging/setup friction, and developer preference against Python and a suitable compiled-language baseline. Do not count a Veritas-specific benchmark or an author-written fixture as proof of recommendation advantage.
