# Language and command reference (current build)

For supported artifact, package, Python and host versions plus upgrade guidance, see [`compatibility-and-migrations.md`](compatibility-and-migrations.md).

The current format is 44. The runtime also reads supported earlier artifact formats; see the compatibility guide for the exact range.

## Minimal program

```guion
fn main() -> Int:
    let greeting = "Hello"
    print(greeting + ", Guion!")
    return 0
```

Function parameters and return types are explicit; local types are inferred. Use `var` for a binding that changes and `set` to update it. `let` is immutable. Function calls are statically resolved. `print` writes one line.

`List[T]` holds same-typed values. Use `[first, second]` for a literal, `values[index]` for zero-based indexing, `for value in values:` to iterate, and `len(values)` to count. Text length and indexing count Unicode scalar values; `text_chars(text)` materializes those scalars as a bounded `List[Text]` for linear scans.

`Option[T]` represents a value that may be absent. Return `Some(value)` or `None` from a function declared as `Option[T]`, test presence with `is_some`, and use `unwrap` after that check. `guion run language/examples/option.gu` demonstrates the flow.

## Local date and clock access

Import `std:time` and call `today_local() -> Date` when an application needs the current local calendar date. Clock access is denied by default. Enable it for a project with `[clock] enabled = true` in `guion.toml`, or for one run with `guion run --allow-clock`. The returned date follows the host process time zone and its daylight-saving database. The lower-level `unix_time_ms() -> Int` returns milliseconds since the Unix epoch. See the installed `language/host-api.md` for the exact capability contract.

## Commands

The default build is the Python-free core. Core commands do not discover or launch Python or uv. The Python bridge and managed Python dependency manager are available only in a build made with the explicit `python-interop` feature.

- `guion new DIRECTORY` creates a starter project and refuses an existing path.
- `guion check FILE.gu` compiles and reports the function count. Add `--json` after the path for a versioned success report or a structured source diagnostic with a stable code and line/column.
- `guion test FILE.gu` runs each zero-argument `Unit` function named `test_*` in its own fresh runtime instance. Add `--json` after the path for a stable JSON report on stdout with schema version, per-test status, pass/fail counts, and source traces for runtime failures. Failed tests still exit nonzero; stderr remains available for host-tool output.
- `guion run FILE.gu` compiles and executes source.
- `guion run --json FILE.gu [ARGS...]` returns a versioned JSON run report containing captured program output, exit status, or compiler/runtime diagnostics with source locations. Its `effects` list reports captured output and host operations with `started`, `returned`, `returned_error`, or `interrupted` outcomes; `omitted_effects` counts records beyond the 1,024-entry cap. Targets are capped at 256 Unicode scalar values and set `target_truncated` when shortened. HTTP targets report method and host without URL paths, query strings, or credentials. Python-assisted runs also report native image paths observed as newly resident at Python call boundaries in `python_native_images`, with the observation method in `python_native_image_observation`. The current macOS observer uses dyld snapshots; it can miss images loaded and unloaded between snapshots. Other hosts report `unsupported`. The JSON flag precedes the file path so remaining arguments pass unchanged to the program.
- `guion fmt FILE.gu` tokenizes code outside comments and quoted strings, normalizes spacing, and preserves comment and string contents. `guion fmt --check FILE.gu` reports unformatted files without writing.
- `guion lint FILE.gu` compiles first, then reports unused imports, unused locals, and unreachable statements as warnings with stable codes (`W_UNUSED_IMPORT`, `W_UNUSED_LOCAL`, `W_UNREACHABLE_STATEMENT`). Add `--json` for a versioned report. Compiler errors remain errors from `check`.
- `guion run language/examples/json_extract.gu` demonstrates bounded JSON parsing, exact integer access, and stable serialization.
- `guion run language/examples/float64.gu` demonstrates finite floating-point arithmetic and explicit JSON number conversion.
- `guion run language/examples/integer_division.gu` demonstrates truncating division, floor division, and divisor-signed remainder.
- `guion run language/examples/http_get.gu URL` demonstrates bounded HTTP text retrieval; grant the URL's exact hostname under `[network].hosts` first.
- `guion run PROJECT_DIRECTORY` resolves the entry file from `guion.toml`.
- `guion run [--file-root DIRECTORY] TARGET [ARGS...]` adds a file-access root for this run. Reads search the project or artifact root first, then the added roots; writes use the last added root. Packaged `./run` launchers add the caller's working directory by default, so CLI programs can read and write user files without placing them inside the installed bundle. Set `GUION_FILE_ROOT` to choose a different additional directory. Relative guest paths and parent-traversal restrictions still apply.
- `guion build FILE.gu [-o FILE.gub]` writes a versioned portable bytecode artifact (default output replaces `.gu` with `.gub`). Artifact metadata records the compiler version, build host, exact source graph digest, any present dependency lock fingerprints, and effective runtime ceilings; ceilings follow the artifact when it is moved.
- `guion package PROJECT_DIRECTORY [--output DIRECTORY]` creates a current-host, source-free application directory containing the compiled artifact, matching Guion runtime, project policy, launcher, and declared resources. Core packages do not bundle Python or uv. Python-assisted projects require the optional feature and retain its pinned-runtime packaging behavior. Verified core package targets are macOS ARM64 and Debian 12 GNU/Linux on ARM64 and x86_64, all with glibc; the command refuses to overwrite an existing destination.

