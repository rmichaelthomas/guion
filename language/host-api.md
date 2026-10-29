# Guion host API contract

Status: executable contract for the current build. `print`, `args`, `args_from`, exact-grant environment reads, text and byte file operations, default-deny bounded HTTP requests, explicitly granted clock reads, packaged timezone resolution, `assert`, `len`, JSON operations, and the initial bounded `parallel_map` are implemented.

## Capabilities and policy

The runtime receives a `HostPolicy` when an artifact is loaded. It grants individual capabilities and resource ceilings. Guest source cannot expand those grants. The embedding API and CLI use the same host service implementations. The CLI reads optional ceilings from `[runtime]` in `guion.toml`; embedders can set the same fields in `HostPolicy`. The CLI's default file root is the project root; path checks are defense in depth and do not isolate a process from the operating system.

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

Unknown `[runtime]` fields are errors so spelling mistakes do not silently select defaults. These limits apply to `guion run` and discovered tests; compile-time source and artifact limits are enforced separately.

Environment reads are denied by default. A project opts in to individual variable names with `[environment].variables`; values remain process-provided at launch and are never copied into the project manifest or artifact.

```toml
[environment]
variables = ["API_TOKEN"]
```

Wall-clock access is denied by default. A project opts in with `[clock].enabled = true`, an embedding host uses `HostPolicy::with_clock_access()`, or a single CLI run uses `guion run --allow-clock`. Clock reads are nondeterministic and are reported as `clock.read` by `guion inspect --json`. `time_today_local()` follows the host process time zone; the time zone and daylight-saving database come from the host operating system.

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
| General HTTP request | `http_request(method: Text, url: Text, headers: Map[Text], body: Text) -> Result[Json, HttpError]` | Exact-host grant; GET/POST/PUT/PATCH/DELETE/HEAD/OPTIONS; bounded UTF-8 request/response bodies; same-origin redirects; non-2xx status returned in JSON envelope | Implemented; text response decoding is checked UTF-8 |
| Byte HTTP request | `http_request_bytes(method: Text, url: Text, headers: Map[Text, Text], body: Bytes, timeout_ms: Int) -> Result[HttpResponse, HttpError]` | Exact-host grant; exact request/response bytes; bounded total deadline and body; redirects stay on the initial origin; cancellation interrupts in-flight reads | Implemented in artifact format 38 |
| HTTP response text | `http_response_text(response: HttpResponse) -> Result[Text, HttpError]` | Checks UTF-8, enforces `max_text_bytes`, and preserves non-2xx status as response data; invalid UTF-8 returns `R_HTTP_ENCODING` | Implemented in artifact format 45 |
| HTTP response JSON | `std:http.http_response_json(response: HttpResponse) -> Result[Json, StdError]` | Checks the response text ceiling and UTF-8, then parses JSON directly from the response bytes; preserves HTTP and JSON error codes and messages in `StdError` | Implemented in standard library; requires format 47 |
| UTC date formatting | `format_utc_date(value: Instant) -> Text` | Formats the instant's UTC calendar date as `YYYY-MM-DD`, without formatting a clock time | Implemented in standard library; requires format 48 |
| clock | `unix_time_ms() -> Int`, `time_today_local() -> Date` | Explicit clock grant; `unix_time_ms` is milliseconds since Unix epoch and local date follows the host process time zone | Implemented in artifact format 39 |
| local time resolution | `resolve_local(LocalDateTime, Text) -> Result[TemporalResolution, TimeError]`, `time_zone_data_version() -> Text` | Pure lookup using Guion's packaged IANA rules; returns every matching instant and makes no clock, file, or network call | Introduced in format 40; current format 42 uses opcode tags 161 and 162 to coexist with format 41 instructions; bundled timezone data is 2026c |
| structured work | `parallel_map(items: List[T], worker: fn(T)->U, limit: Int) -> Result[List[U], TaskError]` | Bounded workers, isolated VM state, ordered outputs, joined scope | Implemented in named-function form |

`HttpError` is an implemented structured error with stable code/message and a source span; inspect it with `http_error_code` and `http_error_message`. `std:http` exports `HttpResponse` and `StdError` through a small shared type module; `std:types` re-exports those same declarations for compatibility. Importing only `std:http` is enough to use the typed response and JSON helper. `HttpResponse` contains integer `status`, lowercase text `headers`, and a byte-preserving `body`. `http_response_text` returns the body as text after checking the text-size ceiling and UTF-8 encoding. `std:http.http_response_json` applies the same checks and parses directly from the response bytes, avoiding a temporary text copy while preserving the guest allocation charge. It maps HTTP decoding failures and JSON parse failures to `StdError` with their stable code and message. Both helpers treat status as data, so a non-2xx response can still be decoded. The legacy general HTTP API and both existing request signatures remain unchanged. `Bytes` is an immutable byte-preserving value introduced in artifact format 17. `len` accepts Bytes and returns the byte count. Foreign calls are recorded as opaque effects even when declarations include types.

