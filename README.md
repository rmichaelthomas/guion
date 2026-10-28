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

The current Guion-identity release matrix has passed on macOS ARM64 and Debian 12 GNU/Linux on ARM64 and x86_64 with glibc. Other operating systems, CPU architectures, and libc variants remain unverified.

**Latest performance checkpoint: 2026-10-27.** The measured build at commit `6b5e5aba` (artifact format 44) was tested on macOS ARM64 against CPython 3.14.7. Guion and Python both passed **127/127** frozen application cases.

| Workload | Guion result | Comparison |
| --- | --- | --- |
| Richards, uncached source | 27.46 ms | 72.92 ms Python; ratio **0.375** (95% interval 0.370–0.379), passing the 0.60 target |
| Five short-source tasks | All gates passed | Ratios **0.182–0.236**, 21 randomized pairs each |
| Nine everyday applications | Guion faster in all nine selected cases | Ratios **0.137–0.845**, seven randomized pairs each |
| 200,000-row JSONL | 3.81 s uncached source; 11.1 MiB peak RSS | 17.93 s and 74.4 MiB for the Python data reference |
| Lockfile auditor | Exact output through 16,384 packages / 32,768 diagnostics | 171.82 ms, 40.92 MiB vs. Python's 179.87 ms, 47.13 MiB |

N-body, Mandelbrot, both parallel CPU sizes, 20,000-row JSONL, and all application outputs also passed their checks. The full [current-main checkpoint](evaluation/completion/results/real-task-sprint/2026-10-27-current-main-outcome-checkpoint.md) links the raw samples and describes the methods and limits.

The same current-format run exercised bounded HTTP concurrency in the calendar and health-check applications, including mixed service responses. A runtime regression confirms that when one concurrent HTTP worker fails, an in-flight sibling request is interrupted and its socket closes. This supports those named cancellation cases on macOS; it is not a general async-runtime or cross-target claim. See the [concurrency report](evaluation/completion/results/real-task-sprint/2026-10-27-parallel-http-cancellation.md).

On the 100,000-row NumPy/orjson bridge task, the 2026-10-28 15-pair follow-up measured Guion `Bytes` at **15.1% faster** than its `Text` path and **138.49 ms** versus **136.05 ms** for the same package-backed Python function. The paired ratio was **1.020** (95% interval **0.988–1.031**), so this run establishes neither a Guion win nor a loss against that package function. The standard-library Python reference remains faster at ratio **1.199** (95% interval **1.177–1.225**). Every output matched byte-for-byte. This single-host result is not a broad language-speed claim; see the [latest report](evaluation/completion/results/real-task-sprint/2026-10-28-python-bridge-single-copy.md) and [raw samples](evaluation/completion/results/real-task-sprint/2026-10-28-python-bridge-single-copy.json). The earlier [serialization pass](evaluation/completion/results/real-task-sprint/2026-10-27-python-bridge-serialization.md) records its own separate measurement.

A four-lane no-op control kept package imports and the `Bytes` call constant while comparing empty and 5.1 MB inputs. At zero bytes, Guion and Python were at parity (ratio **0.993**, 95% interval **0.979–1.019**); at 5.1 MB, Guion was **4.3% slower** (ratio **1.043**, **1.004–1.050**). The relative payload penalty was **1.045** (**1.001–1.088**), locating the remaining difference in work that scales with transferred bytes. Every output matched. This does not attribute CPU time among file reading, pipe transfer, and frame handling; see the [control report](evaluation/completion/results/real-task-sprint/2026-10-28-python-bridge-startup-sizes.md) and the earlier [5.1 MB probe](evaluation/completion/results/real-task-sprint/2026-10-28-python-bridge-startup-5mb.md).

The next frame-reader change reduced measured temporary allocation for one 5.1 MB frame from **10.20 MB** to **5.10 MB**. A fresh 15-pair application run remained at parity with package-backed Python (ratio **1.019**, 95% interval **0.995–1.046**); an independent-cohort comparison found no measurable timing change. See the [frame-reader report](evaluation/completion/results/real-task-sprint/2026-10-28-python-bridge-readinto.md).