Linux packages include `linux-dependencies.json`, which records ELF dependencies, runtime resolutions, required glibc versions, architectures, hashes, and available distribution provenance. The report is an audit aid; it does not observe libraries opened only through runtime APIs.
- `guion verify-package PACKAGE_DIRECTORY` checks the package target and recorded SHA-256 digests for the artifact, runtime, resources, managed Python tree, pinned `uv`, Python lock, dependency and native reports, copied license files, and native images, including native files in the offline wheel cache. The package launcher skips this full-tree hash scan by default to keep short commands responsive; set `GUION_VERIFY_PACKAGE=1` to run it before launch. The reports are unsigned, so they detect accidental corruption but do not authenticate the publisher. Native dependency reports contain target-specific static load records; dynamic Python image observations are emitted on macOS through `run --json`.
- `guion sync PROJECT_DIRECTORY` without `--locked --offline` is reserved for projects using the optional `python-interop` feature. In a core build, use `guion sync --locked --offline` to validate native packages and explicitly migrate a legacy Python-only lock while preserving its Python metadata.
- `guion run FILE.gub` executes a compiled artifact without source.
- `guion inspect FILE.gu` or `.gub` prints artifact and function metadata, including recorded build limits when present, plus statically reachable runtime effects. `guion inspect --json FILE.gu` emits schema-versioned function call/effect data and opaque Python declarations. This is bytecode reachability analysis for known host operations, not a sandbox, a proof of Python behavior, or a complete observation of native code and operating-system effects.
- `guion doc FILE.gu` prints function, record, and optional external signatures. `guion doc --json FILE.gu` emits a versioned `guion_api_report` with source parameter names and checked types, reachable effects, standard-library availability, native package identities, and optional profile requirements.
- `guion build --json FILE.gu [--output ARTIFACT]` emits a versioned build report with artifact format, size and SHA-256, source digest, compiler version, and host target. Compile failures use the structured diagnostic format. Human-readable build output remains the default.
- `guion package --json PROJECT [--output DIRECTORY]` emits a package report with target, package schema, artifact digest, metadata digest, resource count, and whether a Python runtime was bundled. Packaging failures emit a stable diagnostic; source compile failures retain their compiler code and source location. `guion verify-package --json DIRECTORY` reports verified package identity or a structured verification failure. Both commands remain human-readable by default.
- `guion doctor` reports the current host and enabled profile. `guion doctor --json` emits a versioned report; a core build reports that Python interoperability is disabled and does not inspect Python or uv, while an optional bridge build reports interpreter and toolchain readiness.
- `guion add NAME --path DIRECTORY` adds a local native package; `guion add NAME --url HTTPS_URL --version VERSION --sha256 HASH` adds a content-verified archive. `guion update` explicitly resolves native package sources and transactionally updates the lock and managed cache. `check`, `run`, and `build` never rewrite locks.
- `guion package --library SOURCE_DIRECTORY --output DIRECTORY` creates a reusable native library package. `guion install PACKAGE_DIRECTORY [--prefix DIRECTORY]` verifies and installs an app package into `share/guion/apps/NAME/VERSION` and creates its generated command under `bin/NAME`. The default prefix is `~/.local`; unrelated existing executables are preserved.

