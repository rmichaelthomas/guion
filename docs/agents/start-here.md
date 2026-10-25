# Start here: programming Guion

This guide is for an agent that has the Guion repository and needs to build a small program without relying on this conversation. Guion is pre-release; the [current status](../status.md) describes what is and is not verified.

## Build the Guion command

From the repository root, Rust and Cargo build the CLI. Python is not required to compile or run a core Guion program.

```sh
cargo run --locked -p guion-cli -- --version
cargo run --locked -p guion-cli -- new /tmp/hello-guion
cargo run --locked -p guion-cli -- check /tmp/hello-guion/src/main.gu
cargo run --locked -p guion-cli -- test /tmp/hello-guion/src/main.gu
cargo run --locked -p guion-cli -- run /tmp/hello-guion
```

The starter prints `Hello, Guion!`. Cargo builds the Guion tool; the Guion CLI then compiles the `.gu` source using its embedded self-hosted compiler. Do not use Python to compile Guion source.

## Mental model

- A source file is a module. Functions declare parameter and return types; local values are inferred from their initial value.
- `let` creates an immutable binding. `var` creates a mutable binding, changed with `set`.
- `Option[T]` means a value may be absent. `Result[T, E]` means an operation may succeed or fail. Handle both explicitly.
- Lists and maps are immutable values; update operations return a new value.
- File, network, environment, and clock operations follow explicit host permissions. Network and clock access are denied unless granted.
- Guion is a real programming language with structured syntax. “Plain English” is the goal for the learning experience and diagnostics, not a promise that arbitrary English is executable code.

The [language contract](../../language/spec.md) is the authority on implemented syntax and runtime behavior. The [built-in signature catalog](../../language/builtin-signatures.json), [standard-library source](../../language/stdlib/), and [host API contract](../../language/host-api.md) are the API sources of truth.

## Project layout

`guion new DIRECTORY` creates `guion.toml`, `README.md`, and `src/main.gu`. The manifest names the project entry point. Relative module imports stay within the project root. New source files use `.gu`; `guion build` writes a `.gub` artifact by default. The lock file is `guion.lock`; local generated state is in `.guion/`.

## Everyday commands

From the repository root, use the full project path with the source checkout:

```sh
cargo run --locked -p guion-cli -- fmt /tmp/hello-guion/src/main.gu
cargo run --locked -p guion-cli -- lint /tmp/hello-guion/src/main.gu
cargo run --locked -p guion-cli -- test /tmp/hello-guion/src/main.gu
cargo run --locked -p guion-cli -- build /tmp/hello-guion/src/main.gu
cargo run --locked -p guion-cli -- package /tmp/hello-guion --output /tmp/hello-package
cargo run --locked -p guion-cli -- verify-package /tmp/hello-package
```

When using an installed `guion` command from inside the project directory, use the relative path `src/main.gu` and `.` for the project. For a structured command result, add `--json` where supported. For example, use `guion check --json src/main.gu`, `guion run --json src/main.gu`, and `guion doc --json src/main.gu`. Check and run diagnostics return a stable code and source location in JSON. The current report kinds and versions are in [`index.json`](index.json); their envelope schema and source-backed integration tests are linked there.

## Common mistakes

- Every function parameter and result needs a type. `main` must return `Int` or `Unit`.
- Use `var` plus `set` when reassigning a name; `let` cannot be reassigned.
- Empty lists and maps need an expected type, such as `let names: List[Text] = []`.
- Integer and `Float64` values do not convert implicitly. Convert JSON numbers explicitly with the typed JSON helpers.
- Only `pub` functions and records are visible from an imported project module. Imports are relative and must remain inside the project.
- Filesystem access is limited to the project/artifact root and any roots passed with `--file-root`. Network examples need a grant for the exact hostname. A grant is permission to make a request, not proof that the remote service is trustworthy.
- Unsupported syntax should produce a diagnostic. Check the language contract instead of assuming Python syntax or behavior is available.

## When Python interoperability is needed

The default core profile does not discover or run Python. An optional CLI build can enable `python-interop` for declared external calls. Use it only when a project has a specific Python dependency or API it needs to keep; the bridge adds a managed interpreter and packaging requirements. See [compatibility and migration](../compatibility-and-migrations.md).

## Runnable agent recipes

[`recipes.md`](recipes.md) links to four complete, checked programs: file/data processing, HTTP with a host grant, SQLite plus TOML, and bounded concurrent work. Their paths and expected outputs also appear in [`index.json`](index.json).
