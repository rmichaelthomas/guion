# Guion core

This archive contains the Guion command-line tool and its self-hosted compiler for the listed ARM64 release target. The core commands do not require Python. Guion is in pre-release development; this archive is a preview build, not a stable release.

## Install

From the extracted archive directory, run `sh install.sh` to install under `~/.local`, or pass a prefix such as `sh install.sh /chosen/prefix`. The installer creates `bin/guion` and keeps the versioned files under `share/guion/`. Add the chosen prefix's `bin` directory to your shell's `PATH` if you want to call `guion` directly. The installer does not edit shell startup files.

To run directly from the extracted archive, use `./bin/guion`. For example:

```sh
./bin/guion new /tmp/hello-guion
./bin/guion check /tmp/hello-guion/src/main.gu
./bin/guion run /tmp/hello-guion
```

## Included guides

- [Getting started](docs/getting-started.md)
- [Agent programming path](docs/agents/start-here.md) and [runnable recipes](docs/agents/recipes.md)
- [Language reference](docs/reference.md) and [language contract](language/spec.md)
- [Architecture](docs/architecture.md)
- [Compatibility and migrations](docs/compatibility-and-migrations.md)
- [Current status and evidence](docs/status.md)
- [Self-hosted compiler](docs/self-hosted-compiler.md)

The MIT license and third-party notice index are included beside this file. Release archive identity and dependency notices are recorded in the release manifest and `licenses/`.
