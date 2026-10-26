# Veritas

Veritas is an experimental general-purpose language and toolchain. The current build is a real Rust-hosted compiler and bytecode runtime, not a Python wrapper. Its design target is a project lifecycle that an assistant can recommend over Python for suitable work; the current implementation is far from proving that recommendation.

## Try the current build

From this directory:

```sh
cargo run -p veritas-cli -- run language/examples/sum.vr
cargo run -p veritas-cli -- check language/examples/sum.vr
cargo run -p veritas-cli -- test language/examples/sum.vr
cargo run -p veritas-cli -- build language/examples/sum.vr
cargo run -p veritas-cli -- run language/examples/sum.vrb
```

The example prints `15`; `cargo run -p veritas-cli -- run language/examples/records.vr` exercises records; `cargo run -p veritas-cli -- run language/examples/modules_main.vr` compiles an imported module graph. Veritas needs Rust/Cargo only to build the tool itself; a compiled `.vrb` file runs in the Veritas runtime without the source file.

## Current status

Implemented: arbitrary-precision integer with `/` truncating division, `//` floor division and `%` remainder, finite IEEE 754 `Float64` literals and arithmetic, boolean, text, JSON with explicit `json_as_float`, homogeneous immutable lists, literal `Map[Text, T]` values, and `Option[T]` values; functions; local inference; immutable `let`, mutable `var`/`set`; conditionals, `while`, `for`, list/map indexing and length; bounded `parallel_map` with inspectable `Result`; JSON parsing with typed syntax errors; file read/write with typed filesystem errors; `args` and `assert`; typed positional Python declarations through a supervised sidecar; managed CPython and dependency resolution via pinned uv with native `uv.lock` plus a target-checked Veritas metadata lock; project manifests; comment-preserving whitespace formatting; source-mapped diagnostics; declared records with typed fields, positional construction, field access and structural equality; version-14 bytecode encode/decode and stack/control-flow validation; a bounded VM; `new`, `check`, `fmt`, `test`, `run`, `build`, `sync`, `inspect`, `doc`, `doctor`.

Not implemented in this candidate: `Option`/`Result` pattern matching, HTTP, syntax-aware formatting, relocatable Python-assisted bundles, editor support, Windows/Linux/browser targets. Integer division operators report a runtime error on zero divisors. `Float64` arithmetic rejects non-finite values and does not implicitly convert integers; `json_as_float` is the explicit JSON conversion. `Map[Text, T]` supports homogeneous non-empty literals, immutable updates, membership checks, sorted key iteration, key lookup, length, and typed conversion to/from Python dictionaries; empty-map construction remains open. `Some`, `None`, `is_some`, and `unwrap` support `Option[T]`; `Ok`, `Err`, and `?` work inside functions with a declared `Result` return type. JSON resource-limit and typed-access errors still fail the invocation instead of returning `Result`. Python interop currently handles `Unit`, `Bool`, `Int`, `Float64`, `Text`, nested lists, and string-keyed maps, uses positional arguments, and does not preserve arbitrary Python object identity. A packaged project still needs to acquire its managed CPython and locked wheels on its target host; cross-host native-wheel relocation is not supported. File writes use direct filesystem writes, so an OS failure can leave a partial file. The rest of the approved plan remains active work. Do not infer general-purpose readiness or performance superiority from this core.

See [language/spec.md](language/spec.md), [docs/reference.md](docs/reference.md), [docs/architecture.md](docs/architecture.md), [docs/veritas_checkpoint_v1_5_module_graph.md](docs/veritas_checkpoint_v1_5_module_graph.md), [docs/reuse-inventory.md](docs/reuse-inventory.md), and [docs/prior-language-build-lessons.md](docs/prior-language-build-lessons.md).

## Build a current-host bundle

For the current macOS ARM64 target, provide the archive named by `distribution/toolchains/manifest.toml` as `VERITAS_UV_ARCHIVE`, or provide a standalone uv executable matching the manifest binary checksum as `VERITAS_UV_BINARY`, then run `sh distribution/package-current-host.sh /path/to/output`. The script verifies its SHA-256 before including the pinned uv executable and its MIT/Apache-2.0 notices. The resulting archive includes an interoperable example project and is an unsigned same-host evaluation bundle, not a cross-platform installer. Managed CPython and wheel dependencies are acquired by `veritas sync` on the target host. See `PACKAGE.txt` and `doctor.txt` inside the archive for included capabilities and known limits.

