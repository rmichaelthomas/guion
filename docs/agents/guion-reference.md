# Guion reference for agents

Generated from the current Guion language contract, builtin signature catalog, standard library, host API, diagnostics, and runnable examples. Use this file as a compact entry point; the linked contracts remain authoritative.

## Syntax summary

## Source and declarations

- Source is UTF-8. Newlines separate statements. Four spaces define one indentation level; tabs and mixed indentation are errors. `#` begins a line comment.
- A source file is a module. It contains one or more `fn name(parameter: Type, ...) -> Type:` declarations and may declare records with `record Name:` followed by indented `field: Type` declarations. `import "relative-path.gu"` loads another source file; the imported module's `pub fn` and `pub record` declarations become available by their declared names. Private declarations remain local to their module. Imports are resolved from the importing file and must stay inside the project root. The graph is limited to 512 files, 128 import levels, 8 MiB per source file, and 64 MiB total source text. Duplicate declaration names and cycles are errors. Types in this executable subset are `Int`, `Float64`, `Bool`, `Text`, `Bytes`, `Unit`, `Json`, `JsonError`, `FileError`, `HttpError`, `TaskError`, `List[T]`, `Map[Text, T]`, `Option[T]`, `Result[T, E]`, and declared record names.
- The entry point is `fn main() -> Int:` or `fn main() -> Unit:` in the project entry module. Calls resolve statically to local declarations, explicitly imported public declarations, or host functions such as `print(value)`. A single source file can be compiled without a project only when it has no imports.
- Function parameters and results are annotated. Local types are inferred unless an optional local annotation is supplied, as in `let pending: List[Int] = []`. `let` creates an immutable local; `var` creates a mutable local; `set name = expression` changes only a mutable local. An annotation supplies context for empty collection literals and must exactly match the initializer type.

## Statements and expressions

Statements: `let`, `var`, `set`, `return`, `if`/`else`, `while`, `for name in list`, exhaustive `match` statements for `Option` and `Result`, and a call expression as a statement. `if` and `while` conditions must be `Bool`. A function body is indented under its declaration. `else` aligns with `if`.

- Expressions: arbitrary-precision decimal integer literals, finite `Float64` literals with a decimal point or exponent, booleans (`true`, `false`), double-quoted UTF-8 text with `\u{H...}` Unicode scalar escapes (one to six hexadecimal digits; surrogate values and values above U+10FFFF are invalid), local/function names, calls, record construction/access, list literals, map literals with unique text keys, list/map/text indexing, parentheses, unary `-` and `not`, and binary `*`, `/`, `//`, `%`, `+`, `-`, comparisons `== != < <= > >=`, and short-circuit `and` / `or`. `Float64` is IEEE 754 binary64; its arithmetic and comparisons require two `Float64` values, reject non-finite literals/results, and never convert `Int` implicitly. `sqrt(Float64)` returns the non-negative square root; a negative input raises `R_MATH_DOMAIN`. Float division by zero raises `R_DIV_ZERO`. Integer `/` truncates toward zero; `//` rounds down; `%` follows the divisor's sign, so `a == (a // b) * b + (a % b)`. All three integer division operators raise `R_DIV_ZERO` on a zero divisor. Map literals infer one homogeneous value type; an empty map needs a `Map[Text, T]` annotation or other expected type, and keys must be text literals. Empty list literals likewise need an expected `List[T]` type. `list_push(List[T], T)` appends and `list_set(List[T], Int, T)` replaces an existing zero-based index; both return immutable copies, and the compiler may use copy-on-write for updates assigned back to the same mutable local while preserving aliases; out-of-range replacement raises `R_LIST_INDEX`. `Map[Text, T]` is immutable, `map_set` returns an updated copy, `map_has` tests membership, keys iterate and display in Unicode scalar lexicographic order, lookup of an absent key raises `R_MAP_KEY_MISSING`, and `len` supports lists, maps, Bytes (byte count), and Text (Unicode scalar count). Text indexing is zero-based by Unicode scalar and out-of-range access is a runtime error. Types include `List[T]`, `Map[Text, T]`, `Option[T]`, and `Result[T, E]`; `Some(value)` and `None` are Option constructors in Option-returning functions. Precedence from low to high: `or`, `and`, comparisons, `+ -`, `* / // %`, unary, calls/indexing/field access/atoms. `+` accepts two Ints, two Float64 values, or two Text values.

## Runtime rules

