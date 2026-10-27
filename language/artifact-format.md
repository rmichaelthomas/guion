# Guion artifact format

This document specifies the canonical artifact encoding currently emitted by the Guion compiler and accepted by the native model reader. The current format is **43**. Multi-byte integers are unsigned little-endian unless a value is explicitly described as signed or textual. Strings are UTF-8 prefixed by a `u32` byte length; counts use a `u32` and are bounded to 1,000,000. A complete artifact is at most 64 MiB. Recursive values and types are limited to 64 nested levels. Readers reject truncation, overflow, unknown tags, invalid indexes, trailing bytes, and version-incompatible instructions.

## Top-level layout

Fields occur in this order, without padding:

| Field | Encoding | Meaning |
|---|---|---|
| Magic | 4 bytes `VRTA` | Identifies a Guion artifact. |
| Format version | `u32` | Reader accepts versions 14 through 43. The current writer emits 43. |
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

Opcode tags are append-only within a format version. Tags 73–132 were added by successive format versions; format-version checks reject artifacts that use an instruction newer than their declared version. Format 36 adds stable generic sorting; the named comparator has signature `(T, T) -> Int` and must have a pure transitive call graph. Format 37 adds Unicode normalization/case mapping, exact-byte HMAC, instance-owned SQLite/TOML resources, and iCalendar parsing. Native resource type tag 17 is valid in format 37; resource handles remain non-serializable values. Format 38 adds byte-preserving HTTP request and response bodies through `http_request_bytes`. Format 39 adds explicit-grant wall-clock access through `unix_time_ms()` and `time_today_local()`.

Format 40 adds pure local-time resolution against Guion's packaged timezone database and reports its pinned data version using tags 158 and 159. Format 41 adds stable `sort_by_int_key` and `merge_sorted_by_int_key`; both compute each named `(T) -> Int` key once and charge native comparisons against the guest instruction budget. The merge operation checks that each input feed is already sorted and preserves source order for equal keys. Format 41 also adds `JsonSetLocal`, an alias-safe copy-on-write optimization for assigning `json_set(local, key, value)` back to the same mutable local. Format 42 combines those operations and assigns local-time resolution tags 161 and 162, preserving the established meanings of tags 158 and 159 in format 41.

Format 43 adds `TextJoin` (tag 163), which joins `List[Text]` using a text delimiter in one native pass. It checks the collection and text-size ceilings, accounts for the output against the guest allocation budget, and preserves older format-42 artifacts.

## Build metadata and reproducibility

Formats 15 and later end with one metadata tag: 0 means absent; 1 means present. A present entry contains compiler-version string, target string, and—starting in format 16—metadata schema `u32` (currently 1), optional source SHA-256 string, and optional dependency-lock SHA-256 string. Each optional string is encoded as tag 0 for absent or tag 1 followed by a string. A present provenance record requires a valid 64-character lowercase hexadecimal source digest. The dependency-lock digest identifies the exact lockfile; package-level dependency identities and native component hashes are recorded by the corresponding lock/manifest and are validated by packaging.

The metadata ends with nine `u64` resource ceilings in order: instruction count, allocation bytes, call depth, output bytes, integer digits, text bytes, collection items, parallelism, and file bytes. The reader checks conversions against the host address size. No bytes may follow metadata.

For byte-identical reproducibility, source names and the source table use stable relative paths, declarations and dependencies use deterministic order, and build metadata is omitted unless a build explicitly supplies it. The self-host bootstrap compares complete C2 and C3 bytes for the same relative source graph. A future format change that adds artifact identity or dependency fields must increment the format version, preserve the documented old readers, and update this contract before changing the writer.