For optional managed Python calls, build Guion with `--features python-interop`, then declare an exact `[python].version` and a `dependencies` array in `guion.toml`. The core build never discovers or launches Python or uv.

For offline app packaging, `GUION_PYTHON_INSTALL_DIR` can point to an already installed matching CPython tree, and `GUION_PYTHON_CACHE_DIR` can point to a preloaded uv cache. Package assembly copies the specified cache into the app's portable `.uv-cache` before syncing its locked dependencies. Set `UV_OFFLINE=1` to require cache-only packaging; the command fails if a required artifact is missing. This copies the full selected cache, so use a project-scoped cache when package size matters. Without these settings, the optional Python package path may acquire the pinned interpreter and wheels during packaging.

Pure-Guion packages can include runtime data files explicitly:

```toml
[package]
resources = ["assets/schema.json", "data/lookup.csv"]
```

Resource paths must stay inside the project, name files, and avoid package-owned paths. List imported Python modules alongside data files in `[package].resources`; package execution imports them from the package root. The package writes `PACKAGE.json` with target and SHA-256 digests. Schema 2 includes integrity hashes for immutable Python offline-cache payloads. uv bookkeeping files matching `.lock`, `*.lock`, `*.http` and `*.msgpack` are excluded because offline runs update them. Schema 1 packages remain verifiable. Run the result with `./run [arguments]`. Files not listed as resources are not copied. Relocation is verified on macOS ARM64 and Debian 12-compatible GNU/Linux ARM64 for selected Python wheels. Target-specific audits attribute installed native files through wheel `RECORD`; they do not prove ownership of vendored source code, and static scans cannot observe libraries opened only through runtime APIs.

HTTP is denied unless a project manifest grants exact hosts. The first operation is text GET:

```toml
[network]
hosts = ["feeds.example.org"]
timeout_ms = 15000
max_response_bytes = 16777216
max_redirects = 5
```

`http_get(url)` returns `Result[Text, HttpError]`; errors are inspectable with `http_error_code` and `http_error_message`. `http_request(method, url, headers, body)` returns a JSON response envelope and uses the project's network timeout. `http_request_with_timeout(method, url, headers, body, timeout_ms)` sets a per-request deadline; the effective deadline is the smaller of the requested value and `[network].timeout_ms`. A nonpositive or unrepresentable timeout returns `R_HTTP_TIMEOUT`. Both request functions enforce exact-host grants, request/response byte limits and same-host redirect limits. Streaming and binary response bodies remain unimplemented.

The per-request timeout operation requires artifact format 27. A host may cap the requested deadline through its `HostPolicy`; guest code cannot raise that ceiling.

The current-format feed-digest workload in `evaluation/development/guion_feed_digest` combines bounded concurrent HTTP, managed `feedparser`, a project-local Python adapter, and Guion output handling. Its loopback-only host grant is for the local acceptance fixture; edit the exact host allowlist before using it with external feeds. The acceptance harness in `evaluation/tests/test_feed_digest.py` compares output with an independent Python baseline and checks concurrency, order stability, Unicode, malformed feeds, HTTP failures, changed input and denied hosts. The original Veritas-named project is retained separately at `evaluation/development/feed_digest` as a compatibility fixture.

Runtime errors go to stderr and return a failing process status. Output produced before a runtime error is preserved on stdout. Diagnostics include stable error codes and source line/column locations.

`print(value)` writes program output to stdout. `eprint(value)` writes program diagnostics to stderr. Both are captured independently by `guion run --json`, reported as distinct effects, and charged to the combined output limit. `eprint` requires artifact format 28.

## Current limits

The `std:text` `text_split` and `text_join` helpers combine pieces in bounded groups to reduce repeated copying for larger inputs. `text_split` with an empty delimiter returns the original text as one field; `text_join` of an empty list returns empty text. They retain Unicode-scalar behavior and remain subject to the normal instruction and allocation budgets.