- `Int` is arbitrary precision subject to the configured digit ceiling. Integer `/` truncates toward zero, `//` rounds down, and `%` follows the divisor sign so `a == (a // b) * b + (a % b)`; all report `R_DIV_ZERO` for a zero divisor. `Float64` stores finite IEEE 754 binary64 values. Operations that overflow to infinity or NaN fail with `R_FLOAT_NONFINITE`; division by either signed zero fails with `R_DIV_ZERO`; `sqrt` of a negative value fails with `R_MATH_DOMAIN`. Python interop transports Float64 through round-trippable decimal strings and rejects non-finite values. Lists are homogeneous, immutable value sequences; indexing is zero-based and out-of-range access is a runtime error. `list_push` and `list_set` return immutable lists, and `list_set` requires an existing index. When `set name = list_push(name, value)` or `set name = list_set(name, index, value)` updates a mutable local, the compiler may reuse uniquely owned storage and copies if any alias exists; this is unobservable and preserves immutable value behavior. Maps are immutable `Map[Text, T]` values with unique literal keys, homogeneous values, deterministic key iteration, immutable `map_set` updates, membership checks via `map_has`, and traced errors for missing keys. Map values may be passed between functions, serialized in versioned artifacts, and crossed through the Python bridge as string-keyed dictionaries when `T` is bridge-supported. Empty lists and maps use their expected type from annotations, function results or parameters, enclosing collections, and typed operations; uncontextualized empty literals produce a diagnostic. `Json` is a bounded JSON tree; object keys are text, duplicate keys are rejected, and object encoding sorts keys. JSON values require explicit accessors and do not silently coerce to source types. Records are immutable ordered named fields, constructed positionally (for example, `Point(3, 4)`) and accessed by field name (`point.x`). Construction requires every field with its exact declared type; record and field names are unique; equality is structural for the same record type. Unknown fields/types and malformed schemas are rejected. Records serialize in compiled artifacts, but are not supported through the Python bridge yet. First-class functions remain for subsequent implementation steps. `Option[T]` supports `Some(value)` and `None` inside functions declared to return `Option[T]`; `is_some(option)` tests presence and `unwrap(option)` returns the value or raises `R_OPTION_UNWRAP`. `Result[T, E]` is available for file operations, JSON parsing, and `parallel_map`; inspect results with `is_ok`, `unwrap`, and `error`. `Ok`, `Err`, postfix `?`, and exhaustive `match` statements are available for `Result[T, E]`.
- Integers compare by mathematical value. Text is Unicode scalar sequence data; `+` concatenates text. Equality requires equal runtime types.
- Locals are frame-owned values. `let` cannot be assigned; `var` can. Each invocation owns its locals. Calls have a bounded stack depth, instructions consume a configurable budget, and output is captured per instance. A shared cumulative estimated guest-data allocation budget covers input values, collection and record construction, text/byte/JSON values, host results, and output rendering across nested calls and parallel workers; exhaustion reports `R_ALLOCATION_LIMIT`. The default is 256 MiB. Interpreter/JIT frames, operand stacks, and scalar arithmetic temporaries are excluded so changing execution backend does not change this limit. Integer growth is bounded separately by `max_integer_digits`. This is language-level estimate accounting, not an exact process-memory ceiling; limits for output, file bytes, text size, collection size, integer digits, and source/artifact size are separate.
- `print(value)` writes its display form plus one newline to the instance output buffer. `args()` returns all CLI arguments as `List[Text]`; `args_from(start)` returns the suffix beginning at a zero-based index and reports `R_ARGUMENT_INDEX` for a negative or past-end index. `env(name) -> Option[Text]` reads one environment variable only when its exact name is granted in `[environment].variables` in `guion.toml` (or by an embedding `HostPolicy`); an ungranted name raises `R_ENV_DENIED`, and an unset granted name returns `None`. `read_text(path) -> Result[Text, FileError]`, `write_text(path, content) -> Result[Unit, FileError]`, `read_bytes(path) -> Result[Bytes, FileError]`, and `write_bytes(path, content) -> Result[Unit, FileError]` are host operations; runtime embedders grant a project root and read/write rights in `HostPolicy`. The CLI grants the source or artifact directory; `guion run --file-root DIRECTORY` adds a further root. Reads search roots in grant order; writes use the last granted root. Relative paths only are accepted, parent traversal is rejected, and canonicalized symlink targets must remain under one granted root. Access/path/filesystem failures return `FileError`; configured size-limit failures remain runtime errors. This is a path policy, not an OS security sandbox. Bytes preserve arbitrary byte sequences and are limited by `max_file_bytes`.
- `http_get(url) -> Result[Text, HttpError]` performs a bounded GET for HTTP or HTTPS text. Network is denied by default. Embedders explicitly grant exact hostnames through `HostPolicy`; CLI projects list them under `[network].hosts` in `guion.toml`. A redirect is followed only when its destination hostname is also granted. Timeouts, response byte ceilings, and redirect counts are configurable; non-2xx statuses and transport/encoding failures return `HttpError`, inspectable through `http_error_code` and `http_error_message`. Host allowlists do not prevent DNS rebinding or provide OS-level egress confinement.
- `http_request(method, url, headers, body) -> Result[Json, HttpError]` supports GET, POST, PUT, PATCH, DELETE, HEAD, and OPTIONS with `Map[Text]` headers and a UTF-8 text body. Its JSON envelope contains integer `status`, text `body`, and lowercase response headers. `http_request_bytes(method, url, headers, body, timeout_ms) -> Result[HttpResponse, HttpError]` preserves arbitrary request and response bytes; `HttpResponse` in `std:types` has `status: Int`, `headers: Map[Text, Text]`, and `body: Bytes`. Non-2xx responses remain values. A shared host client enforces one monotonic deadline through connection and body reads, bounds response accumulation, refuses cross-origin redirects, and interrupts in-flight reads on cancellation. Runtime URL hosts require exact grants in `[network].hosts` or `guion run --allow-http-host HOST`; network remains denied by default.
- `unix_time_ms() -> Int` returns wall-clock milliseconds since the Unix epoch, and `today_local() -> Date` returns the current date in the host process time zone. Clock access is denied unless `[clock].enabled = true` or the caller uses `guion run --allow-clock` / an embedding `HostPolicy` grant. The host supplies local-time and daylight-saving rules; tests and embedders can pin the clock through `HostPolicy::with_clock_unix_time_ms`.
- `parallel_map(items, worker, limit)` calls a named unary function on immutable list items in worker-local VM instances and returns `Result[List[U], TaskError]`. `is_ok(result)` tests the variant; `unwrap(result)` returns the list from `Ok`; `error(result)` returns the `TaskError` from `Err`. `task_index`, `task_code`, and `task_message` read its failed input index and failure details. Workers preserve result order, use a bounded shared worker instruction allowance, stop scheduling and poll cancellation after failure, join running tasks, and merge completed output by input index. An instruction-limit failure inside a worker is a `TaskError`; host setup errors still fail the invocation.
- Runtime and compile errors preserve source spans. Format-17 artifacts add Bytes values and byte file operations; format-18 artifacts add source-level Python keyword calls; format-19 artifacts add HTTP GET and `HttpError`; format-38 artifacts add byte HTTP; format-39 artifacts add explicitly granted clock access; format-16 metadata carries source and dependency-lock provenance; format-15 metadata and format-14 artifacts remain readable. Runtime combines recorded ceilings with host policy by taking the stricter value. The bytecode is an implementation detail apart from the versioned artifact boundary.

