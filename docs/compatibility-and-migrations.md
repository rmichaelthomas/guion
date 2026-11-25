# Compatibility and migration

Guion is pre-1.0. Source syntax, public APIs, bytecode, and package metadata may change before 1.0; there is no long-term compatibility promise yet. What is promised within a release series, and how deprecation works, is in the [stability policy](stability-policy.md).

## Current Guion names

| Purpose | Current name |
| --- | --- |
| Source | `.gu` |
| Compiled artifact | `.gub` |
| Project manifest | `guion.toml` |
| Project lock | `guion.lock` |
| Local project state | `.guion/` |
| Core command | `guion` |

New projects, generated files, packages, locks, and local state use these names.

## Reading projects from Veritas

The rename is a pre-1.0 transition. The current CLI retains read compatibility for selected old project inputs:

- `veritas.toml` and `veritas.lock` can be read when a project has not yet adopted the Guion filenames.
- Existing `.vr` source files are accepted as legacy project inputs.
- Prior `.veritas/python-bridge` state can be located for the optional Python bridge.
- Existing compiled artifacts that use the `VRTA` artifact magic remain readable, including prior files named `.vrb`.

This compatibility is a migration aid, not a guarantee that old names will remain supported through 1.0. New projects and new outputs use Guion names. A Guion project may continue to contain a legacy-named manifest while it is being migrated; avoid creating both old and new manifest or lock files in the same project, because that makes project identity ambiguous.

## Moving from 0.8.0 to 0.9.0

Nothing is removed or renamed, and no word is newly reserved. Programs written for 0.8.0 compile and run unchanged except for the one change below.

- **`Request` has a seventh field, `peer`.** In `std:http_response`, `Request` is `Request(method, path, headers, query, body, params, peer)`. A `Request(...)` record literal needs the extra argument: add `peer: ""`, or positionally `Request("GET", "/", {}, [], body, {}, "")`. Code that only reads a `Request`, or builds one with `request_new`, is unaffected; `request_new` sets `peer` to `""` and `http_request_value` fills it from the connection. The `web` package is version 0.3.0 and passes it through its router and middleware.
- **Artifact format 90.** The runtime reads formats 14 through 90 and writes 90. A program that uses `text_lower`, `text_upper`, `json_keys`, `json_stringify_pretty`, `fs_set_mode`, `fs_mode`, `int_to_text_radix`, `text_parse_int_radix`, `tcp_peer`, `http_server_peer` or `process_stop_requested` needs a runtime that reads its format and is refused by an older one with `E_ARTIFACT_VERSION`. `web` 0.3.0 needs Guion 0.9.0.
- **`serve` and `serve_for` stop on signals.** In a program that polls for signals, `INT`, `TERM` and `HUP` now make them finish the requests being handled and return `Ok`; a program that never polls is unchanged.
- **`match` literal patterns.** `Int`, `Text` and `Bool` literals are patterns now. A name that was bound in an arm is still a name; only a literal is new.

## Moving from 0.7.0 to 0.8.0

Nothing is removed or renamed, and no word is newly reserved. Programs written for 0.7.0 compile and run unchanged except for the three stricter behaviours below.

- **Artifact format 89.** The runtime reads formats 14 through 89 and writes 89. A program that uses the new `std:crypto` functions, bit operations, `sleep_ms` or `Bytes` indexing needs a runtime that reads its format and is refused by an older one with `E_ARTIFACT_VERSION`. Packages that use them (`beacons`) need this release.
- **Enum names inside `Some`, `Ok` and `Err`.** A name there that equals a declared enum variant now means that variant: `Some(Red)` matches `Red`, where before it bound a new local called `Red`. A pattern that names a variant of the wrong type is `E_MATCH_TYPE`. Rename a local that shadowed a variant.
- **`crypto_tsa_verify`.** The TSA certificate's `timeStamping` extended key usage must be marked critical (RFC 3161 section 2.3); a certificate whose extension is not critical gives `None`. The DigiCert, Sectigo and FreeTSA certificates are all critical.
- **`web` package.** `csrf_middleware` refuses an unsafe-method request whose `Origin` (or, without one, `Referer`) does not name its own `Host`. Behind a proxy that rewrites `Host`, those requests now get a 403 until the proxy passes `Host` through. The package is version 0.2.0.

