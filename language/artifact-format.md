# Guion artifact format

This document specifies the canonical artifact encoding currently emitted by the Guion compiler and accepted by the native model reader. The current format is **73**. Multi-byte integers are unsigned little-endian unless a value is explicitly described as signed or textual. Strings are UTF-8 prefixed by a `u32` byte length; counts use a `u32` and are bounded to 1,000,000. A complete artifact is at most 64 MiB. Recursive values and types are limited to 64 nested levels. Readers reject truncation, overflow, unknown tags, invalid indexes, trailing bytes, and version-incompatible instructions.

## Top-level layout

Fields occur in this order, without padding:

| Field | Encoding | Meaning |
|---|---|---|
| Magic | 4 bytes `VRTA` | Identifies a Guion artifact. |
| Format version | `u32` | Reader accepts versions 14 through 73. The current writer emits 73. |
| Source name | string | Entry source identity; relative names are portable. |
| Entry function | string | Name of the entry function, normally `main`. |
| Sources | count, then strings | Source graph in stable compiler order. A span's `source_id` indexes this table. |
| Records | count, then record entries | Each entry is name, field count, then each field's name and encoded type. |
| External declarations | count, then external entries | Each entry is symbol name, module, callable, parameter count, each parameter name and type, then result type. |
| Functions | count, then function entries | Each entry is name, parameter count and types, result type, local count, instruction count, then instructions. |
| Build metadata | versioned optional entry | Present in formats 15 and later. Its byte layout is below. |

Every record, external, function, field, parameter, and instruction index is zero-based. Call operands refer to the function table; external-call operands refer to the external table; record and field operands refer to the record table and that record's field table. Branch destinations refer to executable instruction indexes after labels are resolved. Labels themselves are compile-time IR and are not serialized.

## Spans

Each instruction starts with a 36-byte source span: `source_id:u32`, then `start:u64`, `end:u64`, `line:u64`, and `column:u64`. Offsets are UTF-8 byte offsets into the source selected by `source_id`; line and column are one-based. The encoder supplies the instruction position as a column only when an IR instruction has no explicit span. All integer conversions are checked before emission.

## Types

Types are a one-byte tag followed by the indicated payload. Nested types use the same encoding recursively.

| Tag | Type | Payload |
|---:|---|---|
| 0 | `Int` | — |
| 1 | `Bool` | — |
| 2 | `Text` | — |
| 3 | `Unit` | — |
| 4 | `List[T]` | encoded `T` |
| 5 | `Result[Ok, Err]` | encoded `Ok`, then encoded `Err` |
| 6 | `TaskError` | — |
| 7 | `Json` | — |
| 8 | `JsonError` | — |
| 9 | `FileError` | — |
| 10 | `Option[T]` | encoded `T` |
| 11 | `Map[Text, T]` | encoded value type `T`; map keys are always `Text` |
| 12 | `Float64` | — |
| 13 | record type | string record name |
| 14 | `Bytes` | — |
| 15 | `HttpError` | — |
| 17 | opaque native resource | UTF-8 resource kind string; runtime-only handles cannot appear as constants |
| 18 | `Array` | element code (`u8`; format 62): 0 `i8`, 1 `i16`, 2 `i32`, 3 `i64`, 4 `u8`, 5 `u16`, 6 `u32`, 7 `u64`, 8 `f32`, 9 `f64`, 10 `bool`. Arrays are runtime values only and cannot appear as constants |
| 19 | function type | parameter count (`u32`, at most 255), each parameter type, then the result type (format 66). A function type names a function signature; function values are runtime values only and cannot appear as constants |

## Constant values

The `Const` instruction (tag 0) embeds one value. Values are recursively encoded with the following tags:

| Tag | Value | Payload |
|---:|---|---|
| 0 | `Int` | canonical base-10 integer in a string; arbitrary precision |
| 1 | `Bool` | one byte, exactly 0 or 1 |
| 2 | `Text` | UTF-8 string |
| 3 | `Unit` | — |
| 4 | list | count, then values |
| 5 | successful `Result` | value |
| 6 | failed `Result` | value |
| 7 | `TaskError` | decimal index string, code string, message string, trace count and spans |
| 8 | `Json` | canonical JSON text |
| 9 | `JsonError` | code string, message string, line count, column count |
| 10 | `FileError` | code string, message string, trace count and spans |
| 11 | `Option.None` | — |
| 12 | `Option.Some` | value |
| 13 | map | count, then each text key and value; duplicate keys are invalid |
| 14 | finite `Float64` | IEEE-754 binary64 bits as `u64`; NaN and infinities are invalid |
| 15 | record value | record name string, field count, then field values in declaration order |
| 16 | `Bytes` | `u32` byte length, then raw bytes |
| 17 | `HttpError` | code string, message string, trace count and spans |

## Instructions