## Builtin signatures

```text
Err($E) -> Result[$T, $E]
Ok($T) -> Result[$T, $E]
Some($T) -> Option[$T]
args() -> List[Text]
args_from(Int) -> List[Text]
assert(Bool) -> Unit
bytes_from_ints(List[Int]) -> Bytes
bytes_from_text(Text) -> Bytes
bytes_join(List[Bytes]) -> Bytes
cli_parse(List[Text], List[Record[CliOption]]) -> Result[Record[CliArgs], FileError]
csv_encode(List[List[Text]]) -> Result[Text, FileError]
csv_next(FileReader) -> Result[Option[Record[CsvRow]], FileError]
csv_open(Text) -> Result[FileReader, FileError]
env(Text) -> Option[Text]
eprint(Any) -> Unit
error(Result[$T, $E]) -> $E
file_close(FileReader) -> Result[Unit, FileError]
file_error_code(FileError) -> Text
file_error_column(FileError) -> Int
file_error_line(FileError) -> Int
file_error_message(FileError) -> Text
file_next_line(FileReader) -> Result[Option[Record[SourceLine]], FileError]
file_open_lines(Text) -> Result[FileReader, FileError]
float64_bits(Float64) -> Int
fs_walk(Text) -> Result[List[Record[DirectoryEntry]], FileError]
http_error_code(HttpError) -> Text
http_error_message(HttpError) -> Text
http_get(Text) -> Result[Text, HttpError]
http_request(Text, Text, Map[Text, Text], Text) -> Result[Json, HttpError]
http_request_bytes(Text, Text, Map[Text, Text], Bytes, Int) -> Result[Record[HttpResponse], HttpError]
http_request_with_timeout(Text, Text, Map[Text, Text], Text, Int) -> Result[Json, HttpError]
is_ok(Result[$T, $E]) -> Bool
is_some(Option[$T]) -> Bool
json_array() -> Json
json_as_bool(Json) -> Bool
json_as_float(Json) -> Float64
json_as_int(Json) -> Int
json_as_int_list(Json) -> List[Int]
json_as_text(Json) -> Text
json_bool(Bool) -> Json
json_error_code(JsonError) -> Text
json_error_column(JsonError) -> Int
json_error_line(JsonError) -> Int
json_error_message(JsonError) -> Text
json_float(Float64) -> Json
json_get(Json, Text) -> Json
json_has(Json, Text) -> Bool
json_index(Json, Int) -> Json
json_int(Int) -> Json
json_int_column(Json, Text) -> List[Int]
json_is_null(Json) -> Bool
json_len(Json) -> Int
json_null() -> Json
json_object() -> Json
json_parse(Text) -> Result[Json, JsonError]
json_project_columns(Text, List[Text], List[Text]) -> Result[Json, JsonError]
json_push(Json, Json) -> Json
json_set(Json, Text, Json) -> Json
json_stringify(Json) -> Text
json_text(Text) -> Json
json_text_categories(Json, Text) -> Json
json_text_categories_values(Json) -> Json
json_try_bool(Json) -> Result[Bool, JsonError]
json_try_float(Json) -> Result[Float64, JsonError]
json_try_get(Json, Text) -> Result[Json, JsonError]
json_try_index(Json, Int) -> Result[Json, JsonError]
json_try_int(Json) -> Result[Int, JsonError]
json_try_text(Json) -> Result[Text, JsonError]
len(Sized) -> Int
list_get(List[$T], Int) -> Option[$T]
list_push(List[$T], $T) -> List[$T]
list_set(List[$T], Int, $T) -> List[$T]
map_get(Map[Text, $T], Text) -> Option[$T]
map_has(Map[$T], Text) -> Bool
map_set(Map[$T], Text, $T) -> Map[$T]
native_calendar_parse(Text) -> Result[List[Record[CalendarEvent]], Record[StdError]]
native_crypto_hmac_sha256(Bytes, Bytes) -> Bytes
native_crypto_verify_hmac_sha256(Bytes, Bytes, Bytes) -> Bool
native_sqlite_begin(Db) -> Result[Transaction, Record[StdError]]
native_sqlite_close(Db) -> Result[Unit, Record[StdError]]
native_sqlite_commit(Transaction) -> Result[Unit, Record[StdError]]
native_sqlite_execute(Transaction, Text, List[Record[SqlValue]]) -> Result[Int, Record[StdError]]
native_sqlite_open(Text) -> Result[Db, Record[StdError]]
native_sqlite_query(Transaction, Text, List[Record[SqlValue]]) -> Result[List[Record[SqlRow]], Record[StdError]]
native_sqlite_rollback(Transaction) -> Result[Unit, Record[StdError]]
native_text_casefold(Text) -> Text
native_text_collapse_whitespace(Text) -> Text
native_text_normalize(Text, Text) -> Result[Text, Record[StdError]]
native_toml_get_bool(TomlDocument, List[Text]) -> Result[Bool, Record[StdError]]
native_toml_get_int(TomlDocument, List[Text]) -> Result[Int, Record[StdError]]
native_toml_get_text(TomlDocument, List[Text]) -> Result[Text, Record[StdError]]
native_toml_has(TomlDocument, List[Text]) -> Bool
native_toml_parse(Text) -> Result[TomlDocument, Record[StdError]]
native_toml_put_bool(TomlDocument, List[Text], Bool) -> Result[Unit, Record[StdError]]
native_toml_put_int(TomlDocument, List[Text], Int) -> Result[Unit, Record[StdError]]
native_toml_put_text(TomlDocument, List[Text], Text) -> Result[Unit, Record[StdError]]
native_toml_render(TomlDocument) -> Text
parallel_map(List[$T], NamedWorker[$T, $R], Int) -> Result[List[$R], TaskError]
print(Any) -> Unit
read_bytes(Text) -> Result[Bytes, FileError]
read_text(Text) -> Result[Text, FileError]
semver_compare(Record[SemVer], Record[SemVer]) -> Int
semver_parse(Text) -> Result[Record[SemVer], FileError]
sort_by(List[$T], NamedComparator[$T]) -> List[$T]
sqrt(Float64) -> Float64
task_code(TaskError) -> Text
task_index(TaskError) -> Int
task_message(TaskError) -> Text
text_chars(Text) -> List[Text]
text_parse_float(Text) -> Result[Float64, FileError]
text_parse_int(Text) -> Result[Int, FileError]
time_format_utc(Record[Instant]) -> Text
time_parse_date(Text) -> Result[Record[Date], FileError]
time_parse_timestamp(Text) -> Result[Record[Instant], FileError]
time_today_local() -> Record[Date]
unix_time_ms() -> Int
unwrap(Result[$T, $E]|Option[$T]) -> $T
write_bytes(Text, Bytes) -> Result[Unit, FileError]
write_text(Text, Text) -> Result[Unit, FileError]
write_text_atomic(Text, Text) -> Result[Unit, FileError]
```

