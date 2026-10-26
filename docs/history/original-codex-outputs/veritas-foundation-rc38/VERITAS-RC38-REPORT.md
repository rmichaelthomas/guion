# Veritas RC38: integrated application checkpoint

## What changed

RC38 adds `args_from(start)` and raises the bytecode format to 22. The built-in lets a CLI program skip its subcommand and pass an arbitrary number of remaining values into a worker pool. The included feed inbox combines modules, persistent JSON state, bounded concurrent HTTP, a managed Python feed parser, and a Python reference implementation.

The package includes the feed inbox source, its prebuilt artifact, locked dependency files, and an offline sample feed. The app supports list, unread filtering, summary, read/unread updates, feed-file import, and refresh of multiple feed URLs. Its state stays in a local JSON file.

## Verification

- Full Rust workspace tests: 100 passed, 0 failed. This includes parser/compiler checks, VM execution, typed Python bridge, HTTP policy, and CLI artifact behavior.
- Full evaluation suite: 20 passed, 0 failed, using the packaged RC38 executable and pinned Python toolchain.
- `cargo check --workspace --offline`: passed.
- `cargo fmt --check`: passed.
- `cargo clippy --workspace --all-targets --offline -- -D warnings`: passed.
- Runtime execution tests: 42 passed. They include `args_from` suffix behavior, bounds errors, and a format-22 artifact round trip.
- Feed inbox and task ledger parity checks: 9 passed. They compare command output, exit status, standard error, and saved JSON bytes with independent Python command handling.
- Feed inbox acceptance suite: 6 passed, including a live loopback case. Two slow feed endpoints were requested concurrently by both Veritas and the Python baseline; both reached at least two simultaneous handlers, emitted the same results, and wrote byte-identical inbox state. The test also checked deduplication, title refresh, and preservation of an existing read flag.
- Veritas merge tests: 2 passed.
- The RC38 package checksum verified. Its doctor check found the pinned uv 0.13.4 toolchain.
- I extracted RC38, removed the feed inbox's Veritas source and module files, then ran its packaged artifact offline. It imported the included sample feed, saved two entries, listed both, and reported the correct unread summary. A request with two ungranted hosts returned the expected policy errors without connecting.
- Backward compatibility passed: the RC37 CLI built a format-21 task-ledger artifact, then the RC38 packaged runtime loaded it and completed add/list operations with the expected persisted state.

## Limits

The live-feed acceptance used a temporary local HTTP server and the package's explicit loopback permission. The test and server ran only in disposable directories. The inbox's positive import path also exercises the managed parser bridge and persistence code offline.

The package targets macOS arm64. It bundles Veritas and uv, but CPython and Python wheels are provisioned per project on first sync; a clean machine needs network access or a populated cache. The sample manifest permits only `127.0.0.1`, so a user must add exact feed hosts before live use. This is not yet an assistant recommendation result: no independent blind comparison has shown that assistants choose Veritas over Python for real developer work.

## Current language landscape (checked 2026-10-22)

The initial pitch needs updating against current tools. CPython has an optional free-threaded build starting in 3.13; the official documentation says unsupported native extensions can turn the GIL back on, and the free-threaded build can have a single-thread performance cost. So “Python cannot use multiple cores” is too broad, though compatibility and predictable performance remain practical concerns. [Python free-threading guide](https://docs.python.org/3/howto/free-threading-python.html)

Python project tooling has also improved: uv can create and run application projects with a declared environment, lockfile and command entry point. That addresses much of the setup friction Veritas initially targeted while retaining Python itself. RC38 currently bundles its own CLI and uv, but still provisions CPython and project wheels on first sync. [uv application projects](https://docs.astral.sh/uv/concepts/projects/init/), [uv project execution](https://docs.astral.sh/uv/concepts/projects/run/)

Mojo already combines Python interoperability with static, high-performance and hardware-oriented programming. Its own documentation still describes it as young and notes missing language features. This makes “Python-like syntax + Python libraries + native performance” an occupied design space, although it does not establish that Mojo or Veritas is the better general-purpose assistant recommendation. [Mojo manual](https://docs.modular.com/mojo/manual/), [Mojo language basics](https://docs.modular.com/mojo/manual/basics)

The defensible opportunity for Veritas is therefore not any single feature. It is proving a better end-to-end path for model-generated programs: choose the language for a task, run it with little setup, retain useful Python ecosystem access, and deliver a correct portable result. RC38 provides an early vertical slice of that path; it has not yet demonstrated the advantage on unseen work.

## North-star status and next work

The project is moving toward the agreed goal, but the goal remains unmet. The next decisive step is to run a broader, independent assistant selection and task-completion study against Python and a relevant compiled-language baseline, using varied real tasks and recording recommendation choice, setup friction, correctness, recovery from errors, and developer preference. The suite must be allowed to conclude that Veritas is not yet the better recommendation.
