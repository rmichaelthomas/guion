# Getting started

Guion 0.2.0 is downloadable from the project's GitHub releases. It is the first release that is not a preview, and it is still pre-1.0: the [stability policy](stability-policy.md) says what you can rely on between releases and what you cannot.

## Install a prebuilt package

Prebuilt packages are on the project's [GitHub releases](https://github.com/rmichaelthomas/guion/releases/tag/v0.2.0). The `v0.2.0` release has packages for Debian 12-compatible Linux on ARM64 (`guion-debian-aarch64.tar.gz`) and x86_64 (`guion-debian-x86_64.tar.gz`), and for macOS on Apple Silicon (`guion-macos-aarch64.tar.gz`). Each archive has a `.sha256` checksum file beside it. No Rust, Python or uv is needed:

```sh
sha256sum -c guion-debian-aarch64.tar.gz.sha256
tar -xzf guion-debian-aarch64.tar.gz
sh guion-debian-aarch64/install.sh        # installs under ~/.local; pass a prefix to choose another
~/.local/bin/guion --version
```

On macOS, use `shasum -a 256 -c guion-macos-aarch64.tar.gz.sha256` and the `guion-macos-aarch64` directory instead. The Mac build is signed and notarized by Apple, so it opens without a warning; the first launch checks with Apple and needs an internet connection. For other systems, build from source.

## Build from source

To build from source, install Rust and Cargo, then open a terminal in the Guion repository. Cargo builds the CLI and its Rust runtime. The standard core build does not need Python.

## Create and run a project

From the repository root, create a small project, check it, run its starter test, and run the program:

```sh
cargo run --locked -p guion-cli -- new /tmp/hello-guion
cargo run --locked -p guion-cli -- check /tmp/hello-guion/src/main.gu
cargo run --locked -p guion-cli -- test /tmp/hello-guion/src/main.gu
cargo run --locked -p guion-cli -- run /tmp/hello-guion
```

The program prints `Hello, Guion!`. The generated project has this shape:

```text
hello-guion/
├── guion.toml
├── README.md
└── src/
    └── main.gu
```

`guion.toml` names the entry source file and project version. `.gu` files contain Guion source; `.gub` files are compiled artifacts. To use a prebuilt `guion` executable, run the same commands without the leading `cargo run --locked -p guion-cli --`.

## Read and change the starter

A Guion function declares its input and result types. `let` makes a value that cannot be reassigned; `var` makes a value that can be changed with `set`.

```guion
fn main() -> Int:
    let greeting = "Hello, Guion!"
    print(greeting)
    return 0
```

Edit `src/main.gu`, then repeat `check`, `test`, and `run`. To see the language's current supported syntax, types, host permissions, and limits, read the [language contract](../language/spec.md). The broader [reference](reference.md) describes CLI commands and examples.

Source runs keep a bounded compiled cache in the project’s `.guion/cache/` directory. It is safe to delete; Guion rebuilds it when source, dependencies, compiler identity, target, or run permissions change. Use `guion run --no-cache PROJECT` to force compilation, or `guion cache clean PROJECT` to remove Guion’s cached source artifact.

## Host permissions

File access is limited to the project or artifact directory unless an extra root is explicitly granted. Network access is denied by default; a project must grant exact hostnames in `guion.toml`. Clock access also requires a grant. These limits help keep programs predictable, but they are not operating-system security sandboxes.

For the optional Python bridge, the CLI must be built with the `python-interop` feature. The bridge is not required for core Guion programs. See [compatibility and migration](compatibility-and-migrations.md) before using a project inherited from Veritas.

## Useful next steps

- Run `guion fmt FILE.gu` to format a source file.
- Run `guion lint FILE.gu` to check for unused imports and locals and unreachable statements.
- Run `guion build FILE.gu` to create a `.gub` artifact.
- `guion doctor` reports the current host and enabled profile in human-readable form. `guion doctor --json` emits the versioned report for the active build profile; the [agent index](agents/index.json) lists both report shapes.
- Use `guion package PROJECT --output DIRECTORY` to create a current-host application bundle.

The CLI command list and behavior are in the [reference](reference.md). Guion is pre-1.0, so these commands may change in a minor release; the [stability policy](stability-policy.md) lists what is promised.