Runtime resource ceilings can be set in a project's `guion.toml` under `[runtime]` and are also available on the embedding `HostPolicy`. Every field must be a positive integer; unknown fields fail with a manifest error. The default cumulative allocation budget is 268435456 estimated bytes per invocation. It counts input value footprints, collection/record/text/byte/JSON construction, host results, and output rendering across nested calls. Interpreter/JIT frames, operand stacks, and scalar arithmetic temporaries are excluded so backend selection does not change this limit; integer growth has its own digit ceiling. Parallel workers share a counter and leave a portion available to handle their result. Exhaustion reports `R_ALLOCATION_LIMIT`. The estimate covers guest data at known creation points; it is not exact native heap/RSS accounting, and OS/runtime/third-party allocations may differ. Other limits such as instruction count, call depth, output, file bytes, text bytes, collection items, and integer digits remain independent.

Embedders may cancel a running invocation by creating a `guion_runtime::CancellationToken`, cloning it to a control thread, and calling `Instance::invoke_with_cancellation(export, args, token)`. VM code checks between instructions; native code checks at entry and backward branches; parallel workers stop scheduling and are joined; Python calls are terminated and reaped after cancellation is noticed (polled every 20 ms). A blocking HTTP call is observed only after it returns or reaches its configured timeout. Completed side effects are not undone, so cancellation does not mean rollback. Use a fresh token per invocation. The command-line program currently follows normal process Ctrl-C behavior rather than returning `R_CANCELLED`.

```toml
[runtime]
allocation_limit_bytes = 268435456
instruction_limit = 2000000
max_parallelism = 8
```

The current types are `Int`, `Float64`, `Bool`, `Text`, `Bytes`, `Json`, `JsonError`, `FileError`, `HttpError`, `Unit`, `TaskError`, `List[T]`, `Map[Text, T]`, `Option[T]`, and `Result[T, E]`. `Int` uses decimal arbitrary-precision storage subject to a configurable digit limit. Integer `/` truncates toward zero, `//` rounds down, and `%` follows the divisor's sign, satisfying `a == (a // b) * b + (a % b)`; division by zero raises `R_DIV_ZERO`. `Float64` is finite IEEE 754 binary64; `+ - * /`, unary negation, equality and ordered comparisons require Float64 operands. `sqrt(Float64)` returns a finite square root, reporting `R_MATH_DOMAIN` for negative input and `R_FLOAT_NONFINITE` for a non-finite result. No implicit conversion occurs between `Int` and `Float64`. Non-finite literals and operation results fail; float division by zero raises `R_DIV_ZERO`.

Format-16 artifact metadata records numeric resource ceilings and source/dependency provenance. Format 17 adds `Bytes` values and `read_bytes`/`write_bytes`; format 18 adds source-level Python keyword calls; format 19 adds the HTTP GET host operation; format 20 adds typed JSON construction and updates; format 21 adds atomic text replacement; format 22 adds `args_from` for CLI subcommands; format 23 adds bounded HTTP methods, headers, request bodies, and response envelopes; format 24 adds manifest-granted environment reads; format 25 adds typed empty list and map literals; format 26 adds immutable `list_push`/`list_set` and `sqrt(Float64)`; format 27 adds a per-request HTTP timeout capped by host policy; format 28 adds native guest stderr with separate effect reporting; format 29 adds typed JSON column extraction; format 30 adds streaming JSON column projection; format 31 adds Unicode-scalar `text_chars`; format 32 adds copy-on-write local list append; format 33 adds copy-on-write local JSON-array append; format 40 adds pure local-time resolution against Guion's packaged timezone database using tags 158 and 159. Format 41 adds stable `sort_by_int_key` and `merge_sorted_by_int_key`, which evaluate a named `(T) -> Int` key once per item and charge native comparisons against the guest instruction budget, and alias-safe local `json_set` assignment with a dedicated VM opcode. Format 42 assigns local-time resolution tags 161 and 162 so it coexists with format 41 instructions. Format 43 adds native `text_join`; format 44 adds streaming `json_stringify_pairs`. Artifacts that use these features require their respective format. At runtime, the effective ceiling is the stricter of the artifact limit and the host's `HostPolicy`; a host can tighten a limit but cannot loosen a limit promised by the artifact. The source SHA-256 includes sorted relative module paths and exact UTF-8 source bytes. If both `guion.lock` and `.guion/python-bridge/uv.lock` exist, their combined SHA-256 is recorded; absent locks are reported as absent. Format-15 metadata and format-14 artifacts remain readable. Format-14 has no embedded policy. Metadata hashes identify build inputs; they do not authenticate a publisher or make artifacts tamper-proof. Compiler compatibility ranges, native targets, and Guion package dependency identities are not yet recorded.

