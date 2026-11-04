# Guion reference for agents

Generated from the current Guion language contract, builtin signature catalog, standard library, host API, diagnostics, and runnable examples. Use this file as a compact entry point; the linked contracts remain authoritative.

## Syntax summary

## Source and declarations

- Source is UTF-8. Newlines separate statements. Four spaces define one indentation level; tabs and mixed indentation are errors. `#` begins a line comment.
- A source file is a module. It contains one or more `fn name(parameter: Type, ...) -> Type:` declarations and may declare records with `record Name:` followed by indented `field: Type` declarations. `import "relative-path.gu"` loads another source file; `pub import "relative-path.gu"` also re-exports that module's public declarations to downstream importers. The imported module's `pub fn` and `pub record` declarations become available by their declared names. Private declarations remain local to their module. Imports are resolved from the importing file and must stay inside the project root. The graph is limited to 512 files, 128 import levels, 8 MiB per source file, and 64 MiB total source text. Duplicate declaration names and cycles are errors. Types in this executable subset are `Int`, `Float64`, `Bool`, `Text`, `Bytes`, `Unit`, `Json`, `JsonError`, `FileError`, `HttpError`, `TaskError`, `List[T]`, `Map[Text, T]`, `Option[T]`, `Result[T, E]`, and declared record names.
- The entry point is `fn main() -> Int:` or `fn main() -> Unit:` in the project entry module. Calls resolve statically to local declarations, explicitly imported public declarations, or host functions such as `print(value)`. A single source file can be compiled without a project only when it has no imports.
- Function parameters and results are annotated. Local types are inferred unless an optional local annotation is supplied, as in `let pending: List[Int] = []`. `let` creates an immutable local; `var` creates a mutable local; `set name = expression` changes only a mutable local. An annotation supplies context for empty collection literals and must exactly match the initializer type.

## Statements and expressions

Statements: `let`, `var`, `set`, `return`, `if`/`else`, `while`, `for name in list`, exhaustive `match` statements for `Option` and `Result`, and a call expression as a statement. `if` and `while` conditions must be `Bool`. A function body is indented under its declaration. `else` aligns with `if`.

- Expressions: arbitrary-precision decimal integer literals, finite `Float64` literals with a decimal point or exponent, booleans (`true`, `false`), double-quoted UTF-8 text with `\u{H...}` Unicode scalar escapes (one to six hexadecimal digits; surrogate values and values above U+10FFFF are invalid), local/function names, calls, record construction/access, list literals, map literals with unique text keys, list/map/text indexing, parentheses, unary `-` and `not`, and binary `*`, `/`, `//`, `%`, `+`, `-`, comparisons `== != < <= > >=`, and short-circuit `and` / `or`. `Float64` is IEEE 754 binary64; its arithmetic and comparisons require two `Float64` values, reject non-finite literals/results, and never convert `Int` implicitly. `sqrt(Float64)` returns the non-negative square root; a negative input raises `R_MATH_DOMAIN`. Float division by zero raises `R_DIV_ZERO`. Integer `/` truncates toward zero; `//` rounds down; `%` follows the divisor's sign, so `a == (a // b) * b + (a % b)`. All three integer division operators raise `R_DIV_ZERO` on a zero divisor. Map literals infer one homogeneous value type; an empty map needs a `Map[Text, T]` annotation or other expected type, and keys must be text literals. Empty list literals likewise need an expected `List[T]` type. `list_push(List[T], T)` appends and `list_set(List[T], Int, T)` replaces an existing zero-based index; both return immutable copies, and the compiler may use copy-on-write for updates assigned back to the same mutable local while preserving aliases; out-of-range replacement raises `R_LIST_INDEX`. `Map[Text, T]` is immutable, `map_set` returns an updated copy, `map_has` tests membership, keys iterate and display in Unicode scalar lexicographic order, lookup of an absent key raises `R_MAP_KEY_MISSING`, and `len` supports lists, maps, Bytes (byte count), and Text (Unicode scalar count). Text indexing is zero-based by Unicode scalar and out-of-range access is a runtime error. `text_slice(text, start, end)` returns a copied half-open range by Unicode scalar index, with a non-negative start, an exclusive end, and `0 <= start <= end <= len(text)`; an invalid range raises `R_INDEX_BOUNDS`. Types include `List[T]`, `Map[Text, T]`, `Option[T]`, and `Result[T, E]`; `Some(value)` and `None` are Option constructors in Option-returning functions. Precedence from low to high: `or`, `and`, comparisons, `+ -`, `* / // %`, unary, calls/indexing/field access/atoms. `+` accepts two Ints, two Float64 values, or two Text values.

## Runtime rules

### Typed map-record access and updates

`map_record_get(map, key, "field")` reads a statically checked field from a `Map[Text, Record]`; `map_record_set(map, key, "field", value)` returns an updated map after replacing that field. For a nested `Map[Text, Map[Text, Record]]`, pass both keys before the field name: `map_record_get(map, outer_key, inner_key, "field")` and `map_record_set(map, outer_key, inner_key, "field", value)`. The field name must be a text literal, and the compiler checks the map's record type, field existence, and value type. Missing keys fail with `R_MAP_KEY`. These expressions preserve map and record value semantics: assigning an update back to the same mutable local may reuse uniquely owned storage; any shared outer map, nested map, or record fields are copied as needed so aliases keep their previous values.