## Moving from 0.6.0 to 0.7.0

Nothing is removed, renamed or re-signed, and no word is newly reserved: a 0.6.0 program compiles and runs unchanged. `together`, `start`, `wait`, `send`, `close` and the other new names still work as your own function, variable and record names.

- **Artifact format 76.** The runtime reads formats 14 through 76 and writes 76. A program that uses `together:`, channels, `try_parallel_map` or background commands needs a runtime that reads format 76 and is refused by an older one with `E_ARTIFACT_VERSION`.
- **Worker limit.** `parallel_map` now allows 64 workers on every machine, not one per core. `[runtime] max_parallelism` can raise it to 1024.
- **`std:process` has three new records.** `Workers`, `Process` and `Ready` come with `import "std:process"`; a program that imports it and declares a record with one of those names needs it renamed.
- **Named record arguments.** `Box(height=2, width=1)` is accepted for any record; positional calls are unchanged.
- **Unix and Windows.** Background commands run in their own process group on Unix and in a job object on Windows. Only the Unix behaviour is covered by the full test suite; see [the design notes](design-structured-concurrency.md).
- **Browser.** `together:` and `try_parallel_map` need threads and report that in the browser playground.

## Moving from 0.5.0 to 0.6.0

Nothing is removed, renamed or re-signed, and no word is newly reserved: a 0.5.0 program compiles and runs unchanged.

- **Artifact format 75.** The runtime reads formats 14 through 75 and writes 75. A program that uses `Map[Int, T]` needs a runtime that reads format 75 and is refused by an older one with `E_ARTIFACT_VERSION`.
- **Generic records.** `record Pair[A, B]:` and `Pair[Int, Text]`; instances are made at compile time, so the artifact format does not change for them.
- **Browser.** In the browser VM, function values, the higher-order builtins and the sorts work; `parallel_map` fails with `R_HOST_UNSUPPORTED`; see [browser.md](browser.md).
- **Record and replay.** A program that uses `parallel_map` now writes tape format 2 (each item has its own lane) and replays with any worker limit; other programs still write format 1, and both formats replay.

## Moving from 0.4.0 to 0.5.0

Nothing is removed or renamed. New words are `enum`, `match` over enum variants and the generic-call forms; a 0.4.0 program that used any of them as a name needs renaming (the compiler says which).

- **Artifact format 74.** The runtime reads formats 14 through 74 and writes 74. A program that uses a closure needs a runtime that reads format 74 and is refused by an older one with `E_ARTIFACT_VERSION`.
- **New built-ins and commands.** `try_map_list`, `guion lsp`, `guion repl`, and `--record`, `--replay` and `--replay-changed` for `guion run` and `guion test`.
- **Builds for Intel Macs and Windows.** Added alongside Linux and Apple Silicon; see Getting started. A Homebrew tap and a one-line installer script are new.

## Moving from 0.3.0 to 0.4.0

Nothing is removed or renamed, and no word is newly reserved: a 0.3.0 program compiles and runs unchanged. The one re-signing is the `std:sqlite` effect change below. `language/surface-0.4.0.json` records the 0.4.0 public surface, and `cargo xtask surface-diff --allow-resign std:sqlite.` against `surface-0.3.0.json` shows additions plus that one change.

