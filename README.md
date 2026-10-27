# Guion

Guion is a general-purpose programming language and toolchain built to make useful software easier to write, understand, and run. It has clear, typed syntax, a compiler written in Guion, and a Rust runtime. Normal source runs use the in-process Rust compiler. The default build works without Python.

The long-term ambition is to make Guion a practical choice over Python for more everyday programming, with a quick response and a broad range of useful work. Its programs use structured syntax; “plain English” describes the learning and tooling goal, not free-form natural-language code. That is a goal, not a result we claim today. Guion is an early, private pre-release project; there is no public release download yet.

## Try it from this checkout

You need Rust and Cargo to build Guion from source. From the repository root:

```sh
cargo run --locked -p guion-cli -- new /tmp/hello-guion
cargo run --locked -p guion-cli -- check /tmp/hello-guion/src/main.gu
cargo run --locked -p guion-cli -- test /tmp/hello-guion/src/main.gu
cargo run --locked -p guion-cli -- run /tmp/hello-guion
```

The starter program prints `Hello, Guion!`. The same CLI can format and lint source, build portable `.gub` artifacts, package and verify applications, inspect effects, and generate API documentation.

## What works today

- A self-hosted compiler checks and builds `.gu` programs into versioned `.gub` artifacts.
- The Rust runtime supports typed functions and records, collections, JSON, files, SQLite, TOML, HTTP, bounded concurrency, and explicit host permissions.
- The portable shared VM also exposes a versioned WebAssembly ABI for typed browser calls. ABI 1.0 preserves the legacy exports, grants no host capabilities, and bounds requests, values, output, and execution; see the [browser ABI guide](docs/browser-wasm-abi/README.md).
- The core CLI can create, check, format, lint, test, run, build, package, install, and inspect programs.
- Optional Python interoperability exists for projects that need it. It is off in the default core build.

See the [getting-started guide](docs/getting-started.md), [language and command reference](docs/reference.md), and [language contract](language/spec.md).

## Status and comparison with Python

The Guion validation matrix previously passed on macOS ARM64 and Debian 12 GNU/Linux on ARM64 and x86_64, all with glibc. In a 30-trial macOS ARM64 fresh-source Richards follow-up, the latest candidate measured **30.02 ms**, its preceding build measured **31.23 ms**, and CPython 3.14.7 measured **73.58 ms**. The candidate/preceding-build ratio was 0.969 (paired 95% interval 0.956–0.979); candidate/Python was 0.409 (0.405–0.414). Every run checked the expected output, Guion's source cache was bypassed, and the OS file cache was uncontrolled. The native backend now rejects both negative and too-large list indices with one unsigned bounds comparison. On the compiled Richards artifact, generated IR fell from 454 to 409 blocks and from 4,459 to 4,234 instructions; `main` fell from 358 to 320 blocks. In the 25-execution lane, the candidate measured 7.168 ms per execution versus 7.370 ms before (paired ratio 0.970, 95% interval 0.964–0.976). This Richards-specific improvement does not establish a general Python performance claim. See the [Richards gap-closure report](evaluation/completion/results/runtime-efficiency/2026-10-26-richards-gap-closure.md), [fresh-source comparison](evaluation/completion/results/runtime-efficiency/2026-10-27-richards-index-bounds-fresh-source-30-trial-comparison.json), and [25-execution comparison](evaluation/completion/results/runtime-efficiency/2026-10-27-richards-index-bounds-25-run-30-trial-comparison.json).

The five short-source comparisons also passed. A host profile captured generated-code execution and memory-management activity; it did not isolate every cost inside generated code.

Guion and a Python reference each passed 127 frozen cases across nine applications. Guion took less time on the selected measured case for each application; the shared Python reference uses broad imports, so the numbers describe those particular implementations. The Guion AllHereHTML builder is also retained as a regression, and its text operations now handle larger inputs within the default allocation budget. See the [real-task and runtime report](evaluation/completion/results/real-task-sprint/summary.md) for timings, memory, method, and limits. These measurements do not establish that Guion generally outperforms or replaces Python.

Guion remains a private pre-release project; no public release download has been published. See the [pre-release readiness report](docs/pre-release-readiness.md) for the validated scope and remaining work.

## Project guide

- [Getting started](docs/getting-started.md)
- [Agent programming path](docs/agents/start-here.md) and [runnable recipes](docs/agents/recipes.md)
- [Architecture](docs/architecture.md)
- [Browser WebAssembly ABI](docs/browser-wasm-abi/README.md)
- [Language and command reference](docs/reference.md)
- [Compatibility and migration](docs/compatibility-and-migrations.md)
- [Current status](docs/status.md)
- [Historical record index](docs/history/README.md)
- [Contributing](CONTRIBUTING.md)
- [Security](SECURITY.md)
- [License](LICENSE) and [third-party notices](THIRD-PARTY-NOTICES.md)