## Standard-library public signatures

```text
std:calendar — pub fn calendar_parse(source: Text) -> Result[List[CalendarEvent], StdError]
std:cli — pub fn parse_cli(arguments: List[Text], options: List[CliOption]) -> Result[CliArgs, FileError]
std:collections — pub fn list_get_text(values: List[Text], index: Int) -> Option[Text]
std:collections — pub fn list_get_int(values: List[Int], index: Int) -> Option[Int]
std:collections — pub fn map_get_text(values: Map[Text, Text], key: Text) -> Option[Text]
std:collections — pub fn map_get_int(values: Map[Text, Int], key: Text) -> Option[Int]
std:crypto — pub fn crypto_hmac_sha256(key: Bytes, body: Bytes) -> Bytes
std:crypto — pub fn crypto_verify_hmac_sha256(key: Bytes, body: Bytes, expected: Bytes) -> Bool
std:csv — pub fn csv_open_reader(path: Text) -> Result[FileReader, FileError]
std:csv — pub fn csv_next_row(reader: FileReader) -> Result[Option[CsvRow], FileError]
std:csv — pub fn csv_encode_rows(rows: List[List[Text]]) -> Result[Text, FileError]
std:fs — pub fn fs_read_text(path: Text) -> Result[Text, StdError]
std:fs — pub fn fs_open_lines(path: Text) -> Result[FileReader, FileError]
std:fs — pub fn fs_next_line(reader: FileReader) -> Result[Option[SourceLine], FileError]
std:fs — pub fn fs_close_reader(reader: FileReader) -> Result[Unit, FileError]
std:json — pub fn json_get_safe(value: Json, key: Text) -> Result[Json, JsonError]
std:json — pub fn json_index_safe(value: Json, index: Int) -> Result[Json, JsonError]
std:json — pub fn json_text_safe(value: Json) -> Result[Text, JsonError]
std:json — pub fn json_int_safe(value: Json) -> Result[Int, JsonError]
std:json — pub fn json_bool_safe(value: Json) -> Result[Bool, JsonError]
std:json — pub fn json_float_safe(value: Json) -> Result[Float64, JsonError]
std:jsonl — pub fn jsonl_open(path: Text) -> Result[FileReader, FileError]
std:jsonl — pub fn jsonl_next(reader: FileReader) -> Result[Option[JsonlLine], FileError]
std:jsonl — pub fn jsonl_close(reader: FileReader) -> Result[Unit, FileError]
std:semver — pub fn parse_version(value: Text) -> Result[SemVer, FileError]
std:semver — pub fn compare_versions(left: SemVer, right: SemVer) -> Int
std:sqlite — pub fn sqlite_open(path: Text) -> Result[Db, StdError]
std:sqlite — pub fn sqlite_begin(database: Db) -> Result[Transaction, StdError]
std:sqlite — pub fn sqlite_execute(transaction: Transaction, sql: Text, values: List[SqlValue]) -> Result[Int, StdError]
std:sqlite — pub fn sqlite_query(transaction: Transaction, sql: Text, values: List[SqlValue]) -> Result[List[SqlRow], StdError]
std:sqlite — pub fn sqlite_commit(transaction: Transaction) -> Result[Unit, StdError]
std:sqlite — pub fn sqlite_rollback(transaction: Transaction) -> Result[Unit, StdError]
std:sqlite — pub fn sqlite_close(database: Db) -> Result[Unit, StdError]
std:text — pub fn text_compare_scalar(left: Text, right: Text) -> Int
std:text — pub fn text_compare_bytes(left: Text, right: Text) -> Int
std:text — pub fn text_trim(value: Text) -> Text
std:text — pub fn text_split(value: Text, delimiter: Text) -> List[Text]
std:text — pub fn text_join(values: List[Text], delimiter: Text) -> Text
std:text — pub fn text_replace(value: Text, needle: Text, replacement: Text) -> Text
std:text — pub fn text_parse_integer(value: Text) -> Result[Int, FileError]
std:text — pub fn text_parse_decimal(value: Text) -> Result[Float64, FileError]
std:text — pub fn text_casefold(value: Text) -> Text
std:text — pub fn text_collapse_whitespace(value: Text) -> Text
std:text — pub fn text_normalize(value: Text, form: Text) -> Result[Text, StdError]
std:time — pub fn today_local() -> Date
std:time — pub fn parse_date(value: Text) -> Result[Date, FileError]
std:time — pub fn parse_timestamp(value: Text) -> Result[Instant, FileError]
std:time — pub fn format_utc(value: Instant) -> Text
std:toml — pub fn toml_parse(source: Text) -> Result[TomlDocument, StdError]
std:toml — pub fn toml_has(document: TomlDocument, path: List[Text]) -> Bool
std:toml — pub fn toml_get_text(document: TomlDocument, path: List[Text]) -> Result[Text, StdError]
std:toml — pub fn toml_get_int(document: TomlDocument, path: List[Text]) -> Result[Int, StdError]
std:toml — pub fn toml_get_bool(document: TomlDocument, path: List[Text]) -> Result[Bool, StdError]
std:toml — pub fn toml_put_text(document: TomlDocument, path: List[Text], value: Text) -> Result[Unit, StdError]
std:toml — pub fn toml_put_int(document: TomlDocument, path: List[Text], value: Int) -> Result[Unit, StdError]
std:toml — pub fn toml_put_bool(document: TomlDocument, path: List[Text], value: Bool) -> Result[Unit, StdError]
std:toml — pub fn toml_render(document: TomlDocument) -> Text
std:types — pub fn std_error(code: Text, message: Text) -> StdError
std:types — pub fn sql_null() -> SqlValue
std:types — pub fn sql_integer(value: Int) -> SqlValue
std:types — pub fn sql_real(value: Float64) -> SqlValue
std:types — pub fn sql_text(value: Text) -> SqlValue
std:types — pub fn sql_blob(value: Bytes) -> SqlValue
std:unicode — pub fn unicode_casefold(value: Text) -> Text
std:unicode — pub fn unicode_collapse_whitespace(value: Text) -> Text
std:unicode — pub fn unicode_normalize(value: Text, form: Text) -> Result[Text, StdError]
```