- **Artifact format 73.** The runtime reads formats 14 through 73 and writes 73. An artifact built by 0.3.0 (format 72 or lower) still loads and runs. A program that uses `std:http_static` needs a 0.4.0 runtime and is refused by an older one with `E_ARTIFACT_VERSION`.
- **New modules.** `std:http_response`, `std:cookie`, `std:http_body` and `std:http_static`, and two functions in `std:http_server` (`http_request_value`, `http_server_send_response`). A program of your own that defines a function with one of these names and does not import the module is unaffected.
- **Content types with a line break.** `http_server_respond`, `http_server_reply` and `http_server_send` now return `E_HTTP_HEADER_INVALID` for a content type containing CR or LF, where 0.3.0 wrote it into the response head. Remove the line break; a correct program is unaffected.
- **`guion update` wording.** A capability widening is reported one line per kind of access (`network: +hosts ["api.example.com"]`) instead of a sentence per item. The refusal, the unchanged lock and `--approve-capabilities` are as before.
- **Capabilities are denied until approved (a break).** A project approves nothing by default: `guion add`, `update`, `check`, `build` and `run` refuse a package that declares or reaches capabilities beyond the project's `[approved-capabilities.NAME]` table, and a project with no table approves none. `guion add NAME --approve-capabilities` (or `guion update --approve-capabilities`) records what the package declares; you can also write the table by hand. A package that declares no capabilities and reaches no effects needs nothing. A project that already links a package with capabilities must approve them once.
- **SQLite is file access (a break).** `guion doc` and `guion inspect` now report `filesystem.read` and `filesystem.write` for `std:sqlite` functions (0.3.0 reported none), so those functions are re-signed. A package that uses SQLite must list `files` in its `[capabilities]`, and `cargo xtask surface-diff --allow-resign std:sqlite.` is how the snapshot comparison names the change.
- **Path dependencies are relative.** `guion add NAME --path PATH` writes the dependency as a path relative to the project (for example `../fetcher`) in `guion.toml` and `guion.lock`, where 0.3.0 wrote an absolute path. Manifests written by 0.3.0 with absolute paths still work.
- **Templates.** A file named `NAME.gut` is now compiled by the CLI to `NAME.gut.gu` next to it. A project that already has a file with that name is refused with `TEMPLATE_OUTPUT_EXISTS` rather than overwritten. Add `*.gut.gu` to the project's ignore file if you keep generated files out of version control.
- **Native `Bool` arguments.** A `Bool` passed to a function that runs as machine code could arrive as `true` when the caller ran in the interpreter (for example a `main` that also prints). 0.4.0 passes it correctly. A program that worked around this does not need to change, and a built artifact needs no rebuild.
- **Packages.** `guion.lock` stays at format 2. The `web` package 0.1.0 is on the registry: `guion add web --approve-capabilities`, or `--path PATH` for a checkout.

## Moving from 0.2.0 to 0.3.0

- **`break` and `continue` are reserved.** A 0.2.0 program that used either word as a variable, parameter or function name stops compiling in 0.3.0. Rename the identifier and run `guion check`; the compiler points at the statement. Outside a loop, the two words are `E_NO_LOOP`. No other source construct changed meaning, and the public surface recorded in `language/surface-0.3.0.json` only adds names.
- **Artifact format 72.** The runtime reads formats 14 through 72 and writes 72. An artifact built by 0.2.0 (format 64 or lower) still loads and runs. A program that uses a 0.3.0 operation (`sort`, function values, `crypto_sha256`, `crypto_random_*`, `sqlite_begin_read`, `std:log`, `std:compress`, `std:tar`, `std:yaml`) is written at format 65 to 72 and is rejected by a runtime older than that format with `E_ARTIFACT_VERSION`. Rebuild artifacts with the 0.3.0 compiler to use the new library surface.
- **New `guion.toml` keys.** `[runtime] decompress_limit_bytes` (default 1 GiB) bounds every inflate and tar extraction; `[log] level` sets the default `std:log` level. Existing manifests need no change.
- **Diagnostics grew additive fields.** `R_STEP_LIMIT` reports `used` and `limit`; `guion check --json` diagnostics may carry `suggestions`; `guion doc --json` entries may carry `deprecated` and `summary`. Tools that read these reports should ignore fields they do not know.
- **Packages.** `guion.lock` stays at format 2. `frames` 0.1.0 still needs format 64 or later and works with 0.3.0.

## Supported artifact and package boundaries