CLI projects grant exact HTTP hostnames in `guion.toml` under `[network].hosts`. A one-run grant can be added with `guion run --allow-http-host api.example.test`; repeated options add multiple explicit hostnames. `guion inspect --json` reports whether the entry reaches HTTP and points to the manifest key and run option. Runtime URL hosts still require an explicit grant; no arbitrary host is granted automatically.

`Json`, `JsonError`, and `FileError` are implemented value types. `json_parse`, `read_text`, `write_text`, and `write_text_atomic` return typed Results for syntax/duplicate-key and filesystem access failures; `is_ok`, `unwrap`, and generic `error` inspect them. Missing files return `R_FILE_NOT_FOUND`; permission failures return `R_FILE_PERMISSION`; other read/write failures keep their operation code. `json_error_code/message/line/column` and `file_error_code/message/line/column` expose details. File error locations point to the source host call and survive `?` propagation. `json_get`, `json_index`, `json_has`, `json_len`, `json_as_text`, `json_as_int`, `json_as_bool`, and `json_is_null` access parsed values. Artifact format 20 adds typed JSON constructors and immutable `json_push`/`json_set` updates so programs can create and persist JSON state without a Python bridge. Format 21 adds atomic text replacement. Format 22 adds `args_from(start)` for subcommand-based CLI programs. It prevents readers from observing a partially written replacement but does not coordinate concurrent read-modify-write sequences; simultaneous writers remain last-complete-rename wins. A process crash can leave the sibling temporary file, and directory-entry crash durability is not promised. Missing-key, wrong-container, conversion, and resource-limit errors still fail the invocation.

## Temporal interpretation

`std:time.resolve_local(local, timezone)` uses Guion's packaged timezone rules to classify a valid local date and clock reading as `exact`, `gap`, or `fold`. The returned `TemporalResolution` preserves the original `LocalDateTime` fields and timezone name. `instants` contains one UTC candidate for an exact time, no candidates for a gap, and both ordered UTC candidates for a fold. The resolver never shifts a gap or chooses an occurrence in a fold; the calling program must apply and explain its own policy. The operation is pure: it does not read the current clock, filesystem, or network, so no capability grant is needed. Supported years are 1 through 2582. `TimeError.code` is stable and distinguishes invalid date/time fields (`E_DATE`, `E_TIME`), unknown timezone names (`E_TIMEZONE_UNKNOWN`), unavailable packaged rules (`E_TIMEZONE_DATA`), and rules too complex for the bounded resolver (`E_TIMEZONE_COMPLEX`). `time_zone_data_version()` reports the bundled IANA rule version, currently `2026c`; `guion --version` reports it too.

## File behavior

Paths are interpreted relative to a granted policy root. Absolute paths, parent components, non-UTF-8 names, and canonicalized symlink escapes are rejected. Read and write permissions are independent. Directories, metadata, deletion, and arbitrary file descriptors are not exposed in the first surface. The default CLI grants only the project root; `guion run --file-root DIRECTORY` adds a secondary root, with reads searching the project root first and writes directed to the last granted root. This protects against accidental path traversal; it is not a sandbox against malicious source or native code.

## HTTP behavior

Network access is denied unless the exact hostname is granted. CLI projects configure this in `guion.toml`:

```toml
[network]
hosts = ["feeds.example.org"]
timeout_ms = 15000
max_response_bytes = 16777216
max_request_bytes = 1048576
max_redirects = 5
```

The default is no hosts, a 15-second total request timeout, a 1 MiB request limit, a 16 MiB response limit, and five redirects. Only HTTP/HTTPS URLs without embedded credentials are accepted. `http_get` follows redirects only to granted hosts and returns non-2xx statuses as typed errors. `http_request` restricts redirects to the initial host, caps request and response bodies while streaming, and returns non-2xx status, headers, and UTF-8 response text in its JSON envelope. `http_request_bytes` returns the status, headers, and exact body bytes as a `HttpResponse`; use `http_response_text` for a text body or `std:http.http_response_json` for JSON without an intermediate text copy. Proxies are disabled so ambient proxy settings cannot route a request outside the declared host policy. HTTP requests share one total deadline across connection and response-body reads, and poll both invocation cancellation and `parallel_map` worker cancellation while active. A worker failure interrupts in-flight HTTP requests, stops new work, and joins the remaining workers before returning. Completed external effects are not rolled back. DNS rebinding and network egress confinement require operating-system controls outside the language runtime. Streaming response APIs remain planned.

## Structured concurrency behavior

`parallel_map` runs a named unary function in one worker-local VM per task. It shares immutable input values by copy. Results are returned in input order. The group does not return until workers stop. On failure it stops assigning pending items, sets a cancellation flag that the VM checks between instructions, joins workers, preserves completed output in input-index order, and returns a `TaskError` with the selected input index, original stable runtime code/message, and source trace. Work already completed and host side effects are not rolled back. Nested parallel groups consume the invoking group's budget and worker ceiling.