## Effects and grants

Guest source cannot expand the host grants. File access is limited to granted roots and relative paths. Network access is denied by default and requires exact host grants. Environment variables and clock reads also require explicit grants. Resource limits are enforced by the host policy. This is a capability policy inside the runtime, not an operating-system sandbox.

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

The default is no hosts, a 15-second total request timeout, a 1 MiB request limit, a 16 MiB response limit, and five redirects. Only HTTP/HTTPS URLs without embedded credentials are accepted. `http_get` follows redirects only to granted hosts and returns non-2xx statuses as typed errors. `http_request` restricts redirects to the initial host, caps request and response bodies while streaming, and returns non-2xx status, headers, and UTF-8 response text in its JSON envelope. Proxies are disabled so ambient proxy settings cannot route a request outside the declared host policy. The blocking HTTP client has a request deadline but cannot be interrupted mid-read by `parallel_map` cancellation; the worker group waits for that deadline. DNS rebinding and network egress confinement require operating-system controls outside the language runtime. Binary bodies and streaming APIs remain planned.

## Structured concurrency behavior

`parallel_map` runs a named unary function in one worker-local VM per task. It shares immutable input values by copy. Results are returned in input order. The group does not return until workers stop. On failure it stops assigning pending items, sets a cancellation flag that the VM checks between instructions, joins workers, preserves completed output in input-index order, and returns a `TaskError` with the selected input index, original stable runtime code/message, and source trace. Work already completed and host side effects are not rolled back. Nested parallel groups consume the invoking group's budget and worker ceiling.

The current API rejects a zero or over-policy worker count. Worker count reports OS worker IDs observed, not guaranteed CPU speedup. A future async I/O API must have separate documented scheduling and cancellation semantics; this operation is not presented as a general async runtime.

## Durable storage and interoperability services

The `std:types` module defines `StdError`, `SqlValue`, `SqlRow`, `CalendarEvent`, and their typed constructors. The `std:sqlite`, `std:toml`, `std:crypto`, `std:text`, and `std:calendar` modules expose the service operations below. Native resources are invocation-owned, opaque, non-serializable, and cannot cross `parallel_map` worker boundaries. The runtime closes outstanding handles at invocation completion, rolls back open SQLite transactions, and rejects stale or foreign handles with typed errors.

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

`calendar_parse` returns each `VEVENT` with UID, summary, status, recurrence rule, and either explicit all-day dates or UTC Unix seconds. It resolves an embedded `VTIMEZONE` before a same-named IANA zone, rejects floating times and unknown zones, and retains event RRULE text without expanding events. Timezone observance recurrence is still used for transition lookup. DST ambiguity and nonexistent local-time handling follow libical's timezone conversion behavior. The bundled libical timezone database is tzdata 2026c. Exact dependency versions, source hashes, and licenses are recorded in `distribution/native-components.lock.json`.

## Stable compiler and runtime error codes

Codes below are collected from compiler/runtime source and self-hosted language files. Host error codes are also documented beside their operations in the host API contract.