Built-ins include `args()`, `args_from(start)`, `read_text(path)`, `write_text(path, content)`, `write_text_atomic(path, content)`, `read_bytes(path)`, `write_bytes(path, content)`, `http_get(url)`, `assert(condition)`, `len(list_or_map_or_bytes)`, `list_push(list, value)`, `list_set(list, index, value)`, `sort_by_int_key(list, key_function)`, `merge_sorted_by_int_key(sorted_feeds, key_function)`, `sqrt(Float64)`, `map_set(map, key, value)`, `map_has(map, key)`, and `parallel_map(list, function_name, worker_limit)`, plus JSON bulk conversions `json_int_column`, `json_text_categories`, `json_as_int_list`, and `json_stringify_pairs`, and format-30 streaming projection via `json_project_columns` / `json_text_categories_values`. `sort_by_int_key` requires a named pure `(T) -> Int` function, computes its key once per item, uses a stable sort, and preserves the input and its aliases. `merge_sorted_by_int_key` stably merges already-sorted lists using the same key-function contract and rejects a feed that is not sorted by that key. `list_push` appends and `list_set` replaces an existing zero-based index; both return immutable values. Assigning `list_push(name, value)` or `list_set(name, index, value)` back to that same mutable local uses copy-on-write storage and clones only when an alias must retain the old value. Assigning `json_set(name, key, value)` back to that same mutable `Json` local uses an alias-safe copy-on-write update to reduce temporary object copying. `args_from` returns the CLI argument suffix beginning at a zero-based index. Bytes are immutable, preserve all values including invalid UTF-8 and NUL, and use the `max_file_bytes` ceiling. Maps support literals with unique text keys, homogeneous values, deterministic sorted-key iteration, immutable updates, key lookup and `len`; empty literals need an expected `Map[Text, T]` type. Empty lists likewise need an expected `List[T]` type, which can come from a local annotation, return type, function parameter, enclosing collection, or typed operation. Typed `Map[Text, T]` values cross Python calls as dictionaries with string keys. Python interop also supports finite `Float64`, `Bytes` (request values use private raw-byte attachments and arrive as Python `bytes`; Python byte returns remain base64 encoded), `Json`, top-level NumPy arrays returned against a `List[T]` declaration, and source-level keyword arguments mapped to declared parameter names. NumPy boolean/numeric arrays up to 64 bits are materialized with `tolist()` and checked against the declared nested list shape and type; arrays are limited to 98,304 elements before materialization and data is copied. A `Json` bridge value preserves nested heterogeneous JSON and arbitrary-precision integers; Python cycles, non-finite values, non-string object keys and non-JSON objects are rejected. Omitted arguments are passed through as omitted so Python defaults can apply. JSON construction uses `json_null`, `json_bool`, `json_int`, `json_float`, `json_text`, `json_array`, `json_object`, `json_push`, and `json_set`; constructors and immutable updates require artifact format 20. `write_text_atomic` requires format 21 and performs atomic visibility replacement without writer locking.