Every instruction is its span followed by a one-byte opcode tag and the listed operands. Counts and indexes are `u32`; strings are length-prefixed UTF-8. An em dash means no operands.

| Tag | Instruction | Operands |
|---:|---|---|
| 0 | `Const` | encoded value |
| 1 | `Load` | local index |
| 2 | `Store` | local index |
| 3 | `Pop` | — |
| 4 | `Add` | — |
| 5 | `Subtract` | — |
| 6 | `Multiply` | — |
| 7 | `Negate` | — |
| 8 | `Not` | — |
| 9 | `Equal` | — |
| 10 | `NotEqual` | — |
| 11 | `Less` | — |
| 12 | `LessEqual` | — |
| 13 | `Greater` | — |
| 14 | `GreaterEqual` | — |
| 15 | `Jump` | executable instruction index |
| 16 | `JumpIfFalse` | executable instruction index |
| 17 | `Call` | function index, argument count |
| 18 | `Print` | — |
| 19 | `Return` | — |
| 20 | `Length` | — |
| 21 | `Index` | — |
| 22 | `MakeList` | item count |
| 23 | `ParallelMap` | worker function index |
| 24 | `ReadText` | — |
| 25 | `WriteText` | — |
| 26 | `Args` | — |
| 27 | `Assert` | — |
| 28 | `IsOk` | — |
| 29 | `Unwrap` | — |
| 30 | `GetError` | — |
| 31 | `TaskIndex` | — |
| 32 | `TaskCode` | — |
| 33 | `TaskMessage` | — |
| 34 | `PythonCall` | external index, argument count |
| 35 | `JsonParse` | — |
| 36 | `JsonStringify` | — |
| 37 | `JsonGet` | — |
| 38 | `JsonIndex` | — |
| 39 | `JsonHas` | — |
| 40 | `JsonLength` | — |
| 41 | `JsonAsText` | — |
| 42 | `JsonAsInt` | — |
| 43 | `JsonAsBool` | — |
| 44 | `JsonIsNull` | — |
| 45 | `JsonErrorMessage` | — |
| 46 | `JsonErrorLine` | — |
| 47 | `JsonErrorColumn` | — |
| 48 | `JsonErrorCode` | — |
| 49 | `FileErrorCode` | — |
| 50 | `FileErrorMessage` | — |
| 51 | `MakeOk` | — |
| 52 | `MakeErr` | — |
| 53 | `TryResult` | — |
| 54 | `FileErrorLine` | — |
| 55 | `FileErrorColumn` | — |
| 56 | `MakeNone` | — |
| 57 | `MakeSome` | — |
| 58 | `IsSome` | — |
| 59 | `MakeMap` | pair count; stack carries each key/value pair |
| 60 | `MapKeys` | — |
| 61 | `MapSet` | — |
| 62 | `MapHas` | — |
| 63 | `Divide` | — |
| 64 | `JsonAsFloat` | — |
| 65 | `IntegerDivide` | — |
| 66 | `FloorDivide` | — |
| 67 | `Remainder` | — |
| 68 | `MakeRecord` | record index, field count |
| 69 | `GetField` | record index, field index |
| 70 | `ReadBytes` | — |
| 71 | `WriteBytes` | — |
| 72 | `PythonCallKw` | external index, positional count, keyword count, keyword-name strings |
| 73 | `HttpGet` | — |
| 74 | `HttpErrorCode` | — |
| 75 | `HttpErrorMessage` | — |
| 76 | `JsonNull` | — |
| 77 | `JsonBool` | — |
| 78 | `JsonInt` | — |
| 79 | `JsonFloat` | — |
| 80 | `JsonText` | — |
| 81 | `JsonArray` | — |
| 82 | `JsonObject` | — |
| 83 | `JsonPush` | — |
| 84 | `JsonSet` | — |
| 85 | `WriteTextAtomic` | — |
| 86 | `ArgsFrom` | — |
| 87 | `HttpRequest` | — |
| 88 | `Environment` | — |
| 89 | `MakeTypedList` | encoded item type |
| 90 | `MakeTypedMap` | encoded map value type |
| 91 | `ListPush` | — |
| 92 | `ListSet` | — |
| 93 | `FloatSqrt` | — |
| 94 | `ListSetLocal` | local index |
| 95 | `HttpRequestWithTimeout` | — |
| 96 | `PrintErr` | — |
| 97 | `JsonIntColumn` | — |
| 98 | `JsonTextCategories` | — |
| 99 | `JsonAsIntList` | — |
| 100 | `JsonProjectColumns` | — |
| 101 | `JsonTextCategoriesValues` | — |
| 102 | `TextChars` | — |
| 103 | `ListPushLocal` | local index |
| 104 | `JsonPushLocal` | local index |
| 105 | `BytesFromInts` | — |
| 106 | `BytesFromText` | — |
| 107 | `BytesJoin` | — |
| 108 | `Float64Bits` | — |
| 109 | `FileOpenLines` | — |
| 110 | `FileNextLine` | — |
| 111 | `FileClose` | — |
| 112 | `CsvOpen` | — |
| 113 | `CsvNext` | — |
| 114 | `CsvEncode` | — |
| 115 | `JsonTryGet` | — |
| 116 | `JsonTryIndex` | — |
| 117 | `JsonTryText` | — |
| 118 | `JsonTryInt` | — |
| 119 | `JsonTryBool` | — |
| 120 | `JsonTryFloat` | — |
| 121 | `TimeParseDate` | — |
| 122 | `TimeParseTimestamp` | — |
| 123 | `TimeFormatUtc` | — |
| 124 | `SemVerParse` | — |
| 125 | `SemVerCompare` | — |
| 126 | `CliParse` | — |
| 127 | `FsWalk` | — |
| 128 | `ListGetOption` | — |
| 129 | `MapGetOption` | — |
| 130 | `TextParseInt` | — |
| 131 | `TextParseFloat` | — |
| 132 | `SortBy` | comparator function index |
| 133 | `TextCaseFold` | — |
| 134 | `TextCollapseWhitespace` | — |
| 135 | `TextNormalize` | — |
| 136 | `CryptoHmacSha256` | — |
| 137 | `CryptoVerifyHmacSha256` | — |
| 138 | `SqliteOpen` | — |
| 139 | `SqliteBegin` | — |
| 140 | `SqliteExecute` | — |
| 141 | `SqliteQuery` | — |
| 142 | `SqliteCommit` | — |
| 143 | `SqliteRollback` | — |
| 144 | `SqliteClose` | — |
| 145 | `TomlParse` | — |
| 146 | `TomlHas` | — |
| 147 | `TomlGetText` | — |
| 148 | `TomlGetInt` | — |
| 149 | `TomlGetBool` | — |
| 150 | `TomlPutText` | — |
| 151 | `TomlPutInt` | — |
| 152 | `TomlPutBool` | — |
| 153 | `TomlRender` | — |
| 154 | `CalendarParse` | — |
| 155 | `HttpRequestBytes` | — |
| 156 | `UnixTimeMs` | — |
| 157 | `TimeTodayLocal` | — |
| 158 | `TimeResolveLocal` (format 40); `SortByIntKey` (format 41 and later) | none; key function index |
| 159 | `TimeZoneDataVersion` (format 40); `MergeSortedByIntKey` (format 41 and later) | none; key function index |
| 160 | `JsonSetLocal` | local index |
| 161 | `TimeResolveLocal` in format 42 | — |
| 162 | `TimeZoneDataVersion` in format 42 | — |
| 163 | `TextJoin` | — |
| 164 | `JsonStringifyPairs` | — |
| 165 | `HttpResponseText` | — |
| 166 | `TextSlice` | — |
| 167 | `HttpResponseJson` | — |
| 168 | `TimeFormatUtcDate` | — |
| 169 | `MapRecordGet` | record index, field index, nested flag (`u8`: 0 for a direct map, 1 for a map of maps) |
| 170 | `MapRecordSet` | record index, field index, nested flag (`u8`: 0 for a direct map, 1 for a map of maps) |
| 171 | `JsonTryGetText` | — (format 50: pops key and Json, pushes `Result[Text, JsonError]`; same errors as `JsonTryGet` followed by `JsonTryText`) |
| 172 | `JsonTryGetInt` | — (format 50: pops key and Json, pushes `Result[Int, JsonError]`; same errors as `JsonTryGet` followed by `JsonTryInt`) |
| 173 | `TextFind` | — (format 51: pops start, needle and text, pushes the scalar index of the first match at or after start, or -1) |
| 174 | `TextSplit` | — (format 51: pops delimiter and text, pushes `List[Text]`) |
| 175 | `TextReplace` | — (format 51: pops replacement, needle and text, pushes `Text`) |
| 176 | `TextTrim` | — (format 51: pops text, pushes `Text`) |
| 177 | `Math` | function code (`u8`; format 52): 0 `sin`, 1 `cos`, 2 `tan`, 3 `asin`, 4 `acos`, 5 `atan`, 6 `sinh`, 7 `cosh`, 8 `tanh`, 9 `exp`, 10 `ln`, 11 `log10`, 12 `log2`, 13 `floor`, 14 `ceil`, 15 `round`, 16 `abs`, 17 `pow`, 18 `atan2`, 19 `hypot`. Pops one `Float64` (two for codes 17 to 19; the first operand pushed is the first argument) and pushes `Float64`. Domain errors raise `R_MATH_DOMAIN`, a zero base with a negative exponent raises `R_DIV_ZERO`, and a non-finite result raises `R_FLOAT_NONFINITE` |
| 178 | `IntToFloat` | — (format 52: pops `Int`, pushes the nearest `Float64`; a value too large raises `R_FLOAT_NONFINITE`) |
| 179 | `FloatToInt` | — (format 52: pops `Float64`, pushes the `Int` truncated toward zero) |
| 180 | `NumberToText` | — (format 52: pops `Int` or `Float64`, pushes the same text `print` shows) |
| 181 | `RandomMix` | — (format 52: pops an `Int`, pushes the SplitMix64 output function applied to its low 64 bits as a non-negative `Int`) |
| 182 | `Codec` | function code (`u8`; format 53): 0 `native_base64_encode`, 1 `native_base64url_encode`, 2 `native_base64_decode`, 3 `native_base64url_decode`, 4 `native_hex_encode`, 5 `native_hex_decode`, 6 `bytes_to_ints`, 7 `bytes_slice`, 8 `text_from_bytes`, 9 `text_from_bytes_lossy`, 10 `native_percent_encode`, 11 `native_percent_decode`. Operand and result types: encoders take `Bytes` and push `Text`; decoders take `Text` and push `Option[Bytes]`; `bytes_to_ints` pops `Bytes`, pushes `List[Int]`; `bytes_slice` pops end, start and `Bytes`, pushes `Bytes` and raises `R_INDEX_BOUNDS` outside `0 <= start <= end <= len`; `text_from_bytes` pushes `Option[Text]` (`None` unless the bytes are valid UTF-8); `text_from_bytes_lossy` pushes `Text` with U+FFFD for invalid sequences; percent encoding pushes `Text` and percent decoding `Option[Text]` | Format 59 adds codes 12 `sha1` (`Bytes` to `Bytes`, 20 bytes; for the WebSocket handshake and legacy interchange) and 13 `bytes_xor` (`Bytes` data, `Bytes` non-empty repeating key, in that push order; `R_INDEX_BOUNDS` on an empty key; format 67 adds code 14 `sha256`, `Bytes` to `Bytes`, 32 bytes)
| 183 | `Fs` | function code (`u8`; format 54): 0 `exists`, 1 `kind`, 2 `size`, 3 `modified_ms`, 4 `list_dir`, 5 `make_dir`, 6 `remove`, 7 `remove_all`, 8 `rename`, 9 `copy`, 10 `temp_dir`, 11 `temp_file`. Pops one `Text` path (two for `rename` and `copy`: source then destination) and pushes `Result[T, FileError]`: `Bool`, `Text`, `Int`, `Int`, `List[Text]`, `Unit`, `Unit`, `Unit`, `Unit`, `Int` (bytes copied), `Text`, `Text`. Codes 0 to 4 need the file-read grant and 5 to 11 the file-write grant; format 67 adds code 12 `sha256` (`Result[Bytes, FileError]`, the 32-byte SHA-256 digest of the file streamed in fixed-size blocks, file-read grant); format 71 adds codes 13 `gzip_compress_file` and 14 `gzip_decompress_file` (two `Text` paths, source then destination; `Result[Int, FileError]`: the compressed size, or the decompressed size, in bytes; both stream through fixed 64 KiB buffers, need the file-read and file-write grants, and write the destination through a sibling temporary file renamed on success), 15 `tar_write` (directory then archive path, `Result[Int, FileError]`: entries written) and 16 `tar_read` (archive path then destination directory, `Result[Int, FileError]`: entries extracted) with the file-read and file-write grants, and 17 `tar_list` (one `Text` path, `Result[List[Text], FileError]`, file-read grant). Format 73 adds codes 18 `read_range` (three `Text` operands pushed in order, path then start then length, both decimal non-negative integers; `Result[Bytes, FileError]`: at most `length` bytes of the file from byte `start`, shorter at the end of the file, never more than 16 MiB per call, memory held only for the bytes returned; file-read grant; the file must be a regular file) and 19 `confined_path` (root then relative path, both `Text`; `Result[Text, FileError]`: `root/relative` when both resolve, through symbolic links, to a place under the granted roots and the target stays inside the root, otherwise `R_HOST_DENIED`; file-read grant) Codes 13 to 16 count as write functions; the `FileError` code of a refused archive is `E_TAR_UNSAFE_PATH`, `E_TAR_CORRUPT`, `E_TAR_UNSUPPORTED_ENTRY`, `E_TAR_LIMIT`, or one of `E_COMPRESS_LIMIT`, `E_COMPRESS_TRUNCATED`, `E_COMPRESS_CORRUPT`, `E_COMPRESS_TRAILING_DATA`; paths are confined to the granted roots exactly as for `read_text` |
| 184 | `Regex` | function code (`u8`; format 55): 0 `is_match`, 1 `find`, 2 `find_all`, 3 `replace`, 4 `replace_first`, 5 `split`, 6 `escape`. Pops `Text` operands (pattern then subject, plus a replacement for codes 3 and 4; one operand for `escape`) and pushes `Result[T, Text]` where the error is the pattern's compile message: `Bool`, `Option[List[Text]]` (group 0 is the whole match, unmatched groups are empty), `List[Text]`, `Text`, `Text`, `List[Text]`. `escape` pushes `Text` directly. Pure: no host grant |
| 185 | `Process` | function code (`u8`; format 56): 0 `run`, 1 `pid`, 2 `signal_poll`. `run` pops `Text` command, `List[Text]` arguments and `Text` standard input (pushed in that order) and pushes `Result[List[Text], List[Text]]`: on success `[exit_code, stdout, stderr]` as text, on failure `[code, message]`; it needs the command grant, and the error codes are `E_PROCESS_DENIED`, `E_PROCESS_ARGUMENT`, `E_PROCESS_SPAWN`, `E_PROCESS_TIMEOUT`, `E_PROCESS_OUTPUT_LIMIT` and `E_PROCESS_WAIT`. `pid` pushes the `Int` process id. `signal_poll` pushes `Text`: `"INT"`, `"TERM"`, `"HUP"` or `""`, and clears the pending signal |
| 186 | `Net` | function code (`u8`; format 57): 0 `tcp_connect(host, port, timeout_ms)`, 1 `tcp_listen(host, port)`, 2 `tcp_accept(handle, timeout_ms)`, 3 `local_port(handle)`, 4 `send(handle, bytes)`, 5 `recv(handle, max, timeout_ms)`, 6 `close(handle)`, 7 `udp_bind(host, port)`, 8 `udp_send_to(handle, host, port, bytes)`, 9 `udp_recv_from(handle, max, timeout_ms)`. Operands are pushed in that order. Pushes `Result[T, List[Text]]` where a failure is `[code, message]` and `T` is `Int` (a handle, port or byte count), `Bytes`, `Unit`, or `List[Bytes]` (`[datagram, sender "host:port"]`). Connecting and datagram sends need a matching `connect` grant, binding a matching `bind` grant |
| 187 | `Tls` | function code (`u8`; format 58): 0 `connect(host, port, timeout_ms, extra_ca_pem)`, 1 `accept(listener, timeout_ms, certificate_chain_pem, private_key_pem)`; format 61 adds 2 `connect_identity(host, port, timeout_ms, extra_ca_pem, certificate_chain_pem, private_key_pem)` (the client presents a certificate) and 3 `accept_verified(listener, timeout_ms, certificate_chain_pem, private_key_pem, client_ca_pem)` (the server requires a client certificate issued by `client_ca_pem`), and a reader rejects codes 2 and 3 in older formats. Pops four (codes 0 and 1), six (code 2) or five (code 3) operands and pushes `Result[Int, List[Text]]`: a socket handle usable with the `Net` send, receive, port and close functions, or `[code, message]`. Connecting needs the same `connect` grant as TCP |
| 188 | `RandomSeed` | — (format 61: pops an `Int`, pushes its low 64 bits read as a signed 64-bit `Int`; this is the generator state for the seed) |
| 189 | `RandomAdvance` | — (format 61: pops a state `Int`, pushes its low 64 bits plus the SplitMix64 increment `0x9E3779B97F4A7C15`, wrapping, read as signed) |
| 190 | `RandomBits` | — (format 61: pops a state `Int`, pushes the SplitMix64 output function of its low 64 bits read as signed) |
| 191 | `RandomUnitFloat` | — (format 61: pops a state `Int`, pushes the top 53 bits of the SplitMix64 output divided by 2^53, a `Float64` in `[0, 1)`) |
| 192 | `RandomBelow` | — (format 61: pops a state `Int` and a range size `Int`, pushes the SplitMix64 output reduced modulo the size, or `-1` when the output falls in the rejected tail at or above `2^64 - (2^64 mod size)`; a size outside `1..=2^64` raises `R_RANDOM_RANGE`) |
| 193 | `Array` | function code (`u8`) then element code (`u8`; format 62). Function codes are positions in `ArrayFunction::ALL` in `crates/model/src/array.rs`, and the operand and result types for each function and element type are `ArrayFunction::signature`. For constructors and `Convert` the element code is the result's element type; otherwise it is the element type of the first array operand. A reader rejects tag 193 below format 62 |
| 195 | `CsvReadColumns` | — (format 64: pops a `Map[Text, Text]` of fixed column kinds and a `Text` path, pushes `Result[List[Record[CsvColumn]], FileError]`; a reader rejects tag 195 below format 64) |
| 196 | `SortNatural` | — (format 65: pops a `List[T]` for `T` in `Text`, `Int` or `Float64` and pushes it stably sorted in natural order: `Text` by Unicode scalar value, `Int` numerically, `Float64` with `-0.0` before `0.0`; a reader rejects tag 196 below format 65) |
| 197 | `SortByKey` | key function index (format 65: pops a `List[T]` and pushes it stably sorted by the key `K` of each item, `K` in `Text`, `Int` or `Float64`, compared as `SortNatural` compares; the key function takes one `T` and returns `K`, is pure, and runs once per item in input order; a reader rejects tag 197 below format 65) |
| 198 | `MakeFn` | function index (format 66: pushes the function value for the function at that index; the value is that index and names a function of this artifact only; a reader rejects tag 198 below format 66) |
| 199 | `CallFn` | argument count (format 66: pops a function value and then that many arguments, pushing the call's result; the call uses the ordinary call frame, so the call-depth and instruction limits apply as for `Call`; an index that names no function, or a function with a different parameter count, is the runtime error `R_FN_INVALID`; a reader rejects tag 199 below format 66) |
| 200 | `HigherOrder` | kind (`u8`; format 66). Kinds are positions in `HigherOrderKind::ALL` in `crates/model/src/lib.rs`: 0 `SortBy`, 1 `SortByKey`, 2 `SortByIntKey`, 3 `MergeSortedByIntKey`, 4 `ParallelMap`, 5 `MapList`, 6 `FilterList`, 7 `FoldList`, 8 `AnyList`, 9 `AllList`. The function value is popped first, from the top of the stack, then the other operands in the order of the source arguments: the sorts take a list and the function; `ParallelMap` takes a list, the function and the worker limit; `FoldList` takes a list, the initial value and the function. The first four kinds and `ParallelMap` behave as the operations that carry a function index (`SortBy`, `SortByKey`, `SortByIntKey`, `MergeSortedByIntKey`, `ParallelMap`); `MapList`, `FilterList`, `AnyList` and `AllList` call the function once per item in order (`AnyList` and `AllList` stop at the first deciding answer), and `FoldList` threads an accumulator. A reader rejects tag 200 below format 66 |
| 201 | `RandomBytes` | — (format 68: pops an `Int` byte count and pushes `Result[Bytes, StdError]` drawn from the host's entropy source, the operating system's cryptographic generator unless an embedding pinned another; a count of 0 gives empty `Bytes`, a negative count or one above 1,048,576 is `Err` with code `E_RANDOM_COUNT`, and a failing source is `Err` with code `E_ENTROPY_UNAVAILABLE`; needs no grant; a reader rejects tag 201 below format 68) |
| 202 | `SqliteBeginRead` | — (format 69: pops a `Db` and pushes `Result[Transaction, StdError]` for a transaction opened with `BEGIN DEFERRED`; a statement that writes inside it is `Err` with code `E_SQLITE_READ_ONLY`; a reader rejects tag 202 below format 69) |
| 203 | `LogAppend` | — (format 70: pops a `Text` line and then a `Text` path and pushes `Result[Unit, FileError]`; it appends the line, verbatim, to the file at the path, creating it if missing, and needs the file-write grant and root confinement of `WriteText`, whose errors it returns; an empty line only creates the file) |
| 204 | `LogDefaultLevel` | — (format 70: pushes the `Text` default log level, the `[log] level` of the manifest, or `info` when it sets none) |
| 205 | `GzipCompress` | — (format 71: pops a `Bytes` and pushes the `Bytes` of a gzip stream (RFC 1952, one member, default level); needs no grant; a reader rejects tag 205 below format 71) |
| 206 | `GzipDecompress` | — (format 71: pops an `Int` output ceiling and then a `Bytes` and pushes `Result[Bytes, StdError]`: all gzip members of the input, concatenated; the ceiling is the smaller of the operand and the host's `decompress_limit_bytes`, and output past it is `Err` with code `E_COMPRESS_LIMIT` having produced no more than the ceiling; a negative operand is `E_COMPRESS_ARGUMENT`; empty or short input is `E_COMPRESS_TRUNCATED`; a bad header, checksum or deflate data is `E_COMPRESS_CORRUPT`; bytes after the last member that do not begin another are `E_COMPRESS_TRAILING_DATA`; needs no grant; a reader rejects tag 206 below format 71) |
| 207 | `ZlibCompress` | — (format 71: as 205 for a zlib stream, RFC 1950) |
| 208 | `ZlibDecompress` | — (format 71: as 206 for one zlib stream; any bytes after the end of the stream are `E_COMPRESS_TRAILING_DATA`) |
| 209 | `YamlParse` | — (format 72: pops a `Text` and pushes `Result[Json, StdError]`: the one YAML document in the text, read as the documented read-only subset (block and flow collections, plain, quoted, literal and folded scalars; plain scalars are text except lowercase `true` and `false`, `null` and `~`, and an integer or decimal that reads back unchanged); anchors, aliases, tags, the merge key, complex keys, directives and multi-document streams are `Err` with code `YAML_UNSUPPORTED`; other failures are `YAML_SYNTAX`, `YAML_DUPLICATE_KEY` and `YAML_LIMIT`, each with the 1-based `line` and `column`; needs no grant; a reader rejects tag 209 below format 72) |

Opcode tags are append-only within a format version. Tags 73–132 were added by successive format versions; format-version checks reject artifacts that use an instruction newer than their declared version. Format 36 adds stable generic sorting; the named comparator has signature `(T, T) -> Int` and must have a pure transitive call graph. Format 37 adds Unicode normalization/case mapping, exact-byte HMAC, instance-owned SQLite/TOML resources, and iCalendar parsing. Native resource type tag 17 is valid in format 37; resource handles remain non-serializable values. Format 38 adds byte-preserving HTTP request and response bodies through `http_request_bytes`. Format 39 adds explicit-grant wall-clock access through `unix_time_ms()` and `time_today_local()`.

Format 40 adds pure local-time resolution against Guion's packaged timezone database and reports its pinned data version using tags 158 and 159. Format 41 adds stable `sort_by_int_key` and `merge_sorted_by_int_key`; both compute each named `(T) -> Int` key once and charge native comparisons against the guest instruction budget. The merge operation checks that each input feed is already sorted and preserves source order for equal keys. Format 41 also adds `JsonSetLocal`, an alias-safe copy-on-write optimization for assigning `json_set(local, key, value)` back to the same mutable local. Format 42 combines those operations and assigns local-time resolution tags 161 and 162, preserving the established meanings of tags 158 and 159 in format 41.

Format 43 adds `TextJoin` (tag 163), which joins `List[Text]` using a text delimiter in one native pass. It checks the collection and text-size ceilings, accounts for the output against the guest allocation budget, and preserves older format-42 artifacts.

Format 44 adds `JsonStringifyPairs` (tag 164), which streams a `Map[Text, List[Text]]` into a sorted JSON array of two-field text objects. It preserves map-key and per-list order, escapes field names and values, checks row and output limits before allocating the output, and avoids materializing an intermediate JSON tree.

Format 45 adds `HttpResponseText` (tag 165), which converts the byte-preserving `HttpResponse.body` to checked UTF-8 text and returns `Result[Text, HttpError]`. It observes the configured text-size ceiling, charges the output against the guest allocation budget, reports invalid UTF-8 as `R_HTTP_ENCODING`, and preserves the response status as data, including non-2xx statuses. Format-44 artifacts remain readable and cannot contain this instruction.

Format 46 adds `TextSlice` (tag 166), which returns a half-open range of Unicode scalar values from `Text`. Bounds are non-negative scalar indices, the end is exclusive, and `0 <= start <= end <= len(text)` is required. It checks and charges the copied output against the configured text and allocation ceilings; the complete-range case reuses the input value. Format-45 artifacts remain readable and cannot contain this instruction.

Format 47 adds `HttpResponseJson` (tag 167), which validates the response body's configured text-size ceiling and UTF-8 encoding, then parses JSON directly from the existing response bytes. It preserves the `StdError` codes and messages returned by `std:http.http_response_json`, including duplicate-key diagnostics. The operation retains the existing guest allocation charge for the response-text conversion while avoiding the intermediate `Text` allocation. Format-46 artifacts remain readable and cannot contain this instruction.

Format 51 adds native text search and rewriting. `TextFind` (tag 173) returns the Unicode scalar index of the first occurrence of a needle at or after a scalar start index, or -1 when there is none; an empty needle matches at the start, and a start past the end of the text is `R_INDEX_BOUNDS`. `TextSplit` (tag 174) splits on every non-overlapping occurrence of a delimiter from left to right and returns the whole text as one field for an empty delimiter. `TextReplace` (tag 175) replaces every non-overlapping occurrence from left to right and returns the text unchanged for an empty needle. `TextTrim` (tag 176) removes leading and trailing Unicode `White_Space` scalars. Each operation runs in one pass over the text, checks the collection and text-size ceilings before allocating, and charges its output against the guest allocation budget. Format-50 artifacts remain readable and cannot contain these instructions.

Format 62 adds native arrays: type tag 18 and instruction tag 193. An artifact below format 62 that mentions either is rejected; formats 14 through 61 remain readable.

Format 63 appends array function codes for gathering, grouping and joining (`Take`, `Factorize`, `FirstIndex`, `CountBy`, `SumBy`, `MinBy`, `MaxBy`, `MeanBy`, `JoinIndex`) to instruction tag 193. A reader rejects any of these function codes below format 63; formats 14 through 62 remain readable.

Format 64 adds `CsvReadColumns` (tag 195), which reads a whole CSV file into typed columns in one native operation. An artifact below format 64 that uses it is rejected; formats 14 through 63 remain readable.

Format 65 adds `SortNatural` (tag 196) and `SortByKey` (tag 197): `sort(values)` and `sort_by(values, key)` with a one-parameter key function. Both are stable merge sorts (insertion sort up to 32 items) and charge one instruction of the guest budget per comparison, as `SortByIntKey` does. `Float64` keys use a total order: every NaN after every number and equal to each other, `-0.0` before `0.0`. `Text` keys compare by Unicode scalar value, which is the order of sorted map keys. A named function with two parameters keeps compiling to `SortBy` (format 36), and `sort_by_int_key` and `merge_sorted_by_int_key` keep their format-41 operations. An artifact below format 65 that uses tag 196 or 197 is rejected; formats 14 through 64 remain readable.

Format 66 adds first-class function values: type tag 19 and instruction tags 198 (`MakeFn`), 199 (`CallFn`) and 200 (`HigherOrder`). A function value is an index into the function table of the artifact that made it, captures nothing, and is never written into an artifact as data (a `Const` of a function value is `E_ARTIFACT_VALUE`). An artifact below format 66 that uses a function type or any of the three instructions is rejected with `E_ARTIFACT_VERSION`, on read and on write; formats 14 through 65 remain readable. The effects of a function value belong to the function that forms it (`MakeFn`), so a tool that follows `Call` edges follows `MakeFn` edges the same way.

Format 67 adds SHA-256: `Codec` function code 14 (`sha256`) and `Fs` function code 12 (`sha256`, the streamed file digest). No new instruction tag is used. An artifact below format 67 that uses either is rejected with `E_ARTIFACT_VERSION`, on read and on write; formats 14 through 66 remain readable.

Format 68 adds `RandomBytes` (tag 201) for `crypto_random_bytes`, `crypto_random_hex` and `crypto_random_base64url`. An artifact below format 68 that uses it is rejected with `E_ARTIFACT_VERSION`, on read and on write; formats 14 through 67 remain readable. A program that uses it is not reproducible: its output differs between runs by design.

Format 69 adds `SqliteBeginRead` (tag 202) for `sqlite_begin_read`. An artifact below format 69 that uses it is rejected with `E_ARTIFACT_VERSION`, on read and on write; formats 14 through 68 remain readable.

Format 70 adds `LogAppend` (tag 203) and `LogDefaultLevel` (tag 204) for `std:log`. An artifact below format 70 that uses either is rejected with `E_ARTIFACT_VERSION`, on read and on write; formats 14 through 69 remain readable.

Format 71 adds `GzipCompress`, `GzipDecompress`, `ZlibCompress` and `ZlibDecompress` (tags 205 to 208) and `Fs` function codes 13 to 17 (`gzip_compress_file`, `gzip_decompress_file`, `tar_write`, `tar_read`, `tar_list`) for `std:compress` and `std:tar`. An artifact below format 71 that uses any of them is rejected with `E_ARTIFACT_VERSION`, on read and on write; formats 14 through 70 remain readable.

Format 72 adds `YamlParse` (tag 209) for `std:yaml`. An artifact below format 72 that uses it is rejected with `E_ARTIFACT_VERSION`, on read and on write; formats 14 through 71 remain readable.

Format 73 adds `Fs` function codes 18 (`read_range`) and 19 (`confined_path`) for `std:http_static`. An artifact below format 73 that uses either is rejected with `E_ARTIFACT_VERSION`, on read and on write; formats 14 through 72 remain readable.

## Build metadata and reproducibility

Formats 15 and later end with one metadata tag: 0 means absent; 1 means present. A present entry contains compiler-version string, target string, and—starting in format 16—metadata schema `u32` (currently 1), optional source SHA-256 string, and optional dependency-lock SHA-256 string. Each optional string is encoded as tag 0 for absent or tag 1 followed by a string. A present provenance record requires a valid 64-character lowercase hexadecimal source digest. The dependency-lock digest identifies the exact lockfile; package-level dependency identities and native component hashes are recorded by the corresponding lock/manifest and are validated by packaging.

The metadata continues with nine `u64` resource ceilings in order: instruction count, allocation bytes, call depth, output bytes, integer digits, text bytes, collection items, parallelism, and file bytes. The reader checks conversions against the host address size.

Starting in format 60 the metadata ends with the packages the build links: a `u32` count, then for each package its name string, exact `major.minor.patch` version string, and 64-character hexadecimal content-hash string, in name order. The hash is the one recorded in `guion.lock`. A format 59 or older writer rejects a non-empty list, and older readers do not read the section. No bytes may follow metadata.

For byte-identical reproducibility, source names and the source table use stable relative paths, declarations and dependencies use deterministic order, and build metadata is omitted unless a build explicitly supplies it. The self-host bootstrap compares complete C2 and C3 bytes for the same relative source graph. A future format change that adds artifact identity or dependency fields must increment the format version, preserve the documented old readers, and update this contract before changing the writer.
