# Start here: programming Guion

This guide is for an agent that has the Guion repository and needs to build a small program without relying on this conversation. Guion is pre-release; the [current status](../status.md) describes what is and is not verified.

For a compact, single-file syntax, signature, effect, error-code, and recipe reference, see the [Guion agent reference](guion-reference.md).

## Build the Guion command

From the repository root, Rust and Cargo build the CLI. Python is not required to compile or run a core Guion program.

```sh
cargo run --locked -p guion-cli -- --version
cargo run --locked -p guion-cli -- new /tmp/hello-guion
cargo run --locked -p guion-cli -- check /tmp/hello-guion/src/main.gu
cargo run --locked -p guion-cli -- test /tmp/hello-guion/src/main.gu
cargo run --locked -p guion-cli -- run /tmp/hello-guion
```

The starter prints `Hello, Guion!`. Cargo builds the Guion tool; normal Guion source runs use the in-process Rust compiler. Guion also maintains a self-hosted compiler. Do not use Python to compile Guion source.

## Mental model

- A source file is a module. Functions declare parameter and return types; local values are inferred from their initial value.
- `let` creates an immutable binding. `var` creates a mutable binding, changed with `set`.
- `Option[T]` means a value may be absent. `Result[T, E]` means an operation may succeed or fail. Use `match`, or postfix `?` inside a function that returns a compatible `Result`; reserve `unwrap` for values whose success was already established.
- Lists and maps are immutable values; update operations return a new value.
- File, network, environment, and clock operations follow explicit host permissions. Network and clock access are denied unless granted.
- Guion is a real programming language with structured syntax. “Plain English” is the goal for the learning experience and diagnostics, not a promise that arbitrary English is executable code.

The [language contract](../../language/spec.md) is the authority on implemented syntax and runtime behavior. The [built-in signature catalog](../../language/builtin-signatures.json), [standard-library source](../../language/stdlib/), and [host API contract](../../language/host-api.md) are the API sources of truth.

## Inspect effects and grants

Run `guion inspect --json src/main.gu` to see the statically reachable effects, such as `filesystem.read`, `filesystem.write`, `network.http`, and `clock.read`. The report follows reachable calls and `parallel_map` workers in the compiled program; it does not run the program. The compiler can identify that an HTTP operation is reachable, but it cannot know which host a runtime-supplied URL from `args()` will name.

The caller controls permissions. A project can list exact hostnames in `guion.toml` under `[network].hosts`; a caller can add a one-run host with `guion run --allow-http-host api.example.test`. In JSON, `required_effects` lists the reachable categories, `manifest_grants` identifies the adjacent manifest and its declared hosts/clock setting, and `grant_assessment` describes what those declarations establish. A runtime-supplied URL is reported as `present_but_target_dynamic` even when the manifest has hosts; no manifest means a later CLI grant is `unknown_external_runner`. The CLI grants and file roots passed to a later `run` are not supplied to `inspect`. The runtime checks the URL host and redirects against its grants. Host matching does not replace operating-system egress controls: DNS rebinding and network isolation remain host responsibilities.

HTTP responses depend on the API: `http_request_bytes` returns a typed `HttpResponse` record with status, headers, and a byte-preserving body. The older text `http_request` returns a checked JSON envelope. The runnable HTTP recipe uses the typed byte API and prints the body length; the current standard library does not yet convert `Bytes` to `Text`.

For text files, import `std:fs` and use `fs_read_text(path)` when you want filesystem failures returned as `StdError` with the requested path attached. The lower-level `read_text(path)` built-in returns `FileError`.

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

`guion run` reuses a project-local compiled source cache when its source graph, lock state, standard library, compiler identity, target, and runtime grants match. The cache is stored under `.guion/cache/` and is ignored by Git. Use `guion run --no-cache TARGET` to force source compilation, or `guion cache clean [PROJECT]` to remove Guion-managed cache entries. Deleting `.guion/cache/` is safe; Guion recreates entries as needed.

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

For offline app packaging, provide an installed matching interpreter with `GUION_PYTHON_INSTALL_DIR`, a preloaded uv cache with `GUION_PYTHON_CACHE_DIR`, and set `UV_OFFLINE=1` to forbid downloads. The cache is copied into the app as a whole; a project-scoped cache keeps unrelated wheels out of the bundle. See the [command reference](../reference.md), the [managed native-wheel package report](../../evaluation/completion/results/real-task-sprint/2026-10-27-current-format-managed-package-analysis.md), and the [Feed Digest pure-Python package report](../../evaluation/completion/results/real-task-sprint/2026-10-27-feed-digest-managed-package.md).

## Runnable agent recipes

[`recipes.md`](recipes.md) links to five runnable programs: file/data processing, HTTP with a host grant, SQLite plus TOML, bounded concurrent work, and four deliberate failure paths. Their paths and expected outputs also appear in [`index.json`](index.json).