```text
E_ARTIFACT_BOOL
E_ARTIFACT_FLOAT
E_ARTIFACT_INT
E_ARTIFACT_JSON
E_ARTIFACT_LIMIT
E_ARTIFACT_MAGIC
E_ARTIFACT_MAP
E_ARTIFACT_METADATA
E_ARTIFACT_METADATA_SCHEMA
E_ARTIFACT_NESTING
E_ARTIFACT_OPCODE
E_ARTIFACT_RESOURCE
E_ARTIFACT_SIGNATURE
E_ARTIFACT_SIZE
E_ARTIFACT_TRAILING
E_ARTIFACT_TRUNCATED
E_ARTIFACT_TYPE
E_ARTIFACT_UTF8
E_ARTIFACT_VALUE
E_ARTIFACT_VERSION
E_ASSIGN_IMMUTABLE
E_BLOCK_EMPTY
E_BLOCK_UNCLOSED
E_BUILD_TYPE
E_BUILTIN_TZDATA
E_BUSY_DEADLINE_MS
E_BYTES
E_CACHE
E_CALL_ARITY
E_CALL_KEYWORD
E_CLI_DUPLICATE
E_CLI_REQUIRED
E_CLI_SCHEMA
E_CLI_UNKNOWN
E_CLI_VALUE
E_COMPILE_ARGUMENTS
E_COUNT
E_DATA_VERSION
E_DATE_FORMAT
E_DATE_RANGE
E_DEPENDENCY_INSTALL_FAILED
E_DEPENDENCY_RESOLUTION_FAILED
E_DEPTH
E_DIR
E_DIRECTORY
E_DISABLE_FIND_PACKAGE_ICU
E_ENTRY_MISSING
E_ENTRY_SIGNATURE
E_EXCL
E_EXTERN_LANGUAGE
E_EXTERN_PUBLIC
E_EXTERN_TARGET
E_EXTERN_TYPE
E_FIELD_UNKNOWN
E_FILE
E_FLOAT_LITERAL
E_FLOAT_PARSE
E_FS_WALK
E_FS_WALK_LIMIT
E_FS_WALK_ROOT
E_FS_WALK_UTF8
E_HISTORY
E_ICAL_DATE
E_ICAL_EVENT
E_ICAL_FLOATING
E_ICAL_LIMIT
E_ICAL_PARSE
E_ICAL_RANGE
E_ICAL_TZID
E_ICAL_UID
E_ICAL_UTF8
E_IMPORT_DUPLICATE
E_IMPORT_ESCAPE
E_IMPORT_PATH
E_IMPORT_PROJECT_REQUIRED
E_IMPORT_READ
E_IMPORT_ROOT
E_INDENT_TAB
E_INDENT_WIDTH
E_INLINE_LIST_ITEMS
E_INSTALL_LIBDIR
E_INSTALL_PREFIX
E_INT_LITERAL
E_INT_PARSE
E_JSON_INDEX
E_JSON_MISSING
E_JSON_SHAPE
E_JSON_TYPE
E_LIST_BOOL_KIND
E_LIST_FLOAT_KIND
E_LIST_INT_KIND
E_MAGIC
E_MAIN_EXIT_CODE
E_MAIN_RETURN
E_MANAGED_PYTHON_UNAVAILABLE
E_MAP_DUPLICATE_KEY
E_MATCH_DUPLICATE
E_MATCH_EMPTY
E_MATCH_NONEXHAUSTIVE
E_MATCH_PATTERN
E_MATCH_TYPE
E_MATCH_UNCLOSED
E_MATCH_VARIANT
E_MEMORY_RESULTS
E_MODULE_COUNT
E_MODULE_CYCLE
E_MODULE_DEPTH
E_MODULE_EMPTY
E_MODULE_SIZE
E_MS
E_NAME_AMBIGUOUS
E_NAME_DUPLICATE
E_NAME_UNKNOWN
E_NOREPLACE
E_NORMALIZATION_FORM
E_NUMBER_KEY
E_OBJECT_KEYS
E_OPEN_CREATE
E_OPEN_READ_WRITE
E_OPTION_CONSTRUCTOR_CONTEXT
E_OPTION_CONSTRUCTOR_TYPE
E_OWNER
E_PACKAGE_ARGUMENT
E_PACKAGE_FAILED
E_PACKAGE_LOCK
E_PACKAGE_NOT_LOCKED
E_PACKAGE_OUTPUT_EXISTS
E_PACKAGE_PROJECT
E_PACKAGE_VERIFY_FAILED
E_PARALLEL_RESOURCE
E_PARALLEL_WORKER
E_PRINT_VALUE
E_PROFILE
E_PROJECT_ENTRY
E_PROJECT_NOT_FOUND
E_PROJECT_ROOT
E_PYTHON_ARGUMENT_ORDER
E_PYTHON_BOOL
E_PYTHON_CLASS
E_PYTHON_COMPREHENSION
E_PYTHON_DEF
E_PYTHON_ELIF
E_PYTHON_FSTRING
E_PYTHON_IMPORT
E_PYTHON_KEYWORD
E_PYTHON_KEYWORD_DUPLICATE
E_PYTHON_RANGE
E_PYTHON_TRY
E_RECORD_EMPTY
E_RECORD_FIELD_DUPLICATE
E_RECORD_NAME
E_RECORD_UNCLOSED
E_RESOURCE_CLOSED
E_RESOURCE_LIMIT
E_RESOURCE_OWNER
E_RESOURCE_STALE
E_RESULT_CONSTRUCTOR_CONTEXT
E_RESULT_CONSTRUCTOR_TYPE
E_RESULT_TRY_ERROR
E_RESULT_TRY_FUNCTION
E_RESULT_TRY_TYPE
E_RETURN_MISSING
E_ROOT
E_SCHEMA_VERSION
E_SELF
E_SEMVER_BUILD
E_SEMVER_CORE
E_SEMVER_IDENTIFIER
E_SEMVER_LIMIT
E_SEMVER_PRERELEASE
E_SORT_COMPARATOR
E_SORT_EFFECT
E_SOURCE_BYTES
E_SOURCE_IDENTITY
E_SOURCE_READ
E_SQLITE_BEGIN
E_SQLITE_CANCELLED
E_SQLITE_CAPABILITY
E_SQLITE_COMMIT
E_SQLITE_EXECUTE
E_SQLITE_NESTED_TRANSACTION
E_SQLITE_OPEN
E_SQLITE_PATH
E_SQLITE_POLICY
E_SQLITE_QUERY
E_SQLITE_RESULT_LIMIT
E_SQLITE_ROLLBACK
E_SQLITE_STALE
E_SQLITE_SYNTAX
E_SQLITE_UTF8
E_SQLITE_VALUE
E_STATEMENT
E_STMT_EXPR
E_STRING_ESCAPE
E_STRING_UNCLOSED
E_SYNC_ARGUMENT
E_SYNC_CONFIGURATION
E_SYNC_FAILED
E_SYNC_LOCK_INVALID
E_SYNC_PROJECT
E_SYNTAX
E_TEMP_PREFIX
E_TEST_SIGNATURE
E_TEXT_DELIMITER
E_TIMESTAMP_FORMAT
E_TIMESTAMP_OFFSET
E_TIMESTAMP_PRECISION
E_TIMESTAMP_RANGE
E_TIMINGS
E_TOML_EDIT
E_TOML_INTEGER_RANGE
E_TOML_KEY
E_TOML_LIMIT
E_TOML_PARSE
E_TOML_TYPE
E_TYPE_ANNOTATION
E_TYPE_ARGUMENT
E_TYPE_ASSIGN
E_TYPE_CONDITION
E_TYPE_FIELD
E_TYPE_INDEX
E_TYPE_ITERATION
E_TYPE_LIST_EMPTY
E_TYPE_LIST_ITEM
E_TYPE_MAP_EMPTY
E_TYPE_MAP_KEY
E_TYPE_MAP_VALUE
E_TYPE_OPERATOR
E_TYPE_RETURN
E_TYPE_UNKNOWN
E_UV_NOT_FOUND
E_UV_UNAVAILABLE
E_UV_VERSION_MISMATCH
R_ALLOCATION_BYTES
R_ALLOCATION_LIMIT
R_ARGUMENT_INDEX
R_ARTIFACT_INVALID
R_ARTIFACT_VERSION
R_ASSERTION
R_BUILD_ID
R_BYTES
R_BYTE_RANGE
R_CALL_ARITY
R_CALL_DEPTH
R_CALL_TYPE
R_CANCELLED
R_CLOCK_DENIED
R_CLOCK_RANGE
R_CLOCK_UNAVAILABLE
R_COLLECTION_LIMIT
R_CSV_MALFORMED
R_DIV_ZERO
R_ENV_DENIED
R_ENV_ENCODING
R_EXPORT_UNKNOWN
R_FILE_LIMIT
R_FILE_NOT_FOUND
R_FILE_PERMISSION
R_FILE_READ
R_FILE_UTF8
R_FILE_WRITE
R_FLOAT_NONFINITE
R_HOST_DENIED
R_HOST_UNSUPPORTED
R_HTTP_CANCELLED
R_HTTP_CLIENT
R_HTTP_ENCODING
R_HTTP_HEADER
R_HTTP_LIMIT
R_HTTP_METHOD
R_HTTP_REDIRECT
R_HTTP_REQUEST
R_HTTP_STATUS
R_HTTP_TIMEOUT
R_HTTP_URL
R_INDEX_BOUNDS
R_INDEX_INVALID
R_INSTANCE_CLOSED
R_INTEGER_LIMIT
R_JSON_DEPTH
R_JSON_DUPLICATE_KEY
R_JSON_ENCODE
R_JSON_FIELD
R_JSON_INDEX
R_JSON_KEY
R_JSON_PARSE
R_JSON_TYPE
R_LIST_INDEX
R_LOCAL_UNINITIALIZED
R_MAP_DUPLICATE_KEY
R_MAP_KEY_MISSING
R_MATH_DOMAIN
R_NATIVE_INVALID
R_NATIVE_RESULT
R_NETWORK_DENIED
R_NO_RETURN
R_OPTION_UNWRAP
R_OUTPUT_LIMIT
R_PARALLEL_LIMIT
R_PYTHON_ARGUMENT
R_PYTHON_CALL
R_PYTHON_CLOSED
R_PYTHON_CONVERSION
R_PYTHON_CRASH
R_PYTHON_FRAME
R_PYTHON_PROTOCOL
R_PYTHON_RETURN_TYPE
R_PYTHON_START
R_PYTHON_TIMEOUT
R_PYTHON_UNAVAILABLE
R_PYTHON_UNCONFIGURED
R_RESOURCE_CLOSED
R_RESOURCE_MODE
R_RESOURCE_OWNER
R_RESOURCE_STALE
R_RESULT_ERROR
R_RESULT_UNWRAP
R_RETURN_INVALID
R_SQLITE_VALUE
R_STACK_INVALID
R_STEPS
R_STEP_LIMIT
R_TEXT_LIMIT
R_TIME_RANGE
R_TOML_RENDER
R_TYPE_INVALID
R_WORKER_CANCELLED
R_WORKER_PANIC
```

