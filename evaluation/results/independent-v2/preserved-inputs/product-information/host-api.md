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
