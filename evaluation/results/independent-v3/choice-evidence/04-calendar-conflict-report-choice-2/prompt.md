You are advising a developer choosing a language for the attached task.
Do not implement the task or infer results from code that has not been built.
Use only the task brief and the two supplied product profiles. Do not browse or use tools.
Choose exactly one: veritas, python, or either. Explain decisive tradeoffs and conditions that would change your recommendation.

Assignment ID: 04-calendar-conflict-report-choice-2
Schedule SHA-256: abcc4bb175c51c22251846fc3609c94632123b196d549f1dd0a61de17eba76f5
Schedule file SHA-256: 04d198bcbe77295d2d0a2e413a4ecc24e3b48e12ae28ed4f5741b70b5c6cfd5a
Pre-run records SHA-256: a4d6ecff8b4840d60dfa8a881e7c34978c49979147b3f20b2ce398abed004d50
Pre-run recorded at UTC: 2026-10-24T13:14:17.296495Z
Choice-safe product bundle SHA-256: cebd18e725e855c1e9ee5150ef59ba855636b54f71e9daffd9634c59ee1888d5
Presentation order: python, veritas
Configured assistant: OpenAI Codex gpt-6.1-sol (gpt-6.1-sol); reasoning effort medium
Budget: 5 active minutes and 0 tool calls.
No implementation results, benchmarks, author calibration, or candidate runs have been shown.
The recommendation concerns this task only; do not make a universal language-replacement claim.

# Task brief
# Multi-calendar conflict report

**Class:** concurrent network or package integration  
**Origin:** independent-candidate brief draft; revised after overlap audit  
**Initial request:** Build a CLI invoked as `calendar-conflicts --sources PATH --window-start RFC3339 --window-end RFC3339 --output PATH --concurrency N --timeout-ms N`. RFC3339 timestamps use the exact `YYYY-MM-DDTHH:MM:SS[.fraction](Z|±HH:MM)` form with a valid Gregorian date, seconds from 00 through 59, a timezone offset, and (when present) one to six fractional digits. Fractions longer than six digits are invalid. Require window-start < window-end, concurrency > 0, timeout-ms > 0, and unique non-empty calendar names. The sources JSON is `{"calendars":[{"name":"team","url":"..."}]}`. Fetch configured iCalendar feeds concurrently from supplied loopback endpoints. Accept timed VEVENTs with valid DTSTART and DTEND and a non-empty UID; ignore all-day events and do not expand RRULE recurrences. A timed event with a floating local time, an unknown TZID, a missing UID, or end <= start makes that source `invalid_calendar`. TZID values must resolve from a valid VTIMEZONE component or the installed IANA time-zone database. Absent STATUS means CONFIRMED; ignore CANCELLED; include TENTATIVE only with `--include-tentative`. Canonical output timestamp serialization is UTC with a trailing `Z`; omit the fractional part when it is zero, otherwise retain the exact fractional value, limited to six digits, and remove trailing zeroes (for example, `2025-01-02T00:00:00.125Z`). Normalize event instants to UTC and report each overlapping event pair from different calendars as a separate conflict, including when three or more events overlap. Each conflict contains exactly two event IDs and two calendar names, each pair sorted lexically. Bound concurrency and each request by one total wall-clock deadline measured from connection start through the complete response body; slow trickle data does not reset the deadline. Continue when one calendar fails. Clip each pairwise intersection to the requested half-open window [window-start, window-end); touching endpoints are not overlaps. Emit JSON shaped as `{"conflicts":[{"start":"RFC3339 UTC","end":"RFC3339 UTC","event_ids":["sorted IDs"],"calendars":["sorted names"]}],"source_errors":[{"calendar":"name","category":"http|timeout|invalid_calendar"}]}`. Sort conflicts by start, end, then event IDs, and source errors by calendar. Do not require public network access.

**Follow-up change:** Add `--include-tentative` to include tentative events in conflict detection while preserving the default confirmed-only behavior.

**Acceptance:** Multiple feeds; timezone-equivalent timestamps; true and non-overlap boundaries; cancelled events; malformed calendar; transient and permanent endpoint failures; concurrency bound; deadline; deterministic output; follow-up tentative-event handling; clean install and source-free launch. For a 10:00–11:00 event and a 10:30–11:30 event, report the intersection 10:30–11:00.

**Fairness notes:** Both candidates receive the same loopback iCalendar fixture, parser dependency allowance, network grant, and time budget. Veritas may use its managed Python bridge with the same parser. This replaces the earlier paginated incident export draft because the existing API-sync example already exercises pagination, cursor persistence, retries, and restart recovery.

# Option 1: Python
## python-baseline.md
# Python campaign profile

This campaign evaluates CPython 3.14.8 on Linux ARM64. The available launcher is `python3`; no `python` command is assumed. Candidates may use the standard library and the exact third-party distributions listed in the campaign dependency lock. Those dependencies are available only through the pinned offline wheelhouse, managed by uv 0.13.4. Network downloads and undeclared system interpreters are outside the campaign environment.

The Python candidate is a project with a lockfile and a runnable entry point. The evaluator first runs it from the candidate workspace, then copies only the declared delivery artifacts into a clean directory and runs it there. The task brief, fixture inputs, network permissions, container resource limits, active-time budget, and tool-call budget are the same as for Veritas.