The current API rejects a zero or over-policy worker count. Worker count reports OS worker IDs observed, not guaranteed CPU speedup. A future async I/O API must have separate documented scheduling and cancellation semantics; this operation is not presented as a general async runtime.

## Durable storage and interoperability services

The `std:types` module exports `StdError`, `SqlValue`, `SqlRow`, `CalendarEvent`, and their typed constructors. `StdError` and `HttpResponse` come from a small shared type module that `std:http` also re-exports. The `std:sqlite`, `std:toml`, `std:crypto`, `std:text`, and `std:calendar` modules expose the service operations below. Native resources are invocation-owned, opaque, non-serializable, and cannot cross `parallel_map` worker boundaries. The runtime closes outstanding handles at invocation completion, rolls back open SQLite transactions, and rejects stale or foreign handles with typed errors.

The public signatures are:

```text
text_casefold(Text) -> Text
text_collapse_whitespace(Text) -> Text
text_normalize(Text, Text) -> Result[Text, StdError]
crypto_hmac_sha256(Bytes, Bytes) -> Bytes
crypto_verify_hmac_sha256(Bytes, Bytes, Bytes) -> Bool
sqlite_open(Text) -> Result[Db, StdError]
sqlite_begin(Db) -> Result[Transaction, StdError]
sqlite_execute(Transaction, Text, List[SqlValue]) -> Result[Int, StdError]
sqlite_query(Transaction, Text, List[SqlValue]) -> Result[List[SqlRow], StdError]
sqlite_commit(Transaction) -> Result[Unit, StdError]
sqlite_rollback(Transaction) -> Result[Unit, StdError]
sqlite_close(Db) -> Result[Unit, StdError]
toml_parse(Text) -> Result[TomlDocument, StdError]
toml_has(TomlDocument, List[Text]) -> Bool
toml_get_text/int/bool(TomlDocument, List[Text]) -> Result[Text/Int/Bool, StdError]
toml_put_text/int/bool(TomlDocument, List[Text], Text/Int/Bool) -> Result[Unit, StdError]
toml_render(TomlDocument) -> Text
calendar_parse(Text) -> Result[List[CalendarEvent], StdError]
```

SQLite paths must be relative to an explicitly granted file root; absolute paths and parent-directory traversal are rejected. For an installed app, `GUION_FILE_ROOT` selects an additional caller-owned data directory, and the SQLite path can be a filename such as `field.sqlite3`. SQLite opens only read-write databases, loads no extensions, and rejects `ATTACH`, `DETACH`, `VACUUM`, `PRAGMA`, and extension-loading SQL. `sqlite_begin` opens one `BEGIN IMMEDIATE` transaction per database; nested transactions are rejected. Query parameters are bound values, result rows and bytes are bounded, and each operation uses a finite busy deadline and cancellation/progress checks. Dropping or closing a database with an open transaction rolls it back. The standard module provides `sqlite_open`, `sqlite_begin`, `sqlite_execute`, `sqlite_query`, `sqlite_commit`, `sqlite_rollback`, and `sqlite_close`.

HMAC operations consume exact `Bytes` without text conversion. `crypto_hmac_sha256(key, body)` returns a 32-byte digest; `crypto_verify_hmac_sha256(key, body, expected)` returns false for any digest whose length is not 32 and uses the crypto library's verification routine for a candidate digest. Secret material is not included in service errors. Unicode operations use ICU4X full locale-independent case folding and its Unicode `White_Space` property; whitespace collapse trims the ends and replaces each internal run with one ASCII space. Neither operation normalizes text. `text_normalize(text, form)` accepts only `NFC`, `NFD`, `NFKC`, and `NFKD`. The shipped Unicode data version is 18.0.0. Python comparisons record `unicodedata.unidata_version`; differences caused by Unicode data releases are reported separately from case-folding algorithm mismatches.

TOML parsing preserves the original source bytes for an unedited document. Typed `toml_get_*` and `toml_put_*` operations reject missing paths, invalid keys, and mismatched scalar types. Edits are reparsed and checked before publication; targeted scalar replacements preserve surrounding source formatting. Unknown TOML values remain intact. `toml_render` returns the original bytes for a no-op and the validated edited document otherwise.

`calendar_parse` returns each `VEVENT` with UID, summary, status, recurrence rule, and either explicit all-day dates or timed `TemporalResolution` values. Timed endpoints preserve their original local values and timezone, and classify gaps and folds without selecting an instant. Embedded `VTIMEZONE` rules take precedence over a same-named IANA zone; floating times and unknown zones are errors. Event RRULE text is retained without expanding events. Exact dependency versions, source hashes, and licenses are recorded in `distribution/native-components.lock.json`.