A direct-read fast path was tested and reverted because it showed no measurable speed or memory improvement over `readinto`. A streaming-reader prototype also failed the one-copy allocation bound for a fragmented 5.1 MB attachment. Repeated-call profiling found that native-image snapshots cost 0.220 ms per call; gating full snapshots on imports, `ctypes.dlopen`, or dyld image-count changes reduced the 1,000-call ratio from 4.131× to 1.481×. The bridge now also uses `orjson` for request parsing and response encoding when a Python program already imported it, with a standard-library fallback and no added dependency. In the latest 15-pair cohort, the 1,000-call ratio fell from 1.513× to 1.405×, and estimated excess cost fell by 9.84 μs per added call (95% same-index cross-cohort bootstrap interval: 2.50–13.17 μs lower). Guion remains slower at 1,000 calls. The latest profile measures 24.1 μs of response wait, including 19.9 μs before the first byte; that path remains the next optimization target. See the [phase-profile report](evaluation/completion/results/real-task-sprint/2026-10-28-python-bridge-phase-profile.md), the [fast JSON profile samples](evaluation/completion/results/real-task-sprint/2026-10-28-python-bridge-fast-json-phase-profile.json), the [repeated-call report](evaluation/completion/results/real-task-sprint/2026-10-28-python-bridge-repeated-calls.md), and the [allocation experiment](evaluation/completion/results/real-task-sprint/2026-10-28-python-bridge-direct-read.md).

These results come from named workloads on one Mac; operating-system file-cache state was uncontrolled. The current format-44 checkpoint does not validate other targets. It supports a real performance claim for the workloads shown, while overall competitiveness remains open.

Current-format managed packaging covers the mixed-dependency app on macOS ARM64 and the 100,000-row NumPy/orjson analysis on macOS ARM64 and Debian 12 ARM64 and x86_64. Each analysis bundle relocated to a path with spaces and Unicode and launched twice with matching reference output and no external Python or uv settings. Their uncompressed directory sizes were 157,585,367 bytes on macOS, 244,199,858 bytes on Debian ARM64, and 274,853,607 bytes on Debian x86_64; these are not download sizes. A separate current-format Feed Digest package adds pure-Python feedparser 6.0.14 coverage: all six source cases and six relocated-package cases matched the Python reference on macOS ARM64 and Debian 12 ARM64 and x86_64. Debian ARM64 ran natively in the existing Colima VM without Rosetta; Debian x86_64 used Rosetta, not QEMU. The bundles include complete license records and no native extensions from feedparser; their uncompressed directories are 172,422,430 bytes on macOS, 128,874,649 bytes on Debian ARM64, and 158,943,069 bytes on Debian x86_64. These are package results, not speed claims. See the [Feed Digest macOS report](evaluation/completion/results/real-task-sprint/2026-10-27-feed-digest-managed-package.md), the [Debian ARM64 report](evaluation/completion/results/real-task-sprint/2026-10-28-feed-digest-managed-package-debian-aarch64.md), the [Debian x86_64 report](evaluation/completion/results/real-task-sprint/2026-10-28-feed-digest-managed-package-debian-x86_64.md), the [managed analysis package reports](evaluation/completion/results/real-task-sprint/2026-10-27-current-format-managed-package-analysis.md) for macOS, [Debian ARM64](evaluation/completion/results/real-task-sprint/2026-10-27-current-format-managed-package-analysis-debian-aarch64.md), and [Debian x86_64](evaluation/completion/results/real-task-sprint/2026-10-27-current-format-managed-package-analysis-debian-x86_64.md), and the [current status](docs/status.md). Broader package compatibility, bridge performance, and language learnability remain open; see the [north-star design](docs/north-star-design.md) and [validation record](docs/pre-release-readiness.md).

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