## Runnable recipes

### JSON file CLI

Command: `cargo run --locked -p guion-cli -- run docs/agents/examples/file_data_cli.gu records.json`

```guion
fn main() -> Int:
    let arguments = args()
    if len(arguments) != 1:
        print("usage: file_data_cli.gu JSON_FILE")
        return 2

    let source = read_text(arguments[0])
    if not is_ok(source):
        print(file_error_code(error(source)))
        return 1

    let parsed = json_parse(unwrap(source))
    if not is_ok(parsed):
        print(json_error_code(error(parsed)))
        return 1

    let rows = unwrap(parsed)
    print(json_len(rows))
    return 0
```

### Granted HTTP request

Command: `cargo run --locked -p guion-cli -- run --allow-http-host api.example.org docs/agents/examples/http_request.gu https://api.example.org/health`

```guion
import "std:types"

fn get_response(url: Text) -> Result[HttpResponse, HttpError]:
    let headers: Map[Text, Text] = {}
    return http_request_bytes("GET", url, headers, bytes_from_text(""), 1000)

fn main() -> Int:
    let arguments = args()
    if len(arguments) != 1:
        print("usage: http_request.gu URL")
        return 2

    match get_response(arguments[0]):
        Ok(response):
            print(response.status)
            print(len(response.body))
            return 0
        Err(problem):
            eprint(http_error_code(problem))
            eprint(http_error_message(problem))
            return 1
```