- `Int` is arbitrary precision subject to the configured digit ceiling. Integer `/` truncates toward zero, `//` rounds down, and `%` follows the divisor sign so `a == (a // b) * b + (a % b)`; all report `R_DIV_ZERO` for a zero divisor. `Float64` stores finite IEEE 754 binary64 values. Operations that overflow to infinity or NaN fail with `R_FLOAT_NONFINITE`; division by either signed zero fails with `R_DIV_ZERO`; `sqrt` of a negative value fails with `R_MATH_DOMAIN`. Python interop transports Float64 through round-trippable decimal strings and rejects non-finite values. Lists are homogeneous, immutable value sequences; indexing is zero-based and out-of-range access is a runtime error. `list_push` and `list_set` return immutable lists, and `list_set` requires an existing index. When `set name = list_push(name, value)` or `set name = list_set(name, index, value)` updates a mutable local, the compiler may reuse uniquely owned storage and copies if any alias exists; this is unobservable and preserves immutable value behavior. Maps are immutable `Map[Text, T]` values with unique literal keys, homogeneous values, deterministic key iteration, immutable `map_set` updates, membership checks via `map_has`, and traced errors for missing keys. Map values may be passed between functions, serialized in versioned artifacts, and crossed through the Python bridge as string-keyed dictionaries when `T` is bridge-supported. Empty lists and maps use their expected type from annotations, function results or parameters, enclosing collections, and typed operations; uncontextualized empty literals produce a diagnostic. `Json` is a bounded JSON tree; object keys are text, duplicate keys are rejected, and object encoding sorts keys. JSON values require explicit accessors and do not silently coerce to source types. Records are immutable ordered named fields, constructed positionally (for example, `Point(3, 4)`) and accessed by field name (`point.x`). Construction requires every field with its exact declared type; record and field names are unique; equality is structural for the same record type. Unknown fields/types and malformed schemas are rejected. Records serialize in compiled artifacts, but are not supported through the Python bridge yet. First-class functions remain for subsequent implementation steps. `Option[T]` supports `Some(value)` and `None` inside functions declared to return `Option[T]`; `is_some(option)` tests presence and `unwrap(option)` returns the value or raises `R_OPTION_UNWRAP`. `Result[T, E]` is available for file operations, JSON parsing, and `parallel_map`; inspect results with `is_ok`, `unwrap`, and `error`. `Ok`, `Err`, postfix `?`, and exhaustive `match` statements are available for `Result[T, E]`.
- Integers compare by mathematical value. Text is Unicode scalar sequence data; `+` concatenates text. Equality requires equal runtime types.
- Locals are frame-owned values. `let` cannot be assigned; `var` can. Each invocation owns its locals. Calls have a bounded stack depth, instructions consume a configurable budget, and output is captured per instance. A shared cumulative estimated guest-data allocation budget covers input values, collection and record construction, text/byte/JSON values, host results, and output rendering across nested calls and parallel workers; exhaustion reports `R_ALLOCATION_LIMIT`. The default is 256 MiB. Interpreter/JIT frames, operand stacks, and scalar arithmetic temporaries are excluded so changing execution backend does not change this limit. Integer growth is bounded separately by `max_integer_digits`. This is language-level estimate accounting, not an exact process-memory ceiling; limits for output, file bytes, text size, collection size, integer digits, and source/artifact size are separate.
- `print(value)` writes its display form plus one newline to the instance output buffer. `args()` returns all CLI arguments as `List[Text]`; `args_from(start)` returns the suffix beginning at a zero-based index and reports `R_ARGUMENT_INDEX` for a negative or past-end index. `env(name) -> Option[Text]` reads one environment variable only when its exact name is granted in `[environment].variables` in `guion.toml` (or by an embedding `HostPolicy`); an ungranted name raises `R_ENV_DENIED`, and an unset granted name returns `None`. `read_text(path) -> Result[Text, FileError]`, `write_text(path, content) -> Result[Unit, FileError]`, `read_bytes(path) -> Result[Bytes, FileError]`, and `write_bytes(path, content) -> Result[Unit, FileError]` are host operations; runtime embedders grant a project root and read/write rights in `HostPolicy`. The CLI grants the source or artifact directory; `guion run --file-root DIRECTORY` adds a further root. Reads search roots in grant order; writes use the last granted root. Relative paths only are accepted, parent traversal is rejected, and canonicalized symlink targets must remain under one granted root. Access/path/filesystem failures return `FileError`; configured size-limit failures remain runtime errors. This is a path policy, not an OS security sandbox. Bytes preserve arbitrary byte sequences and are limited by `max_file_bytes`.
- `http_get(url) -> Result[Text, HttpError]` performs a bounded GET for HTTP or HTTPS text. Network is denied by default. Embedders explicitly grant exact hostnames through `HostPolicy`; CLI projects list them under `[network].hosts` in `guion.toml`. A redirect is followed only when its destination hostname is also granted. Timeouts, response byte ceilings, and redirect counts are configurable; non-2xx statuses and transport/encoding failures return `HttpError`, inspectable through `http_error_code` and `http_error_message`. Host allowlists do not prevent DNS rebinding or provide OS-level egress confinement.
- `http_request(method, url, headers, body) -> Result[Json, HttpError]` supports GET, POST, PUT, PATCH, DELETE, HEAD, and OPTIONS with `Map[Text]` headers and a UTF-8 text body. Its JSON envelope contains integer `status`, text `body`, and lowercase response headers. `http_request_bytes(method, url, headers, body, timeout_ms) -> Result[HttpResponse, HttpError]` preserves arbitrary request and response bytes; `HttpResponse` has `status: Int`, `headers: Map[Text, Text]`, and `body: Bytes`. Importing `std:http` provides the response type and typed JSON helper; `std:types` re-exports the same type for compatibility. Non-2xx responses remain values. `http_response_text(response) -> Result[Text, HttpError]` checks the body's UTF-8 encoding and text-size limit; `std:http.http_response_json(response) -> Result[Json, StdError]` performs the same checks and parses JSON directly from the response bytes, without an intermediate `Text` allocation. It preserves the stable HTTP and JSON error codes and messages. A shared host client enforces one monotonic deadline through connection and body reads, bounds response accumulation, refuses cross-origin redirects, and interrupts in-flight reads on cancellation. Runtime URL hosts require exact grants in `[network].hosts` or `guion run --allow-http-host HOST`; network remains denied by default.
- `unix_time_ms() -> Int` returns wall-clock milliseconds since the Unix epoch, and `today_local() -> Date` returns the current date in the host process time zone. `format_utc_date(instant) -> Text` formats an `Instant` as its `YYYY-MM-DD` UTC calendar date without formatting the time of day. Clock access is denied unless `[clock].enabled = true` or the caller uses `guion run --allow-clock` / an embedding `HostPolicy` grant. The host supplies local-time and daylight-saving rules; tests and embedders can pin the clock through `HostPolicy::with_clock_unix_time_ms`.
- `parallel_map(items, worker, limit)` calls a named unary function on immutable list items in worker-local VM instances and returns `Result[List[U], TaskError]`. `is_ok(result)` tests the variant; `unwrap(result)` returns the list from `Ok`; `error(result)` returns the `TaskError` from `Err`. `task_index`, `task_code`, and `task_message` read its failed input index and failure details. Workers preserve result order, use a bounded shared worker instruction allowance, stop scheduling and poll cancellation after failure, join running tasks, and merge completed output by input index. An instruction-limit failure inside a worker is a `TaskError`; host setup errors still fail the invocation.
- Numeric conversion and math (artifact format 52): `int_to_float(Int) -> Float64`, `float_to_int(Float64) -> Int` (truncates toward zero), `int_to_text(Int) -> Text`, and `float_to_text(Float64) -> Text` (the same shortest round-trip text `print` shows). `std:math` provides `math_sin`, `math_cos`, `math_tan`, `math_asin`, `math_acos`, `math_atan`, `math_atan2`, `math_sinh`, `math_cosh`, `math_tanh`, `math_exp`, `math_ln`, `math_log10`, `math_log2`, `math_log`, `math_pow`, `math_hypot`, `math_abs`, `math_floor`, `math_ceil`, `math_round` (half away from zero), `math_floor_int`, `math_ceil_int`, `math_round_int`, `math_radians`, `math_degrees`, and the constants `math_pi()`, `math_tau()`, `math_e()`. A domain error raises `R_MATH_DOMAIN`, zero to a negative power raises `R_DIV_ZERO`, and a non-finite result raises `R_FLOAT_NONFINITE`. Results use the platform's IEEE 754 libm, so they can differ in the last digit between targets. `std:random` is a deterministic SplitMix64 generator over immutable `Rng` values: `random_new(seed)`, `random_next_u64`, `random_next_float` (in `[0, 1)`), and `random_next_range(rng, low, high)` (uniform over `low <= value < high`, by rejection sampling); each returns the value together with the next `Rng`. The same seed gives the same sequence on every target. It reads no clock; seed it from `unix_time_ms()` when a changing sequence is wanted.
- Bytes, encodings and URLs (artifact format 53): `bytes_to_ints(Bytes) -> List[Int]`, `bytes_slice(Bytes, start, end) -> Bytes` (raises `R_INDEX_BOUNDS` outside `0 <= start <= end <= len`), `text_from_bytes(Bytes) -> Option[Text]` (strict UTF-8) and `text_from_bytes_lossy(Bytes) -> Text`. `std:encoding` provides `base64_encode`/`base64_decode` and `base64url_encode`/`base64url_decode` (padded; decoding is strict about alphabet, length and padding and returns `E_BASE64_INVALID`), `hex_encode`/`hex_decode` (lowercase output; either case accepted; `E_HEX_INVALID`), and `encoding_utf8_encode`, `encoding_utf8_decode` (`E_UTF8_INVALID`) and `encoding_utf8_decode_lossy`. `std:url` provides `url_parse(Text) -> Result[Url, StdError]` (an RFC 3986 split into `scheme`, `userinfo`, `host`, `port` (`-1` when absent), `path`, `query` and `fragment`, with no normalization; bracketed IPv6 hosts; `E_URL_INVALID`), `url_to_text`, `url_encode_component` (everything except `A-Z a-z 0-9 - . _ ~` is percent-encoded as UTF-8), `url_decode_component`, `url_query_parse` (`+` decodes to a space) and `url_query_build`.
- Filesystem and paths (artifact format 54): `std:fs` adds `fs_exists`, `fs_kind` (`"file"`, `"directory"`, `"other"` or `"missing"`), `fs_size`, `fs_modified_ms`, `fs_list_dir` (sorted names), `fs_make_dir` (creates missing parents; succeeds if the directory exists), `fs_remove` (a file or an empty directory), `fs_remove_all`, `fs_rename`, `fs_copy` (returns the bytes copied) and `fs_temp_dir`/`fs_temp_file` (a unique entry under `.guion-tmp/` in the granted root; the prefix is 1 to 64 ASCII letters, digits, `-` or `_`). All return `Result[T, FileError]`. Each path is relative to a granted file root, may not contain `..`, and is confined after symlink resolution, as for `read_text`; mutating operations need the file-write grant. Symlinks are followed, so removing or renaming a link acts on its confined target. The granted root itself cannot be removed. `std:path` provides POSIX path text functions that never touch the disk: `path_normalize`, `path_join`, `path_dirname`, `path_basename`, `path_extension`, `path_stem`, `path_with_extension`, `path_components` and `path_is_absolute`. Static inspection now reports `filesystem.read` for `fs_walk`, `file_open_lines` and CSV opening as well.
- Regular expressions (artifact format 55): `std:regex` provides `regex_is_match`, `regex_find` (the first match as a list of capture groups, group 0 first, unmatched groups as `""`), `regex_find_all`, `regex_replace`, `regex_replace_first` (replacement text expands `$1`, `${name}`; `$$` is a literal `$`), `regex_split` and `regex_escape`. Patterns use the Rust `regex` crate syntax: Unicode-aware, RE2-style, matching time linear in the subject, with no backreferences and no lookaround. An invalid or unsupported pattern, a pattern over 4,096 bytes, or one that compiles to more than 1 MiB of program returns `Err` with code `E_REGEX_INVALID`; no grant is required. Compiled patterns are cached per thread.
- Processes and signals (artifact format 56): `std:process` provides `process_run(command, arguments, stdin)` returning `Result[ProcessResult, StdError]` with `exit_code`, `stdout` and `stderr` (a child killed by signal N reports `128 + N`; output is decoded as lossy UTF-8), `process_pid()` and `process_signal_poll()`. Starting a program is denied by default: the command name must be listed exactly in `guion.toml` `[process].commands` or passed with `guion run --allow-command NAME`; a grant for one command never covers another. The command starts without a shell from an argument list, with a cleared environment holding only `PATH` and the variables granted in `[environment]`, the project file root as working directory, and its own process group. `[process].timeout_ms` (default 30,000) and `[process].max_output_bytes` (default 16 MiB per stream) bound each call; exceeding either kills the whole group and returns `E_PROCESS_TIMEOUT` or `E_PROCESS_OUTPUT_LIMIT`. Other error codes are `E_PROCESS_DENIED`, `E_PROCESS_ARGUMENT` and `E_PROCESS_SPAWN`. `process_signal_poll()` installs `INT`, `TERM` and `HUP` handlers on first use, so those signals no longer end the program by themselves; it returns and clears the pending signal name, or `""`. Calls are synchronous: there is no way to leave a child running after the call returns. Static inspection reports `process.spawn` and `process.signals`, with a `process_grant` section.
- Sockets (artifact format 57): `std:net` provides blocking TCP and UDP over integer-handle records `TcpConn`, `TcpListener`, `UdpSock`: `tcp_connect`, `tcp_listen`, `tcp_listener_port`, `tcp_accept`, `tcp_send`, `tcp_send_text`, `tcp_recv`, `tcp_close`, `tcp_listener_close`, `udp_bind`, `udp_port`, `udp_send_to`, `udp_recv_from` (a `Datagram` with `data`, `host`, `port`) and `udp_close`. Everything is denied by default. Connecting and sending datagrams need an exact `host:port` (or `host:*`) entry in `guion.toml` `[sockets].connect` or `guion run --allow-connect`; listening and binding need one in `[sockets].bind` or `--allow-bind` (`127.0.0.1:0` allows an ephemeral port on loopback). The host name is matched as written, before resolution, so grant IP literals when the name could resolve elsewhere. Every connect, accept, receive and datagram receive takes a timeout of 1 ms up to `[sockets].timeout_ms` (default 30,000); reads are capped at `[sockets].max_read_bytes` (default 1 MiB) and at most 64 sockets may be open. A zero-length receive on TCP means the peer closed. Error codes: `E_NET_DENIED`, `E_NET_ARGUMENT`, `E_NET_RESOLVE`, `E_NET_REFUSED`, `E_NET_TIMEOUT`, `E_NET_IO`, `E_NET_HANDLE`, `E_NET_LIMIT`. Calls block the calling task until they finish or time out; a cancelled sibling task is noticed at the next call boundary, so cancellation latency is bounded by the timeout. Static inspection reports `network.socket` with a `socket_grant` section. TLS is not part of this module.
- TLS (artifact format 58): `std:net` adds `tls_connect(host, port, timeout_ms, extra_ca_pem)` and `tls_accept(listener, timeout_ms, certificate_chain_pem, private_key_pem)`, which return a `TcpConn` that `tcp_send`, `tcp_send_text`, `tcp_recv`, `tcp_close` and the port functions accept. TLS 1.2 and 1.3 only, through rustls with the ring provider. The client always verifies the server certificate and name against the bundled Mozilla roots plus any PEM certificates passed as `extra_ca_pem`; there is no option to skip verification. A rejected certificate returns `E_TLS_CERTIFICATE`, other handshake failures `E_TLS_HANDSHAKE`, bad PEM or a key that does not match `E_TLS_ARGUMENT`; timeouts and grants behave as for TCP (`E_NET_TIMEOUT`, `E_NET_DENIED`). A peer that closes without a TLS close notification surfaces as `E_NET_IO` on receive. Client certificates, session resumption control and ALPN are not offered. Certificates and keys are PEM text, normally read with `read_text` under the file grant.
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
bytes_slice(Bytes, Int, Int) -> Bytes
bytes_to_ints(Bytes) -> List[Int]
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
float_to_int(Float64) -> Int
float_to_text(Float64) -> Text
fs_walk(Text) -> Result[List[Record[DirectoryEntry]], FileError]
http_error_code(HttpError) -> Text
http_error_message(HttpError) -> Text
http_get(Text) -> Result[Text, HttpError]
http_request(Text, Text, Map[Text, Text], Text) -> Result[Json, HttpError]
http_request_bytes(Text, Text, Map[Text, Text], Bytes, Int) -> Result[Record[HttpResponse], HttpError]
http_request_with_timeout(Text, Text, Map[Text, Text], Text, Int) -> Result[Json, HttpError]
http_response_json_bytes(Record[HttpResponse]) -> Result[Json, Record[StdError]]
http_response_text(Record[HttpResponse]) -> Result[Text, HttpError]
int_to_float(Int) -> Float64
int_to_text(Int) -> Text
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
json_stringify_pairs_native(Map[Text,List[Text]], Text, Text) -> Text
json_text(Text) -> Json
json_text_categories(Json, Text) -> Json
json_text_categories_values(Json) -> Json
json_try_bool(Json) -> Result[Bool, JsonError]
json_try_float(Json) -> Result[Float64, JsonError]
json_try_get(Json, Text) -> Result[Json, JsonError]
json_try_get_int(Json, Text) -> Result[Int, JsonError]
json_try_get_text(Json, Text) -> Result[Text, JsonError]
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
merge_sorted_by_int_key(List[List[$T]], NamedIntKey[$T]) -> List[$T]
native_base64_decode(Text) -> Option[Bytes]
native_base64_encode(Bytes) -> Text
native_base64url_decode(Text) -> Option[Bytes]
native_base64url_encode(Bytes) -> Text
native_calendar_parse(Text) -> Result[List[Record[CalendarEvent]], Record[StdError]]
native_crypto_hmac_sha256(Bytes, Bytes) -> Bytes
native_crypto_verify_hmac_sha256(Bytes, Bytes, Bytes) -> Bool
native_fs_copy(Text, Text) -> Result[Int, FileError]
native_fs_exists(Text) -> Result[Bool, FileError]
native_fs_kind(Text) -> Result[Text, FileError]
native_fs_list_dir(Text) -> Result[List[Text], FileError]
native_fs_make_dir(Text) -> Result[Unit, FileError]
native_fs_modified_ms(Text) -> Result[Int, FileError]
native_fs_remove(Text) -> Result[Unit, FileError]
native_fs_remove_all(Text) -> Result[Unit, FileError]
native_fs_rename(Text, Text) -> Result[Unit, FileError]
native_fs_size(Text) -> Result[Int, FileError]
native_fs_temp_dir(Text) -> Result[Text, FileError]
native_fs_temp_file(Text) -> Result[Text, FileError]
native_hex_decode(Text) -> Option[Bytes]
native_hex_encode(Bytes) -> Text
native_math_abs(Float64) -> Float64
native_math_acos(Float64) -> Float64
native_math_asin(Float64) -> Float64
native_math_atan(Float64) -> Float64
native_math_atan2(Float64, Float64) -> Float64
native_math_ceil(Float64) -> Float64
native_math_cos(Float64) -> Float64
native_math_cosh(Float64) -> Float64
native_math_exp(Float64) -> Float64
native_math_floor(Float64) -> Float64
native_math_hypot(Float64, Float64) -> Float64
native_math_ln(Float64) -> Float64
native_math_log10(Float64) -> Float64
native_math_log2(Float64) -> Float64
native_math_pow(Float64, Float64) -> Float64
native_math_round(Float64) -> Float64
native_math_sin(Float64) -> Float64
native_math_sinh(Float64) -> Float64
native_math_tan(Float64) -> Float64
native_math_tanh(Float64) -> Float64
native_net_close(Int) -> Result[Unit, List[Text]]
native_net_local_port(Int) -> Result[Int, List[Text]]
native_net_recv(Int, Int, Int) -> Result[Bytes, List[Text]]
native_net_send(Int, Bytes) -> Result[Int, List[Text]]
native_net_tcp_accept(Int, Int) -> Result[Int, List[Text]]
native_net_tcp_connect(Text, Int, Int) -> Result[Int, List[Text]]
native_net_tcp_listen(Text, Int) -> Result[Int, List[Text]]
native_net_udp_bind(Text, Int) -> Result[Int, List[Text]]
native_net_udp_recv_from(Int, Int, Int) -> Result[List[Bytes], List[Text]]
native_net_udp_send_to(Int, Text, Int, Bytes) -> Result[Int, List[Text]]
native_percent_decode(Text) -> Option[Text]
native_percent_encode(Text) -> Text
native_process_pid() -> Int
native_process_run(Text, List[Text], Text) -> Result[List[Text], List[Text]]
native_process_signal_poll() -> Text
native_random_mix(Int) -> Int
native_regex_escape(Text) -> Text
native_regex_find(Text, Text) -> Result[Option[List[Text]], Text]
native_regex_find_all(Text, Text) -> Result[List[Text], Text]
native_regex_is_match(Text, Text) -> Result[Bool, Text]
native_regex_replace(Text, Text, Text) -> Result[Text, Text]
native_regex_replace_first(Text, Text, Text) -> Result[Text, Text]
native_regex_split(Text, Text) -> Result[List[Text], Text]
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
native_tls_accept(Int, Int, Text, Text) -> Result[Int, List[Text]]
native_tls_connect(Text, Int, Int, Text) -> Result[Int, List[Text]]
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
sort_by_int_key(List[$T], NamedIntKey[$T]) -> List[$T]
sqrt(Float64) -> Float64
task_code(TaskError) -> Text
task_index(TaskError) -> Int
task_message(TaskError) -> Text
text_chars(Text) -> List[Text]
text_find(Text, Text, Int) -> Int
text_from_bytes(Bytes) -> Option[Text]
text_from_bytes_lossy(Bytes) -> Text
text_join_native(List[Text], Text) -> Text
text_parse_float(Text) -> Result[Float64, FileError]
text_parse_int(Text) -> Result[Int, FileError]
text_replace_native(Text, Text, Text) -> Text
text_slice(Text, Int, Int) -> Text
text_split_native(Text, Text) -> List[Text]
text_trim_native(Text) -> Text
time_format_utc(Record[Instant]) -> Text
time_format_utc_date(Record[Instant]) -> Text
time_parse_date(Text) -> Result[Record[Date], FileError]
time_parse_timestamp(Text) -> Result[Record[Instant], FileError]
time_resolve_local(Record[LocalDateTime], Text) -> Result[Record[TemporalResolution], Record[TimeError]]
time_today_local() -> Record[Date]
time_zone_data_version_native() -> Text
unix_time_ms() -> Int
unwrap(Result[$T, $E]|Option[$T]) -> $T
write_bytes(Text, Bytes) -> Result[Unit, FileError]
write_text(Text, Text) -> Result[Unit, FileError]
write_text_atomic(Text, Text) -> Result[Unit, FileError]
```

`sort_by_int_key` requires a named pure `(T) -> Int` function, evaluates that key once per item, uses a stable sort, and preserves the input and its aliases. `merge_sorted_by_int_key` stably merges sorted `List[List[T]]` feeds using the same key contract and returns `R_SORT_ORDER` if any feed is not already sorted by key. Native comparisons count against the guest instruction budget.

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
std:encoding — pub fn base64_encode(data: Bytes) -> Text
std:encoding — pub fn base64_decode(text: Text) -> Result[Bytes, StdError]
std:encoding — pub fn base64url_encode(data: Bytes) -> Text
std:encoding — pub fn base64url_decode(text: Text) -> Result[Bytes, StdError]
std:encoding — pub fn hex_encode(data: Bytes) -> Text
std:encoding — pub fn hex_decode(text: Text) -> Result[Bytes, StdError]
std:encoding — pub fn encoding_utf8_decode(data: Bytes) -> Result[Text, StdError]
std:encoding — pub fn encoding_utf8_decode_lossy(data: Bytes) -> Text
std:encoding — pub fn encoding_utf8_encode(text: Text) -> Bytes
std:fs — pub fn fs_read_text(path: Text) -> Result[Text, StdError]
std:fs — pub fn fs_open_lines(path: Text) -> Result[FileReader, FileError]
std:fs — pub fn fs_next_line(reader: FileReader) -> Result[Option[SourceLine], FileError]
std:fs — pub fn fs_close_reader(reader: FileReader) -> Result[Unit, FileError]
std:fs — pub fn fs_exists(path: Text) -> Result[Bool, FileError]
std:fs — pub fn fs_kind(path: Text) -> Result[Text, FileError]
std:fs — pub fn fs_size(path: Text) -> Result[Int, FileError]
std:fs — pub fn fs_modified_ms(path: Text) -> Result[Int, FileError]
std:fs — pub fn fs_list_dir(path: Text) -> Result[List[Text], FileError]
std:fs — pub fn fs_make_dir(path: Text) -> Result[Unit, FileError]
std:fs — pub fn fs_remove(path: Text) -> Result[Unit, FileError]
std:fs — pub fn fs_remove_all(path: Text) -> Result[Unit, FileError]
std:fs — pub fn fs_rename(from: Text, to: Text) -> Result[Unit, FileError]
std:fs — pub fn fs_copy(from: Text, to: Text) -> Result[Int, FileError]
std:fs — pub fn fs_temp_dir(prefix: Text) -> Result[Text, FileError]
std:fs — pub fn fs_temp_file(prefix: Text) -> Result[Text, FileError]
std:http — pub fn http_response_json(response: HttpResponse) -> Result[Json, StdError]
std:json — pub fn json_get_safe(value: Json, key: Text) -> Result[Json, JsonError]
std:json — pub fn json_get_text_safe(value: Json, key: Text) -> Result[Text, JsonError]
std:json — pub fn json_get_int_safe(value: Json, key: Text) -> Result[Int, JsonError]
std:json — pub fn json_index_safe(value: Json, index: Int) -> Result[Json, JsonError]
std:json — pub fn json_text_safe(value: Json) -> Result[Text, JsonError]
std:json — pub fn json_stringify_pairs(values: Map[Text, List[Text]], key_field: Text, value_field: Text) -> Text
std:json — pub fn json_int_safe(value: Json) -> Result[Int, JsonError]
std:json — pub fn json_bool_safe(value: Json) -> Result[Bool, JsonError]
std:json — pub fn json_float_safe(value: Json) -> Result[Float64, JsonError]
std:jsonl — pub fn jsonl_open(path: Text) -> Result[FileReader, FileError]
std:jsonl — pub fn jsonl_next(reader: FileReader) -> Result[Option[JsonlLine], FileError]
std:jsonl — pub fn jsonl_close(reader: FileReader) -> Result[Unit, FileError]
std:math — pub fn math_pi() -> Float64
std:math — pub fn math_tau() -> Float64
std:math — pub fn math_e() -> Float64
std:math — pub fn math_sin(value: Float64) -> Float64
std:math — pub fn math_cos(value: Float64) -> Float64
std:math — pub fn math_tan(value: Float64) -> Float64
std:math — pub fn math_asin(value: Float64) -> Float64
std:math — pub fn math_acos(value: Float64) -> Float64
std:math — pub fn math_atan(value: Float64) -> Float64
std:math — pub fn math_atan2(y: Float64, x: Float64) -> Float64
std:math — pub fn math_sinh(value: Float64) -> Float64
std:math — pub fn math_cosh(value: Float64) -> Float64
std:math — pub fn math_tanh(value: Float64) -> Float64
std:math — pub fn math_exp(value: Float64) -> Float64
std:math — pub fn math_ln(value: Float64) -> Float64
std:math — pub fn math_log10(value: Float64) -> Float64
std:math — pub fn math_log2(value: Float64) -> Float64
std:math — pub fn math_log(value: Float64, base: Float64) -> Float64
std:math — pub fn math_pow(base: Float64, exponent: Float64) -> Float64
std:math — pub fn math_hypot(x: Float64, y: Float64) -> Float64
std:math — pub fn math_abs(value: Float64) -> Float64
std:math — pub fn math_floor(value: Float64) -> Float64
std:math — pub fn math_ceil(value: Float64) -> Float64
std:math — pub fn math_round(value: Float64) -> Float64
std:math — pub fn math_floor_int(value: Float64) -> Int
std:math — pub fn math_ceil_int(value: Float64) -> Int
std:math — pub fn math_round_int(value: Float64) -> Int
std:math — pub fn math_radians(degrees: Float64) -> Float64
std:math — pub fn math_degrees(radians: Float64) -> Float64
std:net — pub fn tcp_connect(host: Text, port: Int, timeout_ms: Int) -> Result[TcpConn, StdError]
std:net — pub fn tcp_listen(host: Text, port: Int) -> Result[TcpListener, StdError]
std:net — pub fn tcp_listener_port(listener: TcpListener) -> Result[Int, StdError]
std:net — pub fn tcp_accept(listener: TcpListener, timeout_ms: Int) -> Result[TcpConn, StdError]
std:net — pub fn tcp_send(conn: TcpConn, data: Bytes) -> Result[Int, StdError]
std:net — pub fn tcp_send_text(conn: TcpConn, text: Text) -> Result[Int, StdError]
std:net — pub fn tcp_recv(conn: TcpConn, max_bytes: Int, timeout_ms: Int) -> Result[Bytes, StdError]
std:net — pub fn tcp_close(conn: TcpConn) -> Result[Unit, StdError]
std:net — pub fn tcp_listener_close(listener: TcpListener) -> Result[Unit, StdError]
std:net — pub fn udp_bind(host: Text, port: Int) -> Result[UdpSock, StdError]
std:net — pub fn udp_port(socket: UdpSock) -> Result[Int, StdError]
std:net — pub fn udp_send_to(socket: UdpSock, host: Text, port: Int, data: Bytes) -> Result[Int, StdError]
std:net — pub fn udp_recv_from(socket: UdpSock, max_bytes: Int, timeout_ms: Int) -> Result[Datagram, StdError]
std:net — pub fn udp_close(socket: UdpSock) -> Result[Unit, StdError]
std:net — pub fn tls_connect(host: Text, port: Int, timeout_ms: Int, extra_ca_pem: Text) -> Result[TcpConn, StdError]
std:net — pub fn tls_accept(listener: TcpListener, timeout_ms: Int, certificate_chain_pem: Text, private_key_pem: Text) -> Result[TcpConn, StdError]
std:path — pub fn path_is_absolute(path: Text) -> Bool
std:path — pub fn path_normalize(path: Text) -> Text
std:path — pub fn path_join(base: Text, child: Text) -> Text
std:path — pub fn path_basename(path: Text) -> Text
std:path — pub fn path_dirname(path: Text) -> Text
std:path — pub fn path_extension(path: Text) -> Text
std:path — pub fn path_stem(path: Text) -> Text
std:path — pub fn path_with_extension(path: Text, extension: Text) -> Text
std:path — pub fn path_components(path: Text) -> List[Text]
std:process — pub fn process_run(command: Text, arguments: List[Text], stdin: Text) -> Result[ProcessResult, StdError]
std:process — pub fn process_pid() -> Int
std:process — pub fn process_signal_poll() -> Text
std:random — pub fn random_new(seed: Int) -> Rng
std:random — pub fn random_next_u64(rng: Rng) -> RandomInt
std:random — pub fn random_next_float(rng: Rng) -> RandomFloat
std:random — pub fn random_next_range(rng: Rng, low: Int, high: Int) -> RandomInt
std:regex — pub fn regex_is_match(pattern: Text, text: Text) -> Result[Bool, StdError]
std:regex — pub fn regex_find(pattern: Text, text: Text) -> Result[Option[List[Text]], StdError]
std:regex — pub fn regex_find_all(pattern: Text, text: Text) -> Result[List[Text], StdError]
std:regex — pub fn regex_replace(pattern: Text, text: Text, replacement: Text) -> Result[Text, StdError]
std:regex — pub fn regex_replace_first(pattern: Text, text: Text, replacement: Text) -> Result[Text, StdError]
std:regex — pub fn regex_split(pattern: Text, text: Text) -> Result[List[Text], StdError]
std:regex — pub fn regex_escape(text: Text) -> Text
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
std:time — pub fn format_utc_date(value: Instant) -> Text
std:time — pub fn resolve_local(local: LocalDateTime, timezone: Text) -> Result[TemporalResolution, TimeError]
std:time — pub fn time_zone_data_version() -> Text
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
std:url — pub fn url_encode_component(value: Text) -> Text
std:url — pub fn url_decode_component(value: Text) -> Result[Text, StdError]
std:url — pub fn url_parse(input: Text) -> Result[Url, StdError]
std:url — pub fn url_to_text(url: Url) -> Text
std:url — pub fn url_query_parse(query: Text) -> Result[List[QueryPair], StdError]
std:url — pub fn url_query_build(pairs: List[QueryPair]) -> Text
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

## Stable compiler and runtime error codes

Codes below are collected from compiler/runtime source and self-hosted language files. Host error codes are also documented beside their operations in the host API contract.

```text
E_ABI_ARTIFACT
E_ABI_ENTRY
E_ABI_LIMIT
E_ABI_POINTER
E_ABI_REQUEST
E_ABI_VALUE
E_ABI_VALUE_UNSUPPORTED
E_ABI_VERSION
E_ARENA_RECLAIM_MIN_ENTRIES
E_ARTIFACT_BOOL
E_ARTIFACT_ENCODING
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
E_BASE64_INVALID
E_BLOCK_EMPTY
E_BLOCK_UNCLOSED
E_BRIDGE_ARGS
E_BRIDGE_FILE_CLOSE
E_BRIDGE_FILE_NEXT_LINE
E_BRIDGE_FILE_OPEN_LINES
E_BRIDGE_GET_FIELD
E_BRIDGE_JSON_ARRAY
E_BRIDGE_JSON_GET
E_BRIDGE_JSON_INDEX
E_BRIDGE_JSON_INT
E_BRIDGE_JSON_OBJECT
E_BRIDGE_JSON_PARSE
E_BRIDGE_JSON_PUSH_LOCAL
E_BRIDGE_JSON_SET_LOCAL
E_BRIDGE_JSON_STRINGIFY
E_BRIDGE_JSON_TEXT
E_BRIDGE_JSON_TRY_GET
E_BRIDGE_JSON_TRY_INT
E_BRIDGE_JSON_TRY_TEXT
E_BRIDGE_MAKE_RECORD
E_BRIDGE_MAKE_TYPED_MAP
E_BRIDGE_MAP_HAS
E_BRIDGE_MAP_INDEX
E_BRIDGE_MAP_KEYS
E_BRIDGE_MAP_SET
E_BRIDGE_PRINT_ERR
E_BRIDGE_TAG_BOOL
E_BRIDGE_TAG_FLOAT
E_BRIDGE_TAG_HANDLE
E_BRIDGE_TAG_INT
E_BRIDGE_TAG_NATIVE_RECORD_BASE
E_BRIDGE_TEXT_CHARS
E_BRIDGE_TEXT_COLLAPSE_WHITESPACE
E_BRIDGE_TEXT_CONCAT
E_BRIDGE_TEXT_EQUAL
E_BRIDGE_TEXT_FIND
E_BRIDGE_TEXT_JOIN
E_BRIDGE_TEXT_LENGTH
E_BRIDGE_TEXT_REPLACE
E_BRIDGE_TEXT_SLICE
E_BRIDGE_TEXT_SPLIT
E_BRIDGE_TEXT_TRIM
E_BRIDGE_TIME_FORMAT_UTC
E_BRIDGE_TIME_PARSE_TIMESTAMP
E_BUILD_TYPE
E_BUILTIN_TZDATA
E_BUSY_DEADLINE_MS
E_BYTES
E_CACHE
E_CALL_ARITY
E_CALL_KEYWORD
E_CAPABILITY_UNSUPPORTED
E_CHUNK_ITEMS
E_CLI_DUPLICATE
E_CLI_REQUIRED
E_CLI_SCHEMA
E_CLI_UNKNOWN
E_CLI_VALUE
E_COLLECTION_ITEMS
E_COMPILE_ARGUMENTS
E_CONSTANT_HANDLE_BASE
E_COUNT
E_DATA_VERSION
E_DATE
E_DATE_FORMAT
E_DATE_RANGE
E_DEPENDENCY_INSTALL_FAILED
E_DEPENDENCY_RESOLUTION_FAILED
E_DEPTH
E_DIR
E_DIRECTORY
E_DIRECT_OPERATION_NAMES
E_DISABLE_FIND_PACKAGE_ICU
E_ENTRIES
E_ENTRY_MISSING
E_ENTRY_SIGNATURE
E_EXCL
E_EXTERN_LANGUAGE
E_EXTERN_PUBLIC
E_EXTERN_TARGET
E_EXTERN_TYPE
E_FIELD_LITERAL
E_FIELD_UNKNOWN
E_FILE
E_FLOAT_LITERAL
E_FLOAT_PARSE
E_FS_WALK
E_FS_WALK_LIMIT
E_FS_WALK_ROOT
E_FS_WALK_UTF8
E_HEX_INVALID
E_HISTORY
E_ICAL_DATE
E_ICAL_EVENT
E_ICAL_FLOATING
E_ICAL_LIMIT
E_ICAL_PARSE
E_ICAL_RANGE
E_ICAL_TIMEZONE
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
E_INLINE_TAGGED_VALUES
E_INPUTS
E_INPUT_BYTES
E_INSTALL_LIBDIR
E_INSTALL_PREFIX
E_INTEGER_DIGITS
E_INT_LITERAL
E_INT_PARSE
E_JSON_INDEX
E_JSON_MISSING
E_JSON_SHAPE
E_JSON_TYPE
E_LIMIT
E_LIST_BOOL_KIND
E_LIST_FLOAT_KIND
E_LIST_HANDLE_KIND
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
E_NET_ARGUMENT
E_NET_DENIED
E_NET_HANDLE
E_NET_IO
E_NET_LIMIT
E_NET_REFUSED
E_NET_RESOLVE
E_NET_TIMEOUT
E_NODES
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
E_PROCESS_ARGUMENT
E_PROCESS_DENIED
E_PROCESS_OUTPUT_LIMIT
E_PROCESS_RESULT
E_PROCESS_SPAWN
E_PROCESS_TIMEOUT
E_PROCESS_WAIT
E_PROFILE
E_PROFILE_OUT
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
E_REGEX_INVALID
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
E_SAFEPOINT_TEXT_LIST_TAG
E_SAMPLE_RATE_FRAMES
E_SCHEMA_VERSION
E_SELF
E_SEMVER_BUILD
E_SEMVER_CORE
E_SEMVER_IDENTIFIER
E_SEMVER_LIMIT
E_SEMVER_PRERELEASE
E_SORT_COMPARATOR
E_SORT_EFFECT
E_SORT_KEY
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
E_TAGGED_OPTION_NONE
E_TAGGED_OPTION_SOME
E_TAGGED_RESULT_ERR
E_TAGGED_RESULT_OK
E_TEMP_PREFIX
E_TEST_SIGNATURE
E_TEXT_DELIMITER
E_TIME
E_TIMESTAMP_FORMAT
E_TIMESTAMP_OFFSET
E_TIMESTAMP_PRECISION
E_TIMESTAMP_RANGE
E_TIMEZONE_COMPLEX
E_TIMEZONE_DATA
E_TIMEZONE_UNKNOWN
E_TIME_INPUT
E_TIME_RANGE
E_TIMINGS
E_TLS_ARGUMENT
E_TLS_CERTIFICATE
E_TLS_HANDSHAKE
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
E_URL_INVALID
E_UTF8_INVALID
E_UV_NOT_FOUND
E_UV_UNAVAILABLE
E_UV_VERSION_MISMATCH
E_VALUE_BYTES
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
R_CALL_TARGET
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
R_MAP_KEY
R_MAP_KEY_MISSING
R_MATH_DOMAIN
R_NATIVE_BRIDGE
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
R_ROOTS
R_SORT_ORDER
R_SQLITE_VALUE
R_STACK_INVALID
R_STACK_UNDERFLOW
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
import "std:http"

fn get_response(url: Text) -> Result[HttpResponse, HttpError]:
    let headers: Map[Text, Text] = {}
    return http_request_bytes("GET", url, headers, bytes_from_text(""), 1000)

fn show_response(response: HttpResponse) -> Int:
    let body_text = http_response_text(response)
    if not is_ok(body_text):
        let problem = error(body_text)
        eprint(http_error_code(problem))
        eprint(http_error_message(problem))
        return 1

    print(response.status)
    print(unwrap(body_text))

    let body_json = http_response_json(response)
    if not is_ok(body_json):
        let problem = error(body_json)
        eprint(problem.code)
        eprint(problem.message)
        return 1

    print(json_stringify(unwrap(body_json)))
    return 0

fn main() -> Int:
    let arguments = args()
    if len(arguments) != 1:
        print("usage: http_request.gu URL")
        return 2

    match get_response(arguments[0]):
        Ok(response):
            return show_response(response)
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

<!-- Generated reference content digest: sha256:badca94eaeda81685dc1b0db7ee7aa86787ee7c736a5d4f7453bf624e29f7061. Validated by `cargo xtask agent-reference --check`. -->
