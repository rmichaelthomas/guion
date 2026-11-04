<p align="center">
  <a href="https://rmichaelthomas.github.io/guion/">
    <picture>
      <source media="(prefers-color-scheme: dark)" srcset="brand/wordmark-dark.svg">
      <img src="brand/wordmark-light.svg" alt="Guion" width="300">
    </picture>
  </a>
</p>

<p align="center"><strong>A general-purpose programming language for software that is easier to write, understand and run.</strong></p>

<p align="center">
  <a href="https://github.com/rmichaelthomas/guion/releases"><img alt="Latest release" src="https://img.shields.io/github/v/release/rmichaelthomas/guion?include_prereleases&label=release&color=2148e8"></a>
  <a href="LICENSE"><img alt="MIT license" src="https://img.shields.io/badge/license-MIT-11251d"></a>
  <img alt="Built with Rust" src="https://img.shields.io/badge/built_with-Rust-11251d?logo=rust">
  <a href="https://rmichaelthomas.github.io/guion/"><img alt="Try it in your browser" src="https://img.shields.io/badge/try_it-in_your_browser-d9f987"></a>
</p>

<p align="center">
  <a href="https://rmichaelthomas.github.io/guion/">Website</a> ·
  <a href="docs/getting-started.md">Getting started</a> ·
  <a href="docs/reference.md">Reference</a> ·
  <a href="https://github.com/rmichaelthomas/guion/releases">Download</a>
</p>

---

Guion (say GUY-on) is a programming language for everyday work: scripts, command-line tools, data processing, web requests, and full applications. It aims to be easy to read, hard to get wrong, and fast to run.

Guion is early. The first preview build is out for Linux, and the language is changing quickly. Every speed and quality claim the project makes is measured, and the results are published with their raw data, including the places where Guion still falls short.

## A first look

```guion
record Item:
    name: Text
    price: Int
    quantity: Int

fn line_total(item: Item) -> Int:
    return item.price * item.quantity

fn main() -> Int:
    let cart = [Item("notebook", 4, 3), Item("pen", 2, 5)]
    var total = 0
    for item in cart:
        set total = total + line_total(item)
    print(total)
    return 0
```

```console
$ guion run main.gu
22
```

Mistakes are caught before the program runs. Each one points at the exact spot and carries a stable code that people and tools can look up:

```console
$ guion check mistake.gu
mistake.gu:2:19: error[E_TYPE_OPERATOR]: operator `+` does not accept these operand types
```

You can try these examples and others [in your browser](https://rmichaelthomas.github.io/guion/#try) without installing anything.

## Why Guion

- **Easy to read.** Indentation-based syntax that feels familiar if you know Python, with types that are written where they help and inferred where they don't.
- **Errors you can act on.** Programs are checked before they run. Every error has a stable code and a suggested repair, as text for people and as JSON for tools and AI assistants.
- **One command for the whole project.** `guion` creates, checks, formats, lints, tests, runs, builds, packages, installs and documents programs. There is nothing else to install or configure.
- **Permissions by default.** A program can't touch files, the network or the environment unless it's given permission. You can see what a program is able to do before you run it.
- **Fast.** On measured everyday workloads, from text processing to JSON and CSV, Guion runs faster than CPython. The results are below.
- **Built in itself.** The Guion compiler is written in Guion and runs on a Rust runtime. Compiled programs are portable `.gub` files.
- **Runs in the browser.** The same runtime compiles to WebAssembly, which is how the website runs your code.

## Install

### Linux (preview)

Download the archive for your machine from the [v0.1.0-preview release](https://github.com/rmichaelthomas/guion/releases/tag/v0.1.0-preview). Builds are for Debian 12–compatible systems on ARM64 and x86_64.

```sh
tar -xzf guion-debian-x86_64.tar.gz     # or guion-debian-aarch64.tar.gz
cd guion-debian-x86_64
sh install.sh                           # installs under ~/.local; pass a path to choose another place
```

Each archive has a matching `.sha256` checksum file. You don't need Rust or Python installed.

### macOS and other systems

There's no macOS package yet. You can build from source with [Rust](https://rustup.rs):

```sh
git clone https://github.com/rmichaelthomas/guion.git
cd guion
cargo build --release --locked -p guion-cli
./target/release/guion --version
```

## Quick start

```sh
guion new hello        # create a project
guion run hello        # prints: Hello, Guion!
guion test hello       # run its tests
guion check hello/src/main.gu
```

The [getting-started guide](docs/getting-started.md) goes further.

## How it compares with Python

On November 3, 2026, Guion ran faster than CPython 3.14.7 on every text, JSONL and CSV workload in the project's speed gate. The gate ran on both x86_64 and ARM64 Linux servers, at 20,000 and 200,000 rows. A ratio below 1.0 means Guion took less time.

| Workload, 200,000 rows | x86_64 | ARM64 |
| --- | --- | --- |
| Text: split, trim, replace, find | 0.96× | 0.81× |
| JSONL event summary | 0.86–0.90× | 0.84–0.86× |
| CSV ledger | 0.86–0.88× | 0.79–0.80× |

Earlier tests on a Mac found Guion 2.7× faster on the Richards benchmark and faster in all nine everyday applications tested. These results cover the workloads named here, not every program. The [speed-gate report](evaluation/completion/results/real-task-sprint/2026-11-03-server-speed-gate.md) and the [website](https://rmichaelthomas.github.io/guion/#receipts) give the method, the raw samples and the limits.

## Where Python is still ahead

Python is mature and widely used. Here is where it's still ahead of Guion today:

- **Libraries and packages.** Guion ships 27 standard-library modules, including JSON, CSV, TOML, SQLite, HTTP, files, text, time, random and regex, plus a package manager and a package registry with a signed log. Nothing has been published to the registry yet.
- **Arrays and data science.** Guion has no native array type yet, so NumPy and pandas have no Guion counterpart.
- **A stable release.** The current build is a Linux preview, with no macOS package and no stability promise yet.

## Documentation

- [Getting started](docs/getting-started.md)
- [Language and command reference](docs/reference.md)
- [Language specification](language/spec.md)
- [Guide for AI coding assistants](docs/agents/start-here.md) and [runnable recipes](docs/agents/recipes.md)
- [Architecture](docs/architecture.md)
- [Browser WebAssembly interface](docs/browser-wasm-abi/README.md)
- [Compatibility and migration](docs/compatibility-and-migrations.md)
- [Current status and all measured results](docs/status.md)
- [Changelog](CHANGELOG.md)

## The name

Guion is named in honour of [Guion S. "Guy" Bluford Jr.](docs/namesake.md), the NASA astronaut who in 1983 became the first African American to fly in space. It's pronounced GUY-on.

## Contributing

Issues and pull requests are welcome; see [CONTRIBUTING.md](CONTRIBUTING.md). Please report security problems privately as described in [SECURITY.md](SECURITY.md).

## License

Guion is released under the [MIT License](LICENSE). Third-party components are listed in [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).
