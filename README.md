# Guion

Guion is a general-purpose programming language and toolchain built to make useful software easier to write, understand, and run. It has clear, typed syntax, a compiler written in Guion, and a Rust runtime. The default build works without Python.

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
- The core CLI can create, check, format, lint, test, run, build, package, install, and inspect programs.
- Optional Python interoperability exists for projects that need it. It is off in the default core build.

See the [getting-started guide](docs/getting-started.md), [language and command reference](docs/reference.md), and [language contract](language/spec.md).

## Status and comparison with Python

The Guion pre-release validation matrix has passed on macOS ARM64 and Debian 12-compatible GNU/Linux ARM64 with glibc. In a fresh-source, 15-pair macOS Richards cohort, Guion took 46.00 ms versus 67.43 ms for pinned CPython 3.14.7: about 31.5% less time (ratio 0.685; 95% interval 0.673–0.690). The same cohort measured a 3.9% lower median than the prior Guion build. An earlier separate comparison reported 5.2% lower fresh latency and 14.1% lower per-run latency across 25 repeats; that repeated-speed result was not rerun in the follow-up. Peak RSS was 3.6% lower for fresh source and 8.3% lower for 25 repeats than the prior build. Runtime counters put list copies at 77.2% of estimated guest allocation-budget bytes, but macOS blocked process sampling, so CPU-time attribution remains unknown. These results apply to the measured workloads and host; they do not establish that Guion generally outperforms or replaces Python. See [current status and evidence](docs/status.md) and the [Richards runtime profile](evaluation/completion/results/runtime-efficiency/2026-10-26-richards-profile-summary.md) for details.

Guion remains a private pre-release project; no public release download has been published. See the [pre-release readiness report](docs/pre-release-readiness.md) for the validated scope and remaining work.

## Project guide

- [Getting started](docs/getting-started.md)
- [Agent programming path](docs/agents/start-here.md) and [runnable recipes](docs/agents/recipes.md)
- [Architecture](docs/architecture.md)
- [Language and command reference](docs/reference.md)
- [Compatibility and migration](docs/compatibility-and-migrations.md)
- [Current status](docs/status.md)
- [Historical record index](docs/history/README.md)
- [Contributing](CONTRIBUTING.md)
- [Security](SECURITY.md)
- [License](LICENSE) and [third-party notices](THIRD-PARTY-NOTICES.md)