Runtime limits changed in the same development series without a format change: the `guion` command line now has no default instruction limit and treats `allocation_limit_bytes` as a bound on memory in use. Projects that relied on the old 2,000,000-instruction default to stop runaway programs should set `[runtime] instruction_limit` explicitly. Artifacts built earlier recorded a 2,000,000-instruction limit in their metadata and keep it when run, because a host cannot loosen an artifact's limit; rebuild them to drop it. The current compiler writes artifact format 64 and the runtime reads artifact formats 14 through 64. Format 64 adds `CsvReadColumns` (op tag 195), the operation behind `csv_read_columns`; a format-63 artifact cannot use it and is rejected if it does. Format 63 appends nine array function codes (`Take`, `Factorize`, `FirstIndex`, `CountBy`, `SumBy`, `MinBy`, `MaxBy`, `MeanBy`, `JoinIndex`) to the array instruction; a format-62 artifact cannot use them and is rejected if it does. Format 62 adds the `Array[e]` type (type tag 18), the array instruction family (op tag 193) and in-place local array updates (op tag 194); an artifact that uses arrays cannot be written at an earlier format, and artifacts without arrays are otherwise unchanged. Format 61 adds the 64-bit random instructions that let `std:random` run on the machine-code path (with unchanged sequences) and mutual-TLS client and server handshakes in `std:net`. Format 60 records the packages a build links (name, exact version and content hash) in its build metadata; artifacts built without packages are unchanged. Format 59 adds SHA-1 and byte XOR for the WebSocket support in `std:http_server` and `std:websocket`; format 58 adds TLS client and server handshakes in `std:net`; format 57 adds TCP and UDP socket operations behind `std:net`; format 56 adds subprocess and signal operations behind `std:process`; format 55 adds the regular-expression operations behind `std:regex`; format 54 adds the filesystem operations behind `std:fs` and `std:path`; format 53 adds byte inspection, base64, hex, UTF-8 and percent-encoding functions behind `std:encoding` and `std:url`; format 52 adds Float64 math, numeric conversions, and the SplitMix64 mixing step behind `std:math` and `std:random`; format 48 added direct UTC date formatting from an `Instant`; formats 47, 46, and 45 added JSON decoding from HTTP bytes, Unicode text slicing, and checked UTF-8 decoding for byte HTTP responses. An artifact cannot contain an operation introduced after its format version, and existing compatible artifacts remain readable. Current packages use schema 2; the verifier accepts schemas 1 and 2. Artifact format compatibility does not promise that the source compiler version is checked at runtime. Hashes detect accidental changes but do not authenticate the publisher.

Package and lock formats are unchanged: `guion.lock` stays at format 2, and a library's `[capabilities]` declaration is recorded as an extra table in its lock entry, which earlier readers ignore. Existing package names and exact-version locks keep loading; a name that breaks the new rules (uppercase, over 64 characters, reserved) produces a warning for an existing lock and is refused for a new `add`, `new --lib` or `publish`. Version ranges apply only to registry packages; manifests that pin an exact `path` or `url` dependency are read as before. No migration command is needed for this series.

The Guion-identity pre-release matrix passed on macOS ARM64 and Debian 12 GNU/Linux on ARM64 and x86_64 with glibc, including same-target relocation. Other operating systems, CPU architectures, Linux distributions beyond Debian 12, and libc implementations beyond glibc remain unverified. Consult [status](status.md) and the [pre-release readiness report](pre-release-readiness.md) before relying on a target.

## Python interoperability

The default core build does not discover, launch, or bundle Python or `uv`. The optional bridge requires a build with `--features python-interop` and uses the exact interpreter and dependencies declared by the project. New Guion configuration uses `guion.toml`; `GUION_PYTHON` is the explicit interpreter override when no managed interpreter is configured. Old Veritas project configuration is read for transition compatibility.

For an existing project, first keep a backup of its source, manifest, lock files, and application data. Run `guion doctor`, then `guion check` and the project's tests. Preserve its existing locks unless you intentionally change dependencies. A lock migration or dependency update must be explicit and reviewed before packaging. Application data migration remains the application's responsibility.

## Package and installation behavior

`guion package PROJECT --output DIRECTORY` builds a current-host application bundle. `guion verify-package DIRECTORY` checks its recorded hashes. `guion install DIRECTORY [--prefix PATH]` installs a verified bundle under the chosen prefix, defaulting to `~/.local`; it does not edit shell startup files. Guion 0.2.0 is published on the project's GitHub releases with Linux and Apple Silicon macOS archives. The [stability policy](stability-policy.md) says what is promised about these commands.
