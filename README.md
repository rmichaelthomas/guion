# Guion

Guion is a general-purpose programming language and toolchain built to make useful software easier to write, understand, and run. It has clear, typed syntax, a compiler written in Guion, and a Rust runtime. Normal source runs use the in-process Rust compiler. The default build works without Python.

The long-term ambition is to make Guion a practical choice over Python for everyday programming, with quick responses and a broad range of useful work. Its programs use structured syntax; “plain English” describes the learning and tooling goal, not free-form natural-language code. The comparisons below are evidence for named workloads, not proof of broad superiority.

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
- The standard library can stably sort records by an integer key and merge feeds that are already sorted by that key.
- The portable shared VM also exposes a versioned WebAssembly ABI for typed browser calls. ABI 1.0 preserves the legacy exports, grants no host capabilities, and bounds requests, values, output, and execution; see the [browser ABI guide](docs/browser-wasm-abi/README.md).
- The core CLI can create, check, format, lint, test, run, build, package, install, and inspect programs.
- Optional Python interoperability exists for projects that need it. It is off in the default core build.

See the [getting-started guide](docs/getting-started.md), [language and command reference](docs/reference.md), and [language contract](language/spec.md).

## Status and comparison with Python

**Latest checkpoint: 2026-10-27.** Current main commit `6b5e5aba` (artifact format 44) was measured on macOS ARM64 against CPython 3.14.7. Guion and Python both passed **127/127** frozen application cases.

| Workload | Guion result | Comparison |
| --- | --- | --- |
| Richards, uncached source | 27.46 ms | 72.92 ms Python; ratio **0.375** (95% interval 0.370–0.379), passing the 0.60 target |
| Five short-source tasks | All gates passed | Ratios **0.182–0.236**, 21 randomized pairs each |
| Nine everyday applications | Guion faster in all nine selected cases | Ratios **0.137–0.845**, seven randomized pairs each |
| 200,000-row JSONL | 3.81 s uncached source; 11.1 MiB peak RSS | 17.93 s and 74.4 MiB for the Python data reference |
| Lockfile auditor | Exact output through 16,384 packages / 32,768 diagnostics | 171.82 ms, 40.92 MiB vs. Python's 179.87 ms, 47.13 MiB |

N-body, Mandelbrot, both parallel CPU sizes, 20,000-row JSONL, and all application outputs also passed their checks. The full [current-main checkpoint](evaluation/completion/results/real-task-sprint/2026-10-27-current-main-outcome-checkpoint.md) links the raw samples and describes the methods and limits.

The same current-format run exercised bounded HTTP concurrency in the calendar and health-check applications, including mixed service responses. A runtime regression confirms that when one concurrent HTTP worker fails, an in-flight sibling request is interrupted and its socket closes. This supports those named cancellation cases on macOS; it is not a general async-runtime or cross-target claim. See the [concurrency report](evaluation/completion/results/real-task-sprint/2026-10-27-parallel-http-cancellation.md).

A separate current-format bridge check processed 100,000 rows through NumPy and orjson with exact output parity. The fresh Guion run measured **173.5 ms**, compared with **149.3 ms** for the same package function run directly in Python and **119.0 ms** for a standard-library Python reference. This proves useful package access for one real workload; the bridge adds measurable overhead and has not yet beaten Python on this task. See the [bridge report](evaluation/completion/results/real-task-sprint/2026-10-27-python-bridge-package-analysis.md).

These results come from named workloads on one Mac; operating-system file-cache state was uncontrolled. The current format-44 checkpoint does not validate other targets. It supports a real performance claim for the workloads shown, while overall competitiveness remains open.

The remaining product outcomes are broader Python interoperability and complete managed-package delivery, language learnability, relocatable native dependencies, and current-format cross-target validation. See [current status](docs/status.md), the [north-star design](docs/north-star-design.md), and the [validation record](docs/pre-release-readiness.md) for the wider scope.

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