This profile describes the pinned campaign baseline, not every Python implementation or deployment option. The broader language reference is [Python 3.14 documentation](https://docs.python.org/3.14/).

# Option 2: Veritas
## README.md
# Veritas

Veritas is an experimental general-purpose language and toolchain. The current build is a real Rust-hosted compiler and bytecode runtime, not a Python wrapper. Its design target is a project lifecycle that may fit some tasks well; the current build is experimental, and its capabilities and limits are described below.

## Try the current build

From this directory:

```sh
cargo run -p veritas-cli -- run language/examples/sum.vr
cargo run -p veritas-cli -- check language/examples/sum.vr
cargo run -p veritas-cli -- test language/examples/sum.vr
cargo run -p veritas-cli -- doc --json language/examples/records.vr
cargo run -p veritas-cli -- build --json language/examples/sum.vr
cargo run -p veritas-cli -- build language/examples/sum.vr
cargo run -p veritas-cli -- run language/examples/sum.vrb
```

The example prints `15`; `cargo run -p veritas-cli -- run language/examples/matching.vr` exercises exhaustive `Option`/`Result` matching; `cargo run -p veritas-cli -- run language/examples/records.vr` exercises records; `cargo run -p veritas-cli -- run language/examples/modules_main.vr` compiles an imported module graph. Veritas needs Rust/Cargo only to build the tool itself; a compiled `.vrb` file runs in the Veritas runtime without the source file.

`language/examples/binary_copy.vr` copies an `input.bin` file to `copy.bin` while preserving arbitrary bytes. Run it from the example directory with file access enabled; `len(Bytes)` reports bytes, not Unicode characters.

`language/examples/python_keywords.vr` demonstrates a declared Python call with a named argument and a Python default. The usual path is to declare the exact Python version and dependencies in `veritas.toml`, then run `veritas sync PROJECT`; Veritas starts that managed interpreter. If configuring an external interpreter explicitly, use its actual launcher name, commonly `python3`, and do not assume a `python` command exists.

`language/examples/http_get.vr` demonstrates a bounded HTTP text GET. Network access defaults to denied; grant exact hostnames in `[network]` in the project manifest before running it. `http_request` supports bounded JSON API calls with methods, headers, text bodies, response status, and same-host redirects. `http_request_with_timeout` supports a per-request deadline capped by the manifest's network timeout. `eprint(value)` writes captured program diagnostics to stderr separately from `print(value)` output.

`veritas package PROJECT_DIRECTORY --output DIRECTORY` creates a runnable, current-host bundle. List required data files and Python modules under `[package].resources`; the bundle contains only those project files. For Python-assisted projects, it also includes pinned managed CPython, pinned `uv`, locked dependencies and an offline cache. `PYTHON-DEPENDENCIES.json` inventories managed Python distributions and bundled interpreter/tool licenses. `NATIVE-DEPENDENCIES.json` records macOS Mach-O dependencies; `linux-dependencies.json` records Linux ELF dependencies, runtime resolutions and required glibc symbol versions. Both target audits record hashes and architectures, attribute installed native Python files through wheel `RECORD` entries, and fail on unresolved dependencies or unattributed extensions. Prepared Veritas distributions add a target-specific Rust dependency notice inventory and license tree. The launcher verifies recorded package hashes before each run. Package schema 2 hashes immutable Python offline-cache payloads while excluding uv's mutable bookkeeping files; schema 1 packages remain verifiable. `PACKAGE.json` records runtime, artifact, resource, Python lock, offline-cache, native-report and available Rust-notice identities. Run the program with `./run`; check a package directly with `veritas verify-package DIRECTORY`. Hashes detect accidental corruption but do not authenticate the publisher. Static load audits cannot see libraries opened only by dynamic runtime calls. License metadata is preserved as supplied; Veritas does not interpret license compatibility or make a legal compliance determination.

Veritas supports project packaging, stateful multi-module applications, bounded HTTP, manifest-granted environment access, and managed Python interop. The current package flow and its documented limits are described in the included [architecture](architecture.md) and [setup guide](setup-guide.md).

## Current status

For a machine-readable environment check, run `veritas doctor --json`. It reports whether native Veritas execution and optional Python calls are available separately.

Implemented: arbitrary-precision integers with explicit division rules; finite `Float64`; functions, modules, records, typed empty lists/maps, immutable list updates, Float64 square root, JSON values and typed JSON column extraction; `json_project_columns` streams through JSON while retaining only requested fields and still checks ignored data for duplicate keys and resource limits; stable first-seen text categorization for projected columns; `Option`/`Result`; bounded `parallel_map`; text/byte file operations with atomic replacement; default-deny bounded HTTP GET and general text requests with exact-host grants and request-specific deadlines bounded by host policy; exact-grant environment reads; typed Python calls through a supervised sidecar; managed CPython and locked dependencies via pinned uv; separate stdout/stderr program output with structured effects; source-mapped diagnostics, comment-preserving formatting, inspection and structured CLI reports; current bytecode format 33, including Unicode-scalar `text_chars` and copy-on-write local list and JSON-array append; local package creation/verification and a current-host installer. Cranelift JIT supports selected numeric, list, record-call, and stdout-print paths; checked integer overflow and unsupported operations fall back to the VM. The format-33 campaign candidate is packaged for Debian 12-compatible GNU/Linux ARM64. A macOS ARM64 0.1.0 package exists at the earlier format-30 level and must be refreshed before claiming format-33 packaging parity across both targets.

An experimental self-hosted compiler frontend is included for a supported subset: a Rust-hosted compiler builds the Veritas compiler, then the Veritas-built compiler recompiles its own source to matching compiler IR and compiles selected real programs. The bytecode assembler and VM remain Rust components; this is a bootstrap proof, not a claim that the entire compiler/runtime is written in Veritas.

Remaining gaps: maps and other broader collection shapes in native code, streaming/binary HTTP, cancellation during a blocking network call, syntax-aware formatting, zero-copy Python buffers, additional targets, broader ecosystem coverage, wider VM/native allocation parity, and an exact RSS cap. Selected compiled wheels support same-target relocation and offline launch on macOS ARM64 and Debian 12-compatible Linux ARM64; arbitrary wheels and hosts remain outside the documented support boundary. Network host grants do not prevent DNS rebinding or provide operating-system egress confinement. This bundle describes product capabilities and limits; it contains no candidate or comparative outcome data.

See [architecture.md](architecture.md), [compatibility-and-migrations.md](compatibility-and-migrations.md), [host-api.md](host-api.md), [language-spec.md](language-spec.md), [python-abi.md](python-abi.md), [reference.md](reference.md), and [setup-guide.md](setup-guide.md) in this bundle.

## Build a current-host bundle

On macOS ARM64, provide the archive named by `distribution/toolchains/manifest.toml` as `VERITAS_UV_ARCHIVE` or a standalone uv executable matching its binary checksum as `VERITAS_UV_BINARY`. On Debian-compatible GNU/Linux ARM64, use `distribution/toolchains/manifest-linux-aarch64.toml` and its matching pinned binary. Then run `sh distribution/package-current-host.sh /path/to/output` on the target host. The script verifies uv before packaging. After extraction, run `sh install.sh` to install under `~/.local`, or provide a prefix. The installer keeps release files together, creates a `bin/veritas` command, preserves previous versions, and does not edit shell startup files. Archives are unsigned and require their named OS, architecture and runtime ABI. Managed CPython and wheel dependencies are acquired by `veritas sync` on the target host. See `PACKAGE.txt` and `doctor.txt` inside the archive for included capabilities and limits.

For a Python-assisted application, run `veritas sync` during development, then `veritas package` to create an app bundle containing managed CPython, the locked bridge environment, and offline cache for the current target. Same-target relocation and offline launch are supported for selected wheels on macOS ARM64 and Debian 12-compatible Linux ARM64; arbitrary native wheels and other targets remain outside the documented support boundary.

## architecture.md
# Architecture

Veritas uses a Rust Cargo workspace with four crates. The CLI parses TOML manifests; the Python bridge uses `serde_json` for its framed protocol.

- `veritas-model` owns source locations, diagnostics, mathematical integers, portable values including JSON trees and records, bytecode instructions, artifact encoding, and runtime errors.
- `veritas-compiler` tokenizes and parses the complete entry module graph, checks type and export visibility, records per-module source IDs, and lowers functions into portable stack bytecode.
- `veritas-runtime` validates artifact versions and control-flow stack heights, then executes with per-instance output, instruction, call-depth, integer, text, collection, file-size, and cumulative estimated-allocation limits. Format-16 artifacts carry schema-versioned source/dependency identities and build-time numeric ceilings; runtime uses the lower of each embedded ceiling and the host's `HostPolicy`. Allocation estimates cover guest data such as host inputs, collections, records, text/bytes/JSON, file/Python results, output rendering, and parallel work. They exclude interpreter/JIT frames, operand stacks, and scalar arithmetic temporaries so backend choice cannot alter the allocation-limit outcome; integer growth is bounded separately by the digit limit. File operations require an explicit root/read/write policy; canonical path checks block parent traversal and symlink escapes, though they do not create an OS sandbox.
- `veritas-cli` exposes the same compiler/runtime path to a user.

The direction is `model <- compiler`, `model <- runtime`, and `compiler + runtime + model <- CLI`. Source compilation produces an `Artifact`; both the CLI and embedding API use `Runtime.load`, `Instance.invoke`, and `Instance.close`. The format is versioned and rejects malformed encodings; bytecode internals may change with a format version bump. Artifacts carry external Python declarations and calls. The CLI selects an exact interpreter from a managed project environment when its Python lock is present; embedding APIs and projects without managed Python still require an explicit interpreter path. The isolated Python sidecar explicitly adds the project root to its import path so typed adapters can live with project source while dependencies remain in the managed environment.

The optional Cranelift backend compiles eligible pure bytecode functions to host-native code. Its current typed surface includes scalar `Int` and `Float64`, `List[Float64]` length/index reads, reads of supported numeric/list fields from records, same-local `list_set` updates, and record construction/return. Native list descriptors point into invocation-owned copies; this keeps caller values immutable and lets an updated record return its values without exposing Rust collection layouts to generated code. The verifier and type inference decide eligibility; unsupported operations stay on the VM. Checked list bounds and unsupported numeric cases deopt and replay through the VM so diagnostics and source traces remain authoritative. Native-to-native calls do not cross list-mutating functions, avoiding hidden alias changes. This is a growing execution subset, not a claim that every program is native.

The runtime supports bounded JSON parsing and serialization with exact integer numbers, sorted object keys, duplicate-key rejection, typed accessors and depth/byte ceilings. Parse, missing-key, and type errors are currently invocation errors.

The runtime's bounded `parallel_map` executes each task in an isolated VM instance on worker threads and returns `Result[List[U], TaskError]`. Each worker receives an initial item before workers share the remaining queue; this keeps short JIT-eligible tasks from all being claimed by whichever thread starts first. Workers preserve input order, use bounded worker allowances from the invocation's instruction and allocation budgets, stop scheduling and poll cancellation after failure, join running tasks, and merge completed output in input order. `TaskError` retains the failed input index, runtime code, message and trace. Native call failures also retain their nested source call sites. Host setup and policy failures remain invocation errors. Immutable JSON and list values share reference-counted storage. A `set values = list_set(values, index, item)` assignment lowers to a copy-on-write local update: it reuses uniquely owned storage and clones when another value still aliases it, preserving observable immutability while avoiding repeated full-list copies in loops. Allocation charges are estimates at known guest-data creation points, not exact native memory measurements. `Runtime::vm_only()` provides a VM execution mode for deterministic backend diagnosis and comparison.

Embedders can pass a cloneable `CancellationToken` to `Instance::invoke_with_cancellation` and call `cancel()` from another thread. VM execution polls between instructions; JIT code polls at function entry and on backward branches; parallel workers share the invocation signal, stop taking work and are joined before cancellation returns. A running Python call polls at 20 ms intervals; cancellation kills and reaps the managed sidecar process. Synchronous HTTP cannot be interrupted while the blocking request is in progress, so cancellation is observed when that call returns or reaches its configured timeout. Completed file, network, Python, and output effects are not rolled back. The token is one-shot and should be newly created for each invocation. CLI Ctrl-C still terminates the process directly.

`Instance::effect_report()` retains ordered host-effect records for the most recent invocation, including captured output, file access, HTTP, environment reads, and Python calls. Records distinguish an operation that returned, returned an error, or was interrupted; they do not claim that a remote peer did or did not apply a request. The report is capped at 1,024 records, each target is capped at 256 Unicode scalar values, and `omitted_effects()` reports how many later records could not be retained. The CLI's `run --json` includes the same report and omission count.

The VM is a baseline for portability and inspection. The runtime includes a Cranelift JIT for selected numeric, list, record-call, and stdout-print paths; unsupported operations fall back to the VM. CPU-bound guest tasks can run on multiple OS threads through the bounded parallel mapping API. This architectural capability does not guarantee speedup for arbitrary programs. Managed dependency resolution uses pinned uv and CPython. Bounded HTTP, concurrent I/O, and typed Python calls are available through documented host APIs. Exact allocator/RSS accounting, broader native operation coverage, relocatable Python-assisted bundles, and a syntax-aware formatter remain open work.

## compatibility-and-migrations.md
# Compatibility and migration policy

This is the compatibility policy for the Veritas 0.1.0 evaluation build. Veritas is pre-1.0: source syntax, public APIs, bytecode details and package metadata may change before 1.0. This build does not promise long-term upgrade compatibility.

## Supported combinations

| Boundary | Supported in this candidate | What happens outside it |
| --- | --- | --- |
| Source | Compile with the matching Veritas 0.1.0 CLI and its bundled documentation. | A newer compiler may require source edits; no source-level compatibility range is promised before 1.0. |
| Bytecode artifact | The reader accepts artifact formats 14 through 33; the current compiler writes format 33. | A future format is rejected by older readers. Rebuild from source with a matching compiler when possible. |
| Package metadata | The verifier accepts package schemas 1 and 2; new packages use schema 2. | Unknown schema versions fail closed. |
| Application bundle | The format-33 campaign candidate is Debian 12-compatible GNU/Linux ARM64 with glibc. Same-target relocation is supported. | The repository has an earlier format-30 macOS ARM64 package; refresh and revalidate it before claiming format-33 parity. Other Linux distributions, libc implementations, operating systems and CPU architectures are unverified. |
| Python bridge | The exact Python patch version declared in `veritas.toml`; the release pins uv 0.13.4. | A different Python patch version or host target does not satisfy the lock. Use the explicit update workflow after reviewing dependency changes. |

Artifact format compatibility does not imply that the original compiler version is checked at runtime. Build metadata records the compiler version and other provenance for inspection. Formats 14 and 15 have less metadata than current artifacts; format 14 has no embedded runtime policy. The loader validates each feature against the minimum format that introduced it.

Package hashes and lock fingerprints detect accidental changes. Release archives and package metadata are unsigned, so they do not authenticate their publisher. Python adapter declarations are typed calls, not general Python source compatibility.

## Updating a Python environment

`veritas sync PROJECT` checks the existing lock and installs that exact dependency graph. `veritas sync --update PROJECT` stages a new graph and managed environment, validates the interpreter, then activates the new environment and writes matching lock metadata. If resolution, installation or interpreter validation fails, the previous bridge environment and lock files remain available. Review `veritas.lock` and `.veritas/python-bridge/uv.lock` after a successful update before rebuilding an application package.

## Moving to another Veritas candidate

1. Keep the project source, manifest, both Python lock files, and application data together. Make a backup before changing candidate versions.
2. Use the new candidate's `doctor` and `check` commands, then run the project's test functions.
3. If the existing locks pass, run without `--update`; change dependency resolution only with the explicit update command and review both lock files.
4. Rebuild artifacts and app packages with the new CLI. Verify the package and test it after moving it to a fresh path on the same supported OS and architecture.
5. Keep the previous candidate and data backup until the new build passes the project's acceptance checks.

Veritas does not provide source rewrites or artifact-format upgrades beyond reading the supported range. There is no automatic application-data migration in this release; application data schemas and their upgrades remain the application's responsibility. If a future runtime rejects an artifact format, retain its source and rebuild with a compatible compiler; if the source no longer compiles, follow migration notes for that specific release rather than editing the artifact bytes.

## host-api.md
# Veritas host API contract

Status: executable contract for the current build. `print`, `args`, `args_from`, exact-grant environment reads, text and byte file operations, default-deny bounded HTTP GET and general text requests, `assert`, `len`, JSON operations, and the initial bounded `parallel_map` are implemented.

## Capabilities and policy

The runtime receives a `HostPolicy` when an artifact is loaded. It grants individual capabilities and resource ceilings. Guest source cannot expand those grants. The embedding API and CLI use the same host service implementations. The CLI reads optional ceilings from `[runtime]` in `veritas.toml`; embedders can set the same fields in `HostPolicy`. The CLI's default file root is the project root; path checks are defense in depth and do not isolate a process from the operating system.

The runtime has a default cumulative allocation budget of 268435456 estimated bytes per invocation. The estimate is charged for input values, collection and record construction, text/byte/JSON values, file and Python return values, output rendering, and parallel work. Interpreter/JIT frames, operand stacks, and scalar arithmetic temporaries are excluded so backend selection does not change this limit. Integer growth is bounded separately by the digit ceiling. Nested functions share the invocation budget. Parallel workers share the same counter while preserving a quarter of the remaining allowance for the calling VM to receive and handle the group's result. Exhaustion reports `R_ALLOCATION_LIMIT`. This estimates language-level allocations; it is not exact allocator accounting, an RSS ceiling, or an operating-system sandbox. Output, source/artifact size, file size, collection count, integer digits, text size, call depth and instruction work keep their separate limits.

The CLI accepts positive integer ceilings under `[runtime]`:

```toml
[runtime]
instruction_limit = 2000000
allocation_limit_bytes = 268435456
call_depth_limit = 128
output_limit_bytes = 1000000
max_integer_digits = 4096
max_text_bytes = 16777216
max_collection_items = 1000000
max_parallelism = 8
max_file_bytes = 16777216
```

Unknown `[runtime]` fields are errors so spelling mistakes do not silently select defaults. These limits apply to `veritas run` and discovered tests; compile-time source and artifact limits are enforced separately.

Environment reads are denied by default. A project opts in to individual variable names with `[environment].variables`; values remain process-provided at launch and are never copied into the project manifest or artifact.

```toml
[environment]
variables = ["API_TOKEN"]
```

Each operation has an explicit failure result. Host failures carry a stable code, message, and source trace. Partial writes or other completed external effects are reported as completed effects; errors do not imply rollback. A host call that blocks must have a deadline and a cancellation policy or be rejected by the current target.

## Public operations

| Operation | Proposed signature | Capability and rules | Status |
|---|---|---|---|
| output | `print(value: Any) -> Unit` | UTF-8 text, bounded per invocation | Implemented |
| arguments | `args() -> List[Text]` | Read only process arguments after the source path | Implemented |
| argument suffix | `args_from(start: Int) -> List[Text]` | Zero-based suffix of process arguments; negative or past-end index returns `R_ARGUMENT_INDEX` | Implemented in artifact format 22 |
| environment | `env(name: Text) -> Option[Text]` | Exact variable-name grant in `[environment].variables`; unset values return `None`; ungranted names return `R_ENV_DENIED` | Implemented in artifact format 24 |
| read text | `read_text(path: Text) -> Result[Text, FileError]` | Read grant; relative path under root; bounded bytes; UTF-8 required | Implemented; policy size-limit failures remain runtime errors |
| write text | `write_text(path: Text, content: Text) -> Result[Unit, FileError]` | Write grant; relative path under root; bounded bytes | Implemented; uses direct filesystem write, so failure can leave a partial file |
| atomic text replace | `write_text_atomic(path: Text, content: Text) -> Result[Unit, FileError]` | Write grant; writes and syncs a temporary sibling, then renames it over the target; temporary file is removed on reported errors | Implemented in artifact format 21; atomic visibility does not serialize writers or guarantee directory-entry durability |
| read bytes | `read_bytes(path: Text) -> Result[Bytes, FileError]` | Same root and file-size policy as text; preserves all bytes, including invalid UTF-8 and NUL | Implemented |
| write bytes | `write_bytes(path: Text, content: Bytes) -> Result[Unit, FileError]` | Same root and file-size policy as text | Implemented; direct filesystem write can leave a partial file on OS failure |
| JSON decode | `json_parse(text: Text) -> Result[Json, JsonError]` | Depth and input byte limits; malformed syntax and duplicate object keys return `JsonError` with stable code, message, line, and byte column; resource-limit failures remain runtime errors | Implemented |
| JSON column extraction | `json_int_column(rows, field) -> List[Int]`, `json_text_categories(rows, field) -> Json`, `json_as_int_list(values) -> List[Int]` | Typed bulk extraction from row arrays; exact integers only; category codes follow first-seen label order; traced type/field errors and normal collection/allocation ceilings | Implemented in artifact format 29 |
| JSON projection | `json_project_columns(source, rows_path, field_names) -> Result[Json, JsonError]`, `json_text_categories_values(values) -> Json` | Streaming projection retains requested fields as columns; validates ignored JSON for syntax, duplicate keys, and limits; category codes follow first-seen label order | Implemented in artifact format 30 |
| Unicode text scan | `text_chars(text: Text) -> List[Text]` | Bounded list of Unicode scalar values for linear scans; length and indexing count Unicode scalar values | Implemented in artifact format 31 |
| Local append fast paths | `list_push(list, value) -> List[T]`, `json_push(array, value) -> Json` | Preserve immutable results; assigning back to the same local uses copy-on-write storage and clones only when an alias needs the old value | Implemented in artifact formats 32 and 33 respectively |
| JSON encode | `json_stringify(value: Json) -> Text` | Stable object key order; bounded output; encode failures are stable runtime errors | Implemented |
| JSON values | `json_null`, `json_bool`, `json_int`, `json_float`, `json_text`, `json_array`, `json_object` | Typed constructors; same JSON and allocation ceilings; artifact format 20 | Implemented |
| JSON updates | `json_push(array: Json, value: Json) -> Json`, `json_set(object: Json, key: Text, value: Json) -> Json` | Immutable updated copy; rejects the wrong container kind and enforces collection/text ceilings; artifact format 20 | Implemented |
| HTTP text GET | `http_get(url: Text) -> Result[Text, HttpError]` | Exact-host grant required; HTTP/HTTPS only; proxy auto-discovery disabled; time, redirect and streamed body limits; non-2xx status is `Err` | Implemented |
| General HTTP request | `http_request(method: Text, url: Text, headers: Map[Text], body: Text) -> Result[Json, HttpError]` | Exact-host grant; GET/POST/PUT/PATCH/DELETE/HEAD/OPTIONS; bounded UTF-8 request/response bodies; same-host redirects; non-2xx status returned in JSON envelope | Implemented in artifact format 23; binary bodies, streaming, and mid-request cancellation remain planned |
| clock | `unix_time_ms() -> Int` | Explicit clock grant; wall clock is nondeterministic | Planned |
| structured work | `parallel_map(items: List[T], worker: fn(T)->U, limit: Int) -> Result[List[U], TaskError]` | Bounded workers, isolated VM state, ordered outputs, joined scope | Implemented in named-function form |

`HttpError` is an implemented structured error with stable code/message and a source span; inspect it with `http_error_code` and `http_error_message`. General HTTP responses use a `Json` envelope containing `status`, `headers`, and `body`; the application parses a JSON body explicitly when needed. `Bytes` is an immutable byte-preserving value introduced in artifact format 17. `len` accepts Bytes and returns the byte count. Foreign calls are recorded as opaque effects even when declarations include types.

`Json`, `JsonError`, and `FileError` are implemented value types. `json_parse`, `read_text`, `write_text`, and `write_text_atomic` return typed Results for syntax/duplicate-key and filesystem access failures; `is_ok`, `unwrap`, and generic `error` inspect them. Missing files return `R_FILE_NOT_FOUND`; permission failures return `R_FILE_PERMISSION`; other read/write failures keep their operation code. `json_error_code/message/line/column` and `file_error_code/message/line/column` expose details. File error locations point to the source host call and survive `?` propagation. `json_get`, `json_index`, `json_has`, `json_len`, `json_as_text`, `json_as_int`, `json_as_bool`, and `json_is_null` access parsed values. Artifact format 20 adds typed JSON constructors and immutable `json_push`/`json_set` updates so programs can create and persist JSON state without a Python bridge. Format 21 adds atomic text replacement. Format 22 adds `args_from(start)` for subcommand-based CLI programs. It prevents readers from observing a partially written replacement but does not coordinate concurrent read-modify-write sequences; simultaneous writers remain last-complete-rename wins. A process crash can leave the sibling temporary file, and directory-entry crash durability is not promised. Missing-key, wrong-container, conversion, and resource-limit errors still fail the invocation.

## File behavior

Paths are interpreted relative to the policy root. Absolute paths, parent components, non-UTF-8 names, and canonicalized symlink escapes are rejected. Read and write permissions are independent. Directories, metadata, deletion, and arbitrary file descriptors are not exposed in the first surface. The default CLI grants only the project root. This protects against accidental path traversal; it is not a sandbox against malicious source or native code.

## HTTP behavior

Network access is denied unless the exact hostname is granted. CLI projects configure this in `veritas.toml`:

```toml
[network]
hosts = ["feeds.example.org"]
timeout_ms = 15000
max_response_bytes = 16777216
max_request_bytes = 1048576
max_redirects = 5
```

The default is no hosts, a 15-second total request timeout, a 1 MiB request limit, a 16 MiB response limit, and five redirects. Only HTTP/HTTPS URLs without embedded credentials are accepted. `http_get` follows redirects only to granted hosts and returns non-2xx statuses as typed errors. `http_request` restricts redirects to the initial host, caps request and response bodies while streaming, and returns non-2xx status, headers, and UTF-8 response text in its JSON envelope. Proxies are disabled so ambient proxy settings cannot route a request outside the declared host policy. The blocking HTTP client has a request deadline but cannot be interrupted mid-read by `parallel_map` cancellation; the worker group waits for that deadline. DNS rebinding and network egress confinement require operating-system controls outside the language runtime. Binary bodies and streaming APIs remain planned.

## Structured concurrency behavior

`parallel_map` runs a named unary function in one worker-local VM per task. It shares immutable input values by copy. Results are returned in input order. The group does not return until workers stop. On failure it stops assigning pending items, sets a cancellation flag that the VM checks between instructions, joins workers, preserves completed output in input-index order, and returns a `TaskError` with the selected input index, original stable runtime code/message, and source trace. Work already completed and host side effects are not rolled back. Nested parallel groups consume the invoking group's budget and worker ceiling.

The current API rejects a zero or over-policy worker count. Worker count reports OS worker IDs observed, not guaranteed CPU speedup. A future async I/O API must have separate documented scheduling and cancellation semantics; this operation is not presented as a general async runtime.

## language-spec.md
# Veritas language contract, current executable subset

This is the current implementation contract for the approved north-star design. It defines the subset implemented in the current candidate; unsupported syntax must produce a source diagnostic instead of being ignored.

## Source and declarations

- Source is UTF-8. Newlines separate statements. Four spaces define one indentation level; tabs and mixed indentation are errors. `#` begins a line comment.
- A source file is a module. It contains one or more `fn name(parameter: Type, ...) -> Type:` declarations and may declare records with `record Name:` followed by indented `field: Type` declarations. `import "relative-path.vr"` loads another source file; the imported module's `pub fn` and `pub record` declarations become available by their declared names. Private declarations remain local to their module. Imports are resolved from the importing file and must stay inside the project root. The graph is limited to 512 files, 128 import levels, 8 MiB per source file, and 64 MiB total source text. Duplicate declaration names and cycles are errors. Types in this executable subset are `Int`, `Float64`, `Bool`, `Text`, `Bytes`, `Unit`, `Json`, `JsonError`, `FileError`, `HttpError`, `TaskError`, `List[T]`, `Map[Text, T]`, `Option[T]`, `Result[T, E]`, and declared record names.
- The entry point is `fn main() -> Int:` or `fn main() -> Unit:` in the project entry module. Calls resolve statically to local declarations, explicitly imported public declarations, or host functions such as `print(value)`. A single source file can be compiled without a project only when it has no imports.
- Function parameters and results are annotated. Local types are inferred unless an optional local annotation is supplied, as in `let pending: List[Int] = []`. `let` creates an immutable local; `var` creates a mutable local; `set name = expression` changes only a mutable local. An annotation supplies context for empty collection literals and must exactly match the initializer type.

## Statements and expressions

Statements: `let`, `var`, `set`, `return`, `if`/`else`, `while`, `for name in list`, exhaustive `match` statements for `Option` and `Result`, and a call expression as a statement. `if` and `while` conditions must be `Bool`. A function body is indented under its declaration. `else` aligns with `if`.

- Expressions: arbitrary-precision decimal integer literals, finite `Float64` literals with a decimal point or exponent, booleans (`true`, `false`), double-quoted UTF-8 text, local/function names, calls, record construction/access, list literals, map literals with unique text keys, list/map indexing, parentheses, unary `-` and `not`, and binary `*`, `/`, `//`, `%`, `+`, `-`, comparisons `== != < <= > >=`, and short-circuit `and` / `or`. `Float64` is IEEE 754 binary64; its arithmetic and comparisons require two `Float64` values, reject non-finite literals/results, and never convert `Int` implicitly. `sqrt(Float64)` returns the non-negative square root; a negative input raises `R_MATH_DOMAIN`. Float division by zero raises `R_DIV_ZERO`. Integer `/` truncates toward zero; `//` rounds down; `%` follows the divisor's sign, so `a == (a // b) * b + (a % b)`. All three integer division operators raise `R_DIV_ZERO` on a zero divisor. Map literals infer one homogeneous value type; an empty map needs a `Map[Text, T]` annotation or other expected type, and keys must be text literals. Empty list literals likewise need an expected `List[T]` type. `list_push(List[T], T)` appends and `list_set(List[T], Int, T)` replaces an existing zero-based index; both return immutable copies, and out-of-range replacement raises `R_LIST_INDEX`. `Map[Text, T]` is immutable, `map_set` returns an updated copy, `map_has` tests membership, keys iterate and display in Unicode scalar lexicographic order, lookup of an absent key raises `R_MAP_KEY_MISSING`, and `len` supports lists, maps, and Bytes (byte count). Types include `List[T]`, `Map[Text, T]`, `Option[T]`, and `Result[T, E]`; `Some(value)` and `None` are Option constructors in Option-returning functions. Precedence from low to high: `or`, `and`, comparisons, `+ -`, `* / // %`, unary, calls/indexing/field access/atoms. `+` accepts two Ints, two Float64 values, or two Text values.

Text length and indexing count Unicode scalar values, not bytes or grapheme clusters. `text_chars(text)` materializes those values as a bounded `List[Text]` for linear scans and requires artifact format 31.

## Runtime rules

- `Int` is arbitrary precision subject to the configured digit ceiling. Integer `/` truncates toward zero, `//` rounds down, and `%` follows the divisor sign so `a == (a // b) * b + (a % b)`; all report `R_DIV_ZERO` for a zero divisor. `Float64` stores finite IEEE 754 binary64 values. Operations that overflow to infinity or NaN fail with `R_FLOAT_NONFINITE`; division by either signed zero fails with `R_DIV_ZERO`; `sqrt` of a negative value fails with `R_MATH_DOMAIN`. Python interop transports Float64 through round-trippable decimal strings and rejects non-finite values. Lists are homogeneous, immutable value sequences; indexing is zero-based and out-of-range access is a runtime error. `list_push` and `list_set` return immutable lists, and `list_set` requires an existing index. When `set name = list_set(name, index, value)` updates a mutable local, the compiler may reuse uniquely owned storage and copies if any alias exists; this is unobservable and preserves immutable value behavior. Maps are immutable `Map[Text, T]` values with unique literal keys, homogeneous values, deterministic key iteration, immutable `map_set` updates, membership checks via `map_has`, and traced errors for missing keys. Map values may be passed between functions, serialized in versioned artifacts, and crossed through the Python bridge as string-keyed dictionaries when `T` is bridge-supported. Empty lists and maps use their expected type from annotations, function results or parameters, enclosing collections, and typed operations; uncontextualized empty literals produce a diagnostic. `Json` is a bounded JSON tree; object keys are text, duplicate keys are rejected, and object encoding sorts keys. JSON values require explicit accessors and do not silently coerce to source types. Records are immutable ordered named fields, constructed positionally (for example, `Point(3, 4)`) and accessed by field name (`point.x`). Construction requires every field with its exact declared type; record and field names are unique; equality is structural for the same record type. Unknown fields/types and malformed schemas are rejected. Records serialize in compiled artifacts, but are not supported through the Python bridge yet. First-class functions remain for subsequent implementation steps. `Option[T]` supports `Some(value)` and `None` inside functions declared to return `Option[T]`; `is_some(option)` tests presence and `unwrap(option)` returns the value or raises `R_OPTION_UNWRAP`. `Result[T, E]` is available for file operations, JSON parsing, and `parallel_map`; inspect results with `is_ok`, `unwrap`, and `error`. `Ok`, `Err`, postfix `?`, and exhaustive `match` statements are available for `Result[T, E]`.
- Integers compare by mathematical value. Text is Unicode scalar sequence data; `+` concatenates text. Equality requires equal runtime types.
- Locals are frame-owned values. `let` cannot be assigned; `var` can. Each invocation owns its locals. Calls have a bounded stack depth, instructions consume a configurable budget, and output is captured per instance. A shared cumulative estimated guest-data allocation budget covers input values, collection and record construction, text/byte/JSON values, host results, and output rendering across nested calls and parallel workers; exhaustion reports `R_ALLOCATION_LIMIT`. The default is 256 MiB. Interpreter/JIT frames, operand stacks, and scalar arithmetic temporaries are excluded so changing execution backend does not change this limit. Integer growth is bounded separately by `max_integer_digits`. This is language-level estimate accounting, not an exact process-memory ceiling; limits for output, file bytes, text size, collection size, integer digits, and source/artifact size are separate.
- `print(value)` writes its display form plus one newline to the instance output buffer. `args()` returns all CLI arguments as `List[Text]`; `args_from(start)` returns the suffix beginning at a zero-based index and reports `R_ARGUMENT_INDEX` for a negative or past-end index. `env(name) -> Option[Text]` reads one environment variable only when its exact name is granted in `[environment].variables` in `veritas.toml` (or by an embedding `HostPolicy`); an ungranted name raises `R_ENV_DENIED`, and an unset granted name returns `None`. `read_text(path) -> Result[Text, FileError]`, `write_text(path, content) -> Result[Unit, FileError]`, `read_bytes(path) -> Result[Bytes, FileError]`, and `write_bytes(path, content) -> Result[Unit, FileError]` are host operations; runtime embedders grant a project root and read/write rights in `HostPolicy`. The CLI grants the source or artifact directory. Relative paths only are accepted, parent traversal is rejected, and canonicalized symlink targets must remain under the root. Access/path/filesystem failures return `FileError`; configured size-limit failures remain runtime errors. This is a path policy, not an OS security sandbox. Bytes preserve arbitrary byte sequences and are limited by `max_file_bytes`.
- `http_get(url) -> Result[Text, HttpError]` performs a bounded GET for HTTP or HTTPS text. Network is denied by default. Embedders explicitly grant exact hostnames through `HostPolicy`; CLI projects list them under `[network].hosts` in `veritas.toml`. A redirect is followed only when its destination hostname is also granted. Timeouts, response byte ceilings, and redirect counts are configurable; non-2xx statuses and transport/encoding failures return `HttpError`, inspectable through `http_error_code` and `http_error_message`. Host allowlists do not prevent DNS rebinding or provide OS-level egress confinement.
- `http_request(method, url, headers, body) -> Result[Json, HttpError]` supports GET, POST, PUT, PATCH, DELETE, HEAD, and OPTIONS with `Map[Text]` headers and a UTF-8 text body. The JSON result envelope contains integer `status`, text `body`, and a `headers` object with lowercase names. Non-2xx responses are values so applications can implement retries and partial-failure rules. Request and response bodies, timeout, and redirects are bounded by host policy. Redirects stay on the initial granted hostname to prevent forwarding caller-supplied credentials across hosts. Binary bodies, streaming, and interruption of a blocking request remain unsupported.
- `parallel_map(items, worker, limit)` calls a named unary function on immutable list items in worker-local VM instances and returns `Result[List[U], TaskError]`. `is_ok(result)` tests the variant; `unwrap(result)` returns the list from `Ok`; `error(result)` returns the `TaskError` from `Err`. `task_index`, `task_code`, and `task_message` read its failed input index and failure details. Workers preserve result order, use a bounded shared worker instruction allowance, stop scheduling and poll cancellation after failure, join running tasks, and merge completed output by input index. An instruction-limit failure inside a worker is a `TaskError`; host setup errors still fail the invocation.
- Runtime and compile errors preserve source spans. Format-17 artifacts add Bytes values and byte file operations; format-18 artifacts add source-level Python keyword calls; format-19 artifacts add HTTP GET and `HttpError`; format-16 metadata carries source and dependency-lock provenance; format-15 metadata and format-14 artifacts remain readable. Runtime combines recorded ceilings with host policy by taking the stricter value. The bytecode is an implementation detail apart from the versioned artifact boundary.

## Contracts for planned surfaces

These rules are design commitments for the approved build; the source compiler must continue to reject unsupported constructs. A surface becomes available only when it has parser/type/runtime and acceptance coverage.

- Arbitrary-precision integer arithmetic remains exact subject to a configured digit ceiling. Division by zero is currently a traced runtime error; a future checked division API may expose this as a typed `Result`.
- Text is UTF-8. Length and indexing count Unicode scalar values, not bytes or grapheme clusters. Indexing is zero based and out of range is an error. Byte-oriented operations use `Bytes` explicitly. Normalization is not implicit; equality compares exact scalar sequences.
- Lists and maps are immutable values. `list_push`, `list_set`, and `map_set` return updated copies; `list_set` addresses an existing zero-based index. `Map[Text, T]` is the initial map form; iteration is in Unicode scalar lexicographic key order. Updating a collection returns a new value. Records have declared named fields, exact field types, and structural equality; unknown fields are errors at construction/deserialization. Mutable sharing across workers is not supported.
- `Option[T]` has `None` and `Some(value)`; `Result[T, E]` has `Ok(value)` and `Err(error)`. Absence and failure are distinct. `Some` and `None` construct options inside a function declared to return `Option[T]`; inspect them with `is_some` and `unwrap`, which reports absence as a runtime error. `Ok(value)` and `Err(error)` construct results inside a function declared to return `Result[T, E]`. Postfix `?` unwraps an `Ok` or returns its `Err` from the current function; the error type must match the function's declared `Result` error type. A function still writes `Ok(value)` around a successful return expression. A `match` statement must cover exactly one `Some` and one `None` arm for an `Option`, or one `Ok` and one `Err` arm for a `Result`. The bound name is visible only in that arm; arm order is unrestricted. Wildcards, nested patterns, other type matches, and match expressions are not implemented. No implicit exception-to-Result conversion occurs. Current host results cover file operations, JSON parsing, and `parallel_map`; inspect them with `is_ok`, `unwrap`, and `error`.
- The executable import form is `import "relative-path.vr"`. Every imported module contributes only `pub fn` and `pub record` declarations to its direct importer; private declarations can still call their own module's private functions and direct imports. Names are imported unqualified, so collisions across the resolved graph are rejected. Absolute paths, `.`/`..` components, missing files, root escapes and cycles fail before execution. CLI project builds resolve one module graph from the entry file and store per-file source identifiers in artifacts; diagnostics and runtime traces point to the originating module. External declarations specify a Python module/callable and typed signature; calls are dynamically validated and remain opaque effects. Calls may use positional arguments followed by named arguments matching the declaration's parameter names; positional calls to Veritas functions remain positional. Omitted external parameters are passed through as omitted for Python defaults. Details of the Python wire boundary are in [python-abi.md](python-abi.md).
- Public functions and externally visible records require annotations. Locals infer from their initializer and cannot silently change type. Generics in the first implementation are limited to `List`, `Map`, `Option`, and `Result`; user-defined generic functions are not implied.

`json_parse(text) -> Result[Json, JsonError]`, `json_stringify(json) -> Text`, `json_get(json, key) -> Json`, `json_index(json, index) -> Json`, `json_has(json, key) -> Bool`, `json_len(json) -> Int`, `json_as_text/int/float/bool(json)`, and `json_is_null(json)` are available. JSON construction in artifact format 20 adds `json_null()`, `json_bool(Bool)`, `json_int(Int)`, `json_float(Float64)`, `json_text(Text)`, `json_array()`, `json_object()`, `json_push(Json, Json)`, and `json_set(Json, Text, Json)`. These APIs build immutable JSON values; push and set return updated copies and reject a value of the wrong container kind. Constructors enforce the same nesting, collection, text and integer limits as parsed JSON. `json_as_float` explicitly converts a JSON number to finite `Float64`. Malformed JSON and duplicate object keys return `Err(JsonError)` with a stable code, message, line, and byte column, inspectable through `json_error_code/message/line/column`. Use `unwrap(json_parse(...))` when invalid input should stop the program. Resource-limit failures and invalid typed access still raise runtime errors.

Artifact format 29 adds `json_int_column(rows: Json, field: Text) -> List[Int]`, `json_text_categories(rows: Json, field: Text) -> Json`, and `json_as_int_list(values: Json) -> List[Int]`. Format 30 adds `json_project_columns(source: Text, rows_path: List[Text], field_names: List[Text]) -> Result[Json, JsonError]`, which streams through a JSON document and retains only the requested row fields as column arrays, plus `json_text_categories_values(values: Json) -> Json` for stable first-seen categorization of a projected text column. Format 31 adds bounded `text_chars(text: Text) -> List[Text]` Unicode-scalar materialization. Format 32 adds copy-on-write local list append; format 33 adds copy-on-write local JSON-array append. The projection still validates JSON and duplicate keys throughout ignored data, checks collection, integer and nesting limits, requires an array of row objects at the selected path, and reports missing or duplicate requested fields as `JsonError`. Projected values remain JSON values; use `json_as_int_list` or normal JSON accessors to convert them. Bulk conversion and projection remove per-row source-level JSON access work, but do not imply that Veritas is faster than Python for every workload.

The CLI uses the value returned from `main`: `Int` becomes the process exit status and must be between 0 and 255; `Unit` exits successfully with status 0. Runtime failures and invalid exit values produce a nonzero status. `write_text_atomic(path, content) -> Result[Unit, FileError]` is available in artifact format 21. It writes a temporary sibling file and renames it over the target, preventing readers from seeing a partial replacement. Missing reads return `R_FILE_NOT_FOUND`; permission failures return `R_FILE_PERMISSION`. Atomic replacement does not lock read-modify-write sequences, and it does not promise that a completed rename survives sudden power loss.

Host capability signatures and operational limits are recorded in [host-api.md](host-api.md). Unimplemented signatures and types are proposals, not working APIs.

## Initial examples

See `examples/sum.vr`. This contract intentionally prefers a small executable core that is genuinely compiled and interpreted over unimplemented promises in the parser.

## python-abi.md
# Veritas Python bridge ABI

Status: typed declarations, positional and keyword source calls, omission of Python-defaulted parameters, the framed sidecar, `Unit`, `Bool`, `Int`, finite `Float64`, `Text`, `Bytes`, `Json`, nested `List` and string-keyed `Map` conversions, top-level NumPy array materialization for declared list returns, timeout supervision, explicit-interpreter calls, and a managed project dependency workflow are implemented. The current-host installer distribution pins uv 0.13.4; project sync selects an exact CPython patch version, creates a managed environment, preserves uv's native lock, and writes a Veritas metadata lock. Python-assisted app bundles copy the managed CPython runtime and locked environment for same-target, offline launch. `Option` and `Result` conversions remain unimplemented.

## Declaration shape

The current source form is:

```veritas
extern python "math:factorial" fn factorial(value: Int) -> Int
```

The declaration names a module importable from the managed Python environment or the project root, and a callable plus a Veritas signature. The bridge runs CPython in isolated mode and explicitly adds only the project root to the import path; installed packages still come from the locked environment. Calls are statically checked against that signature, then dynamically validated at the bridge. Declarations describe expected input and output; they do not prove the Python callable is pure, safe, or actually honors the declared types. In the current build, import, call, and conversion failures become source-traced runtime errors. Recoverable bridge errors represented as `Result` remain planned. Python modules are trusted native host code, not sandboxed guest code.

Source calls to declared Python functions also support keyword arguments. The declared parameter names are the Python keyword names. Positional arguments fill parameters from left to right; named arguments may fill any remaining declared parameter. Names are checked at compile time, and duplicate/unknown names or positional arguments after a keyword produce diagnostics. A declaration lists supported types but does not mark a parameter as required or optional: omitted arguments are passed through as omitted, so Python defaults work; a missing required Python argument becomes a normal `R_PYTHON_CALL` error.

The initial ABI does not support callbacks into Veritas, generators, async Python functions, arbitrary Python object identity, context managers, or returning live handles.

```veritas
extern python "builtins:pow" fn power(base: Int, exp: Int, mod: Int) -> Int

fn main() -> Int:
    return power(2, exp=10)  # Python's default for mod applies
```

## Framed transport

The runtime supervises one explicitly selected CPython child per loaded project instance. The control channel uses four-byte unsigned big-endian frame lengths followed by UTF-8 JSON. The current protocol version is `1`; the maximum frame is 24 MiB to accommodate base64-encoded byte values up to the default 16 MiB file ceiling, and the maximum nesting depth is 64. Every request and response has a monotonically increasing request ID. Frames contain one complete value; trailing bytes and unknown required fields are protocol errors.

The sidecar duplicates its framed output descriptor, then redirects native OS-level writes to stdout/stderr away from the protocol pipe; this prevents native writes from corrupting frames but currently drops native-level output. Python-level `print` output is captured and returned as bounded log fields. Veritas stdout remains owned by the Veritas host. A request timeout terminates and reaps the child. EOF, invalid JSON, mismatched request IDs, oversized frames, and a child crash produce distinct bridge errors. A timeout can occur after Python has caused external effects; the runtime does not claim rollback.

## Value conversion

| Veritas value | Wire form | Python value | Return conversion |
|---|---|---|---|
| `Unit` | `{"t":"unit"}` | `None` | `None` to `Unit` |
| `Bool` | `{"t":"bool","v":true}` | `bool` | Exact bool only; Python bool is not accepted as integer |
| `Int` | `{"t":"int","v":"-123..."}` | arbitrary precision `int` | Integer encoded as decimal text; exact, digit-limited |
| `Float64` | `{"t":"f64","v":"-0.125"}` | finite Python `float` | Round-trippable decimal representation of the IEEE 754 binary64 value; NaN and infinities are rejected |
| `Text` | `{"t":"text","v":"..."}` | `str` | UTF-8, bounded by configured text limit |
| `Bytes` | `{"t":"bytes","v":"<base64>"}` | `bytes` | Base64 preserves every byte; bounded by configured file-byte limit |
| `List[T]` | `{"t":"list","v":[...]}` | `list`; a top-level NumPy `ndarray` return is first materialized as nested Python lists | Recursively validated against `T`; nested array dimensions must match nested `List` declarations |
| `Map[Text, T]` | `{"t":"map","v":{"key":...}}` | `dict[str, T]` | String keys and values recursively validated against `T`; key order is canonicalized by Veritas |
| `Json` | `{"t":"json","v":<JSON value>}` | JSON-compatible Python value | `None`, exact `bool`/`int`/`str`, finite `float`, exact `list`, and exact `dict` with string keys; returned recursively as Veritas `Json` |

`Json` preserves heterogeneous arrays and objects without requiring callers to declare their internal shape. Python integers remain exact, including values beyond binary64 precision; Python floating-point values must be finite. The bridge rejects cycles, non-string object keys, tuples, subclasses and other non-JSON Python objects. JSON conversion is limited to 64 nested levels and to the existing 24 MiB framed transport ceiling. Nested `Json` is one value and is not coerced into a typed `List` or `Map`.

When a Python function is declared to return `List[T]` (including nested lists), a top-level NumPy `ndarray` with boolean or numeric elements up to 64 bits is converted with its `tolist()` method and validated against the declared Veritas shape and element type. Object, complex and wider-than-64-bit dtypes are rejected. Before materializing, the sidecar rejects arrays with more than 98,304 elements; this uses a conservative 256-byte per-element temporary-object budget against the 24 MiB frame ceiling. The final encoded frame must also fit the 24 MiB limit. This allocates Python lists and then serializes the values through the framed channel; it is a convenience conversion with copy and transfer costs, not a zero-copy array interface. Arrays embedded inside unrelated Python objects are not recursively adapted.

`Unit`, `Bool`, `Int`, finite `Float64`, `Text`, `Bytes`, `Json`, and recursively nested `List[T]` and `Map[Text, T]` values currently cross source-level calls. Python dictionaries used as typed `Map` values must have string keys. Python exceptions become `R_PYTHON_CALL` runtime errors with exception type, bounded message and Python traceback; a future bridge surface will represent recoverable errors as `Result`.

`Any` is not a public Veritas type in this ABI. Cycles, tuples, sets, arbitrary classes, file handles, NumPy arrays returned against types other than `List[T]`, tensors, decimal objects, and extension-defined values are rejected unless a documented adapter maps them into the table. `Json` is a data interchange shape, not zero-copy access to Python objects or numeric arrays. Large arrays are copied into Python lists, serialized, and reconstructed as Veritas lists, so they incur adapter and transfer costs.

## Error records

Bridge errors carry a stable Veritas code, Python exception class, bounded message, bounded formatted traceback, module/call name, and request ID. The Python traceback is diagnostic text, not a Veritas source trace. Error messages and logs are treated as untrusted data. Secret values are not included in automatic argument dumps.

## Environment and locking

Projects can declare managed Python dependencies in `veritas.toml`:

```toml
[python]
version = "3.14.8"
dependencies = ["requests==2.32.5", "orjson==3.13.0", "numpy==2.5.4"]
```

`veritas sync <project>` creates a separate uv project under `.veritas/python-bridge`, retains uv's complete `uv.lock`, and writes `veritas.lock` with the Veritas manifest digest, uv and CPython versions, target, and uv-lock digest. The checked-in `uv.lock` retains the full resolved graph, artifact hashes, markers, indexes, and wheel identities; Veritas does not parse or rewrite uv's lock format. A first sync resolves dependencies; later syncs check the lock. `veritas sync --update <project>` explicitly changes the resolution. A run verifies both lock digests, syncs offline with uv's locked mode, verifies the managed interpreter's exact version, and starts the sidecar with that interpreter. A stale or missing lock fails with repair guidance. The per-project `.venv` is generated and should not be committed.

The current-host installer distribution pins uv 0.13.4 separately for macOS ARM64 and GNU/Linux ARM64. During development, `veritas sync` uses uv to acquire the exact managed CPython version and locked dependencies. When `veritas package` builds a Python-assisted app from a synced project, the bundle includes managed CPython, the locked bridge environment, required wheels, and the immutable offline cache; its launcher verifies recorded hashes before each run. Same-target offline launch is supported for selected packages on macOS ARM64 and Debian 12-compatible GNU/Linux ARM64. Arbitrary native wheels, other Linux distributions, libc implementations, and architectures remain outside the documented support boundary. Local source builds use the bundled resolver when present, otherwise `VERITAS_UV` or a matching uv on `PATH`. Projects without `[python]` can still use the explicit interpreter path supplied through `VERITAS_PYTHON` or the embedding `HostPolicy`; Veritas never searches for `python` or `python3` by itself.

A compiled extension may depend on system libraries or platform details that prevent relocation. The package command must list those dependencies or reject the bundle; a working local import alone is not portable-package evidence.

## reference.md
# Language and command reference (current build)

For supported artifact, package, Python and host versions plus upgrade guidance, see [`compatibility-and-migrations.md`](compatibility-and-migrations.md).

## Minimal program

```veritas
fn main() -> Int:
    let greeting = "Hello"
    print(greeting + ", Veritas!")
    return 0
```

Function parameters and return types are explicit; local types are inferred. Use `var` for a binding that changes and `set` to update it. `let` is immutable. Function calls are statically resolved. `print` writes one line.

`List[T]` holds same-typed values. Use `[first, second]` for a literal, `values[index]` for zero-based indexing, `for value in values:` to iterate, and `len(values)` to count. Text length and indexing count Unicode scalar values; `text_chars(text)` materializes them into a bounded list for linear scans.

`Option[T]` represents a value that may be absent. Return `Some(value)` or `None` from a function declared as `Option[T]`, test presence with `is_some`, and use `unwrap` after that check. `veritas run language/examples/option.vr` demonstrates the flow.

## Commands

- `veritas new DIRECTORY` creates a starter project and refuses an existing path.
- `veritas check FILE.vr` compiles and reports the function count. Add `--json` after the path for a versioned success report or a structured source diagnostic with a stable code and line/column.
- `veritas test FILE.vr` runs each zero-argument `Unit` function named `test_*` in its own fresh runtime instance. Add `--json` after the path for a stable JSON report on stdout with schema version, per-test status, pass/fail counts, and source traces for runtime failures. Failed tests still exit nonzero; stderr remains available for host-tool output.
- `veritas run FILE.vr` compiles and executes source.
- `veritas run --json FILE.vr [ARGS...]` returns a versioned JSON run report containing captured program output, exit status, or compiler/runtime diagnostics with source locations. Its `effects` list reports captured output and host operations with `started`, `returned`, `returned_error`, or `interrupted` outcomes; `omitted_effects` counts records beyond the 1,024-entry cap. Targets are capped at 256 Unicode scalar values and set `target_truncated` when shortened. HTTP targets report method and host without URL paths, query strings, or credentials. Python-assisted runs also report native image paths observed as newly resident at Python call boundaries in `python_native_images`, with the observation method in `python_native_image_observation`. The current macOS observer uses dyld snapshots; it can miss images loaded and unloaded between snapshots. Other hosts report `unsupported`. The JSON flag precedes the file path so remaining arguments pass unchanged to the program.
- `veritas fmt FILE.vr` normalizes line endings and trailing whitespace while preserving comments and string contents; `fmt --check` reports unformatted files.
- `veritas run language/examples/json_extract.vr` demonstrates bounded JSON parsing, exact integer access, and stable serialization.
- `veritas run language/examples/float64.vr` demonstrates finite floating-point arithmetic and explicit JSON number conversion.
- `veritas run language/examples/integer_division.vr` demonstrates truncating division, floor division, and divisor-signed remainder.
- `veritas run language/examples/http_get.vr URL` demonstrates bounded HTTP text retrieval; grant the URL's exact hostname under `[network].hosts` first.
- `veritas run PROJECT_DIRECTORY` resolves the entry file from `veritas.toml`.
- `veritas build FILE.vr [-o FILE.vrb]` writes a versioned portable bytecode artifact (default output replaces `.vr` with `.vrb`). The current format is 33. Artifact metadata records the compiler version, build host, exact source graph digest, Python lock digest when both lock files exist, and effective runtime ceilings; ceilings follow the artifact when it is moved.
- `veritas package PROJECT_DIRECTORY [--output DIRECTORY]` creates a current-host, source-free application directory containing the compiled artifact, matching Veritas runtime, project policy, launcher, and declared resources. For projects with `[python]`, it also bundles pinned CPython, pinned `uv`, the locked bridge project, dependency cache, and declared project Python modules. `PYTHON-DEPENDENCIES.json` inventories managed Python distributions and runtime notices. `NATIVE-DEPENDENCIES.json` records macOS Mach-O dependencies; `linux-dependencies.json` records Linux ELF dependencies, runtime resolutions and required glibc versions. Both target audits record hashes and architectures, attribute installed native Python files through wheel `RECORD` entries, and fail on unresolved dependencies or unattributed extensions. The inventory copies notices but does not determine legal compliance. The launcher can rebuild the Python environment offline after moving the folder on the same OS and architecture. Supported package targets are macOS ARM64 and Debian 12-compatible GNU/Linux ARM64; the command refuses to overwrite an existing destination.
- `veritas verify-package PACKAGE_DIRECTORY` checks the package target and recorded SHA-256 digests for the artifact, runtime, resources, managed Python tree, pinned `uv`, Python lock, dependency and native reports, copied license files, and native images, including native files in the offline wheel cache. The package launcher skips this full-tree hash scan by default to keep short commands responsive; set `VERITAS_VERIFY_PACKAGE=1` to run it before launch. The reports are unsigned, so they detect accidental corruption but do not authenticate the publisher. Native dependency reports contain target-specific static load records; dynamic Python image observations are emitted on macOS through `run --json`.
- `veritas sync PROJECT_DIRECTORY` resolves and installs the declared managed Python environment without changing an existing lock; use `veritas sync --update PROJECT_DIRECTORY` to change the dependency resolution. Updates resolve and install into a separate staging environment, then replace the environment and lock metadata after validation. Resolution, installation, or interpreter-validation failures leave the previous environment and locks in place. Add `--json` before or after the project path for a versioned report containing success/failure status, whether dependency resolution was updated, and the Veritas and uv lock fingerprints. Setup failures use stable diagnostic codes including `E_SYNC_CONFIGURATION`, `E_UV_NOT_FOUND`, `E_UV_VERSION_MISMATCH`, `E_UV_UNAVAILABLE`, `E_SYNC_LOCK_INVALID`, `E_DEPENDENCY_RESOLUTION_FAILED`, `E_DEPENDENCY_INSTALL_FAILED`, and `E_MANAGED_PYTHON_UNAVAILABLE`.
- `veritas run FILE.vrb` executes a compiled artifact without source.
- `veritas inspect FILE.vr` or `.vrb` prints artifact and function metadata, including recorded build limits when present, plus statically reachable runtime effects. `veritas inspect --json FILE.vr` emits schema-versioned function call/effect data and opaque Python declarations. This is bytecode reachability analysis for known host operations, not a sandbox, a proof of Python behavior, or a complete observation of native code and operating-system effects.
- `veritas doc FILE.vr` prints function, record, and declared Python signatures. `veritas doc --json FILE.vr` emits a versioned `veritas_api_report` with typed function parameters and results, record fields, and Python module/callable identities. Function parameter names are not retained in the current compiled artifact; their types are reported in source order.
- `veritas build --json FILE.vr [--output ARTIFACT]` emits a versioned build report with artifact format, size and SHA-256, source digest, compiler version, and host target. Compile failures use the structured diagnostic format. Human-readable build output remains the default.
- `veritas package --json PROJECT [--output DIRECTORY]` emits a package report with target, package schema, artifact digest, metadata digest, resource count, and whether a Python runtime was bundled. Packaging failures emit a stable diagnostic; source compile failures retain their compiler code and source location. `veritas verify-package --json DIRECTORY` reports verified package identity or a structured verification failure. Both commands remain human-readable by default.
- `veritas doctor` reports the current host and optional tool discovery. Add `--json` for a versioned readiness report with runtime and host identity, Python sidecar mode, managed `uv` availability, and separate pure-Veritas and Python-call capability flags. Missing optional Python tooling does not mark the native Veritas runtime unavailable.

For managed Python calls, add an exact `[python].version` and a `dependencies` array to `veritas.toml`. `sync` writes a native uv lock under `.veritas/python-bridge/` and `veritas.lock` at the project root. Project runs verify those locks and synchronize the environment offline. The current-host bundle includes pinned uv 0.13.4; local source builds may set `VERITAS_UV` to that executable.

Pure-Veritas packages can include runtime data files explicitly:

```toml
[package]
resources = ["assets/schema.json", "data/lookup.csv"]
```

Resource paths must stay inside the project, name files, and avoid package-owned paths. List imported Python modules alongside data files in `[package].resources`; package execution imports them from the package root. The package writes `PACKAGE.json` with target and SHA-256 digests. Schema 2 includes integrity hashes for immutable Python offline-cache payloads. uv bookkeeping files matching `.lock`, `*.lock`, `*.http` and `*.msgpack` are excluded because offline runs update them. Schema 1 packages remain verifiable. Run the result with `./run [arguments]`. Files not listed as resources are not copied. Same-target relocation is supported for selected Python wheels on macOS ARM64 and Debian 12-compatible GNU/Linux ARM64. Target-specific audits attribute installed native files through wheel `RECORD`; they do not prove ownership of vendored source code, and static scans cannot observe libraries opened only through runtime APIs.

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

For networked projects, declare exact hostnames in `[network].hosts`; requests are bounded by the project network policy. Declared Python calls use the project-managed interpreter and locked dependencies.

Runtime errors go to stderr and return a failing process status. Output produced before a runtime error is preserved on stdout. Diagnostics include stable error codes and source line/column locations.

`print(value)` writes program output to stdout. `eprint(value)` writes program diagnostics to stderr. Both are captured independently by `veritas run --json`, reported as distinct effects, and charged to the combined output limit. `eprint` requires artifact format 28.

## Current limits

Runtime resource ceilings can be set in a project's `veritas.toml` under `[runtime]` and are also available on the embedding `HostPolicy`. Every field must be a positive integer; unknown fields fail with a manifest error. The default cumulative allocation budget is 268435456 estimated bytes per invocation. It counts input value footprints, collection/record/text/byte/JSON construction, host results, and output rendering across nested calls. Interpreter/JIT frames, operand stacks, and scalar arithmetic temporaries are excluded so backend selection does not change this limit; integer growth has its own digit ceiling. Parallel workers share a counter and leave a portion available to handle their result. Exhaustion reports `R_ALLOCATION_LIMIT`. The estimate covers guest data at known creation points; it is not exact native heap/RSS accounting, and OS/runtime/third-party allocations may differ. Other limits such as instruction count, call depth, output, file bytes, text bytes, collection items, and integer digits remain independent.

Embedders may cancel a running invocation by creating a `veritas_runtime::CancellationToken`, cloning it to a control thread, and calling `Instance::invoke_with_cancellation(export, args, token)`. VM code checks between instructions; native code checks at entry and backward branches; parallel workers stop scheduling and are joined; Python calls are terminated and reaped after cancellation is noticed (polled every 20 ms). A blocking HTTP call is observed only after it returns or reaches its configured timeout. Completed side effects are not undone, so cancellation does not mean rollback. Use a fresh token per invocation. The command-line program currently follows normal process Ctrl-C behavior rather than returning `R_CANCELLED`.

```toml
[runtime]
allocation_limit_bytes = 268435456
instruction_limit = 2000000
max_parallelism = 8
```

The current types are `Int`, `Float64`, `Bool`, `Text`, `Bytes`, `Json`, `JsonError`, `FileError`, `HttpError`, `Unit`, `TaskError`, `List[T]`, `Map[Text, T]`, `Option[T]`, and `Result[T, E]`. `Int` uses decimal arbitrary-precision storage subject to a configurable digit limit. Integer `/` truncates toward zero, `//` rounds down, and `%` follows the divisor's sign, satisfying `a == (a // b) * b + (a % b)`; division by zero raises `R_DIV_ZERO`. `Float64` is finite IEEE 754 binary64; `+ - * /`, unary negation, equality and ordered comparisons require Float64 operands. `sqrt(Float64)` returns a finite square root, reporting `R_MATH_DOMAIN` for negative input and `R_FLOAT_NONFINITE` for a non-finite result. No implicit conversion occurs between `Int` and `Float64`. Non-finite literals and operation results fail; float division by zero raises `R_DIV_ZERO`.

Format-16 artifact metadata records numeric resource ceilings and source/dependency provenance. Format 17 adds `Bytes` values and `read_bytes`/`write_bytes`; format 18 adds source-level Python keyword calls; format 19 adds the HTTP GET host operation; format 20 adds typed JSON construction and updates; format 21 adds atomic text replacement; format 22 adds `args_from` for CLI subcommands; format 23 adds bounded HTTP methods, headers, request bodies, and response envelopes; format 24 adds manifest-granted environment reads; format 25 adds typed empty list and map literals; format 26 adds immutable `list_push`/`list_set` and `sqrt(Float64)`; format 27 adds a per-request HTTP timeout capped by host policy; format 28 adds native guest stderr with separate effect reporting; format 29 adds typed JSON column extraction; format 30 adds streaming JSON column projection; format 31 adds Unicode-scalar `text_chars`; format 32 adds copy-on-write local list append; format 33 adds copy-on-write local JSON-array append. Artifacts that use these features require their respective format. At runtime, the effective ceiling is the stricter of the artifact limit and the host's `HostPolicy`; a host can tighten a limit but cannot loosen a limit promised by the artifact. The source SHA-256 includes sorted relative module paths and exact UTF-8 source bytes. If both `veritas.lock` and `.veritas/python-bridge/uv.lock` exist, their combined SHA-256 is recorded; absent locks are reported as absent. Format-15 metadata and format-14 artifacts remain readable. Format-14 has no embedded policy. Metadata hashes identify build inputs; they do not authenticate a publisher or make artifacts tamper-proof. Compiler compatibility ranges, native targets, and Veritas package dependency identities are not yet recorded.

Built-ins include `args()`, `args_from(start)`, `read_text(path)`, `write_text(path, content)`, `write_text_atomic(path, content)`, `read_bytes(path)`, `write_bytes(path, content)`, `http_get(url)`, `assert(condition)`, `len(list_or_map_or_bytes)`, `list_push(list, value)`, `list_set(list, index, value)`, `sqrt(Float64)`, `map_set(map, key, value)`, `map_has(map, key)`, and `parallel_map(list, function_name, worker_limit)`, plus JSON bulk conversions `json_int_column`, `json_text_categories`, and `json_as_int_list`, and format-30 streaming projection via `json_project_columns` / `json_text_categories_values`. `list_push` appends and `list_set` replaces an existing zero-based index; both return immutable values. Assigning `list_set(name, index, value)` back to that same mutable local uses copy-on-write storage and clones only when an alias must retain the old value. `args_from` returns the CLI argument suffix beginning at a zero-based index. Bytes are immutable, preserve all values including invalid UTF-8 and NUL, and use the `max_file_bytes` ceiling. Maps support literals with unique text keys, homogeneous values, deterministic sorted-key iteration, immutable updates, key lookup and `len`; empty literals need an expected `Map[Text, T]` type. Empty lists likewise need an expected `List[T]` type, which can come from a local annotation, return type, function parameter, enclosing collection, or typed operation. Typed `Map[Text, T]` values cross Python calls as dictionaries with string keys. Python interop also supports finite `Float64`, `Bytes` (base64 framed as Python `bytes`), `Json`, top-level NumPy arrays returned against a `List[T]` declaration, and source-level keyword arguments mapped to declared parameter names. NumPy boolean/numeric arrays up to 64 bits are materialized with `tolist()` and checked against the declared nested list shape and type; arrays are limited to 98,304 elements before materialization and data is copied. A `Json` bridge value preserves nested heterogeneous JSON and arbitrary-precision integers; Python cycles, non-finite values, non-string object keys and non-JSON objects are rejected. Omitted arguments are passed through as omitted so Python defaults can apply. JSON construction uses `json_null`, `json_bool`, `json_int`, `json_float`, `json_text`, `json_array`, `json_object`, `json_push`, and `json_set`; constructors and immutable updates require artifact format 20. `write_text_atomic` requires format 21 and performs atomic visibility replacement without writer locking.

`Option` constructors `Some(value)` and `None` are available inside functions declared to return `Option[T]`; use `is_some(option)` and `unwrap(option)`, which raises `R_OPTION_UNWRAP` for absence. Text and byte file operations return `Result` values; inspect errors with `file_error_code/message/line/column`. Missing paths return `R_FILE_NOT_FOUND`, permission failures return `R_FILE_PERMISSION`, and file-size policy failures remain runtime errors. `write_text` and `write_bytes` write directly; `write_text_atomic` atomically replaces a text file but does not coordinate multiple writers or promise directory-entry durability after sudden power loss. `Ok`, `Err`, and postfix `?` are available in functions with a declared `Result` return type; Result and Option values can be handled with exhaustive statement matches; see below. General Result inference remains unimplemented. `json_parse` returns `Result[Json, JsonError]`; malformed syntax and duplicate keys are inspectable through `json_error_code/message/line/column`. Resource-limit and typed access failures remain runtime errors. Other JSON operations include `json_stringify`, `json_get`, `json_index`, `json_has`, `json_len`, `json_as_text`, `json_as_int`, `json_as_float`, `json_as_bool`, and `json_is_null`. `json_as_float` converts a JSON number to finite `Float64`; nonnumeric or non-finite conversion fails with `R_JSON_TYPE`. `parallel_map` returns `Result[List[U], TaskError]`; inspect it using `is_ok`, `unwrap`, `error`, `task_index`, `task_code`, and `task_message`. An `extern python "module:callable" fn alias(...) -> Type` declaration calls through a supervised sidecar. A project with `[python]` uses the exact locked managed interpreter; otherwise use an explicit `VERITAS_PYTHON` path. Current Python conversions cover `Unit`, `Bool`, `Int`, finite `Float64`, `Text`, `Bytes`, `Json`, and recursively nested lists/maps with string keys; calls support positional and keyword arguments mapped to declared parameters, and omitted parameters are left to Python defaults. Python failures are runtime errors. The CLI grants file access within the project root; embedded use must grant a root explicitly. This root check is not a sandbox. Network host grants do not prevent DNS rebinding or enforce operating-system egress controls.


## Records

Records declare an ordered set of typed fields. Construction supplies values in declaration order; access uses the field name. Values are immutable and equality is structural for values of the same declared record type.

```veritas
record Point:
    x: Int
    y: Int

fn main() -> Int:
    let origin = Point(0, 0)
    let next = Point(origin.x + 1, origin.y)
    print(next.x)
    return 0
```

Record schemas and values were introduced in version-13 artifacts; format 14 added source maps, format 15 added initial build metadata, and format 16 added metadata schema versioning and source/dependency identities. Records currently cannot cross the Python interop boundary. See `language/examples/records.vr` for an executable example.

## Modules

An entry module can import another file by a relative path. Imported modules expose only declarations marked `pub`; their exported functions and records are available by name in the importing module. Private helpers stay private. The compiler rejects import cycles, path escapes, and duplicate declaration names across the graph.

```veritas
import "math.vr"

fn main() -> Int:
    print(double(21))
    return 0
```

See `language/examples/modules_main.vr` and `language/examples/modules_math.vr`. Compile a project directory or entry source file with imports so Veritas can resolve the full graph. Runtime errors retain the source file that produced them.

## Option and Result matching

`match` is a statement for `Option[T]` and `Result[T, E]`. It requires exactly one arm for each variant, allows the arms in either order, and scopes each bound name to its arm. Wildcards, nested patterns, other matched types, and match expressions are not supported yet.

```veritas
match calculation:
    Ok(value):
        print(value)
    Err(message):
        print(message)
```

See `language/examples/matching.vr` for both variants and both arm orders.

## Installing a current-host release

After extracting a Veritas distribution, run `sh install.sh` to install the versioned files under `~/.local/share/veritas/` and create `~/.local/bin/veritas`. Pass a prefix to choose another location. The installer leaves existing unrelated `bin/veritas` commands intact and does not edit shell startup files. Add the reported `bin` directory to `PATH` using the method appropriate for your shell. Installing a new distribution version updates the managed launcher and keeps prior version directories available. This installer currently targets the packaged host only.

The embedding API exposes `Runtime::vm_only()` for deterministic diagnosis and VM/JIT comparisons; `Runtime::new()` enables automatic JIT selection for eligible pure numeric functions.

## setup-guide.md
# Task setup and delivery guide

This sheet defines the supported campaign setup paths for both candidates. The campaign runs on Debian-compatible Linux ARM64 with CPython 3.14.8 and uv 0.13.4. The same inputs, loopback network fixtures, CPU/memory limits, workspace rules, and offline wheelhouse apply to both languages. Python interop in a Veritas candidate must use that candidate project’s declared `[python]` version and lock; campaign-wide `VERITAS_PYTHON` and `PYTHONPATH` overrides are prohibited. Resolve only the shared pinned wheelhouse packages through the candidate project setup.

## Python

Create a project with `pyproject.toml`, a committed `uv.lock`, and a normal application entry point. Invoke setup with `uv sync --locked --offline`; use the shared offline wheelhouse for third-party dependencies. The user-facing run command must use `python3` or `uv run`, never an assumed `python` launcher. Build a wheel or zipapp before the task is reported complete. The delivered artifact must run from a new path with the original source tree absent. Record the exact sync/build/run commands, lock digest, package/dependency sizes, cold sync time, and warm run time. Dependencies not present in the wheelhouse are unavailable to both candidates.

## Veritas

Create a normal Veritas project with its declared `veritas.lock` and optional managed Python bridge lock. Use the documented `veritas sync`, `veritas test`, `veritas build`, and `veritas package` workflows as appropriate. The final package must launch from a new path with the original source tree absent. If Python interop is used, declare it through the managed project configuration and lockfile; do not call an undeclared system interpreter. Record exact sync/build/package/run commands, lock digests, artifact/dependency sizes, cold sync time, and warm run time. Any third-party dependency must come from the shared offline wheelhouse or be unavailable to both candidates.

## Required delivery manifest

Place `delivery.json` at the project root:

```json
{
  "launch_command": ["${candidate}/dist/app", "--help"],
  "artifact_paths": ["dist/app", "veritas.lock"],
  "source_paths": ["src", "main.vr"],
  "dependency_lock_path": "veritas.lock"
}
```

The launch command is an argument array executed without a shell. `${candidate}` names the relocated artifact directory and `${workspace}` names the clean directory containing task inputs and outputs. `artifact_paths` must include the lock named by `dependency_lock_path`. `source_paths` describe source trees/files that are excluded from the relocated run; they cannot overlap any delivered artifact. The evaluator hashes the delivered artifact set, stages task files in a fresh workspace, and runs the same cases against the relocated launch command. If a task's fixtures require application-generated state, that state must be created by the application from the fixture inputs.

The evaluator runs source acceptance and relocated delivery in separate restricted containers. The relocated container receives the same task inputs and fixtures but mounts only the declared delivery artifacts; the original source tree is absent. The package session has a fixed budget, so environment setup, packaging, and runtime costs remain visible. A task-class recommendation is exploratory and specific to this model/tooling/host sample; it is not a universal claim about either language.