`Option` constructors `Some(value)` and `None` are available inside functions declared to return `Option[T]`; use `is_some(option)` and `unwrap(option)`, which raises `R_OPTION_UNWRAP` for absence. Text and byte file operations return `Result` values; inspect errors with `file_error_code/message/line/column`. Missing paths return `R_FILE_NOT_FOUND`, permission failures return `R_FILE_PERMISSION`, and file-size policy failures remain runtime errors. `write_text` and `write_bytes` write directly; `write_text_atomic` atomically replaces a text file but does not coordinate multiple writers or promise directory-entry durability after sudden power loss. `Ok`, `Err`, and postfix `?` are available in functions with a declared `Result` return type; Result and Option values can be handled with exhaustive statement matches; see below. General Result inference remains unimplemented. `json_parse` returns `Result[Json, JsonError]`; malformed syntax and duplicate keys are inspectable through `json_error_code/message/line/column`. Resource-limit and typed access failures remain runtime errors. Other JSON operations include `json_stringify`, `json_stringify_pairs`, `json_get`, `json_index`, `json_has`, `json_len`, `json_as_text`, `json_as_int`, `json_as_float`, `json_as_bool`, and `json_is_null`. `json_stringify_pairs` streams a grouped text map into a JSON array of `{key: value}` rows in deterministic key and list order, with caller-selected field names. `json_as_float` converts a JSON number to finite `Float64`; nonnumeric or non-finite conversion fails with `R_JSON_TYPE`. `parallel_map` returns `Result[List[U], TaskError]`; inspect it using `is_ok`, `unwrap`, `error`, `task_index`, `task_code`, and `task_message`. An `extern python "module:callable" fn alias(...) -> Type` declaration calls through a supervised sidecar. A project with `[python]` uses the exact locked managed interpreter; otherwise use an explicit `GUION_PYTHON` path. Current Python conversions cover `Unit`, `Bool`, `Int`, finite `Float64`, `Text`, `Bytes`, `Json`, and recursively nested lists/maps with string keys; calls support positional and keyword arguments mapped to declared parameters, and omitted parameters are left to Python defaults. Python failures are runtime errors. The CLI grants file access within the project root and any explicit `--file-root` directories; the additional roots are still restricted to relative paths that remain inside a grant. Embedded use must grant a root explicitly. This root check is not a sandbox. Network host grants do not prevent DNS rebinding or enforce operating-system egress controls.


## Records

Records declare an ordered set of typed fields. Construction supplies values in declaration order; access uses the field name. Values are immutable and equality is structural for values of the same declared record type.

```guion
record Point:
    x: Int
    y: Int

fn main() -> Int:
    let origin = Point(0, 0)
    let next = Point(origin.x + 1, origin.y)
    print(next.x)
    return 0
```

Record schemas and values were introduced in version-13 artifacts; format 14 added source maps, format 15 added initial build metadata, and format 16 added metadata schema versioning and source/dependency identities. Records currently cannot cross the Python interop boundary. See `language/examples/records.gu` for an executable example.

## Modules

An entry module can import another file by a relative path. Imported modules expose only declarations marked `pub`; their exported functions and records are available by name in the importing module. Private helpers stay private. The compiler rejects import cycles, path escapes, and duplicate declaration names across the graph.

```guion
import "math.gu"

fn main() -> Int:
    print(double(21))
    return 0
```

See `language/examples/modules_main.gu` and `language/examples/modules_math.gu`. Compile a project directory or entry source file with imports so Guion can resolve the full graph. Runtime errors retain the source file that produced them.

## Option and Result matching

`match` is a statement for `Option[T]` and `Result[T, E]`. It requires exactly one arm for each variant, allows the arms in either order, and scopes each bound name to its arm. Wildcards, nested patterns, other matched types, and match expressions are not supported yet.

```guion
match calculation:
    Ok(value):
        print(value)
    Err(message):
        print(message)
```

See `language/examples/matching.gu` for both variants and both arm orders.

## Installing a current-host release

After extracting a Guion distribution, run `sh install.sh` to install the versioned files under `~/.local/share/guion/` and create `~/.local/bin/guion`. Pass a prefix to choose another location. The installer leaves existing unrelated `bin/guion` commands intact and does not edit shell startup files. Add the reported `bin` directory to `PATH` using the method appropriate for your shell. Installing a new distribution version updates the managed launcher and keeps prior version directories available. This installer currently targets the packaged host only.

The embedding API exposes `Runtime::vm_only()` for deterministic diagnosis and VM/JIT comparisons; `Runtime::new()` enables automatic JIT selection for eligible pure numeric functions.
# Bootstrap compiler interface

The experimental compiler-to-artifact interchange used by the Guion self-hosting bootstrap is documented in [bytecode IR interchange](bytecode-ir.md). It is not a stable public ABI.