### TOML and SQLite

Command: `cargo run --locked -p guion-cli -- run --file-root /tmp/guion-agent-data docs/agents/examples/sqlite_toml.gu`

```guion
import "std:fs"
import "std:sqlite"
import "std:toml"
import "std:types"

fn run_recipe() -> Result[Text, StdError]:
    let settings_text = fs_read_text("settings.toml")?

    let settings = toml_parse(settings_text)?
    let app_name = toml_get_text(settings,["app", "name"])?

    let database = sqlite_open("agent-recipes.sqlite3")?
    let transaction = sqlite_begin(database)?
    let created = sqlite_execute(transaction, "CREATE TABLE IF NOT EXISTS runs (name TEXT NOT NULL)",[])?
    if created != 0:
        return Err(std_error("E_SQLITE_CREATE", "table creation unexpectedly changed rows"))
    let inserted = sqlite_execute(transaction, "INSERT INTO runs (name) VALUES (?)",[sql_text(app_name)])?
    if inserted != 1:
        return Err(std_error("E_SQLITE_INSERT", "expected one settings row to be inserted"))
    let rows = sqlite_query(transaction, "SELECT name FROM runs ORDER BY rowid DESC LIMIT 1",[])?
    if len(rows) == 0:
        return Err(std_error("E_SQLITE_EMPTY", "query returned no rows"))
    let row = rows[0]
    if len(row.values) == 0:
        return Err(std_error("E_SQLITE_EMPTY", "query returned a row with no values"))
    let name = row.values[0].text

    let committed = sqlite_commit(transaction)
    if not is_ok(committed):
        return Err(error(committed))
    let closed = sqlite_close(database)
    if not is_ok(closed):
        return Err(error(closed))
    return Ok(name)

fn main() -> Int:
    match run_recipe():
        Ok(name):
            print(name)
            return 0
        Err(problem):
            eprint(problem.code)
            eprint(problem.message)
            return 1
```

### Bounded concurrent work

Command: `cargo run --locked -p guion-cli -- run docs/agents/examples/concurrent_task.gu`

```guion
fn square(value: Int) -> Int:
    return value * value

fn square_all(values: List[Int]) -> Result[List[Int], TaskError]:
    return parallel_map(values, square, 3)

fn main() -> Int:
    let result = square_all([1, 2, 3, 4])
    if is_ok(result):
        print(unwrap(result))
        return 0

    let problem = error(result)
    eprint(task_code(problem))
    eprint(task_message(problem))
    return 1
```

### Failure paths

Command: `See the four invocations listed with the example below.`

Run each mode independently:

```sh
cargo run --locked -p guion-cli -- run docs/agents/examples/failure_paths.gu missing-file
cargo run --locked -p guion-cli -- run docs/agents/examples/failure_paths.gu denied-http
cargo run --locked -p guion-cli -- run docs/agents/examples/failure_paths.gu sqlite-escape
cargo run --locked -p guion-cli -- run docs/agents/examples/failure_paths.gu worker-failure
```

```guion
import "std:sqlite"
import "std:types"

fn fail_on_two(value: Int) -> Int:
    assert(value != 2)
    return value

fn main() -> Int:
    let arguments = args()
    if len(arguments) != 1:
        eprint("usage: failure_paths.gu MODE")
        return 2

    if arguments[0] == "missing-file":
        match read_text("missing-agent-input.json"):
            Ok(_text):
                eprint("expected missing file")
                return 0
            Err(problem):
                print(file_error_code(problem))
                return 1

    if arguments[0] == "denied-http":
        let headers: Map[Text, Text] = {}
        match http_request_bytes("GET", "http://127.0.0.1:9/", headers, bytes_from_text(""), 1000):
            Ok(_response):
                eprint("expected denied network")
                return 0
            Err(problem):
                print(http_error_code(problem))
                return 1

    if arguments[0] == "sqlite-escape":
        match sqlite_open("../outside.sqlite3"):
            Ok(database):
                let closed = sqlite_close(database)
                if not is_ok(closed):
                    let problem: StdError = error(closed)
                    eprint(problem.code)
                    eprint(problem.message)
                eprint("expected rejected SQLite path")
                return 0
            Err(problem):
                print(problem.code)
                return 1

    if arguments[0] == "worker-failure":
        match parallel_map([1, 2, 3], fail_on_two, 2):
            Ok(_values):
                eprint("expected worker failure")
                return 0
            Err(problem):
                print(task_index(problem))
                print(task_code(problem))
                print(task_message(problem))
                return 1

    eprint("unknown failure mode")
    return 2
```

## Authoritative sources

- [Language contract](../../language/spec.md)
- [Builtin signature catalog](../../language/builtin-signatures.json)
- [Standard library](../../language/stdlib/)
- [Host API and effects](../../language/host-api.md)
- [Agent recipes](recipes.md)

<!-- Generated reference content digest: sha256:440a2f01b591f0b758ad35bba44438f2721d91c6c5a1fd647b95eb73c818709b. Validated by `cargo xtask agent-reference --check`. -->
