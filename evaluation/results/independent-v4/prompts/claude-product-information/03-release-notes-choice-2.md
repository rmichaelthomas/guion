You are advising a developer choosing a language for the attached task.
Do not implement the task, browse, use tools, or infer results from code that has not been built.
Choose exactly one: Guion, Python, or either. Explain the decisive task-specific tradeoffs and what would change your choice.
In the product-information arm use only the supplied profiles. In the cold arm no product documentation is supplied; use only prior knowledge and the task brief.
Do not make a universal language-replacement claim.

Assignment ID: 03-release-notes-choice-2
Schedule SHA-256: 22c28961c9a5864a782e43594d395f0449b34d0ae8793a9b8752e955ba2e5547
Schedule file SHA-256: f5735516a26fdc25b19b6e64625b77f166699d3dec4acfb67a18049d5c3e0ad8
Choice-safe bundle SHA-256: 16ffe49108785a222a033decb416ac2d129a017949ce4e3b7d99f16f8808bc6a
Presentation order: Guion, Python
Configured assistant: Anthropic Claude claude-sonnet-5 (claude-sonnet-5); reasoning effort provider-default
Budget: 5 active minutes and 0 tool calls.
No implementation results, benchmarks, author calibration, or candidate runs have been shown.

# Task brief
# Release note assembler

**Class:** small automation or data CLI
**Origin:** independent-candidate brief draft
**Initial request:** Build a CLI invoked as `release-notes --changes DIR --config PATH --markdown PATH --summary PATH`. The config must be a JSON object with exactly `date` (a Gregorian date written `YYYY-MM-DD`) and `category_order` (a non-empty array of unique non-empty strings). Each change file must be named `<SemVer>.json`; its JSON object must contain exactly non-empty string fields `id`, `category`, and `text`. The version is taken from the filename stem. Duplicate IDs are rejected across the entire input directory. Versions use SemVer 2.0.0 syntax and precedence; build metadata does not affect precedence. For equal precedence, sort the original version strings in ascending ASCII bytewise order. Reject malformed versions and records whose category is absent from `category_order`. Validate records, group entries by category, sort newest-first and entries by ID, and write Markdown release notes. In change text, prefix every ASCII punctuation character with a backslash. This means characters in code-point ranges U+0021–U+002F, U+003A–U+0040, U+005B–U+0060, and U+007B–U+007E; escape an existing backslash exactly once. Do not alter letters, digits, or spaces. Emit only configured categories containing entries. Write a JSON summary shaped as `{"releases":[{"version":"1.2.0","entry_count":1}]}` with releases in the same order. Invalid records identify the source file and field and leave both existing outputs untouched. Each output is atomically replaced after validation; two separate paths are not a crash-atomic transaction.

The Markdown heading format is `# Release notes (DATE)`, then for each version `## VERSION`, then each configured category as `### CATEGORY`, followed by `- TEXT` entries. Category names and change text must be non-empty single-line strings; reject either carriage return (CR) or line feed (LF) in either field. Separate headings/entries with one blank line and end the file with one newline.

**Follow-up change:** Add `--since VERSION`; include only releases newer than that version while retaining the configured category order and report format. Validate every input filename, record, category, and ID across the entire directory before filtering rendered releases; malformed excluded records and duplicate IDs across the filter boundary still fail while preserving both outputs. Invalid version syntax must be a clear error.

**Acceptance:** Multiple files and categories; duplicate IDs; malformed dates and filename versions; prerelease before its final version; build-metadata precedence ties; unconfigured categories rejected; stable ordering independent of filesystem order; Markdown escaping for punctuation; missing `--since` compatibility; boundary versions; both existing outputs remain unchanged on input validation or rendering failure; clean-workspace delivery. Do not claim crash-atomic replacement across both output paths.

**Fairness notes:** No Git history is needed. Python may use the standard library. Record any external package and its install footprint if chosen.

# Option 1: Guion
## README.md
# Guion product profile

Guion is a private, pre-release general-purpose programming language. Programs use `.gu` source and compile to `.gub` artifacts for a Rust runtime. The core command does not require or launch Python. The current compiler and runtime are version 0.1.0 / artifact format 39; pre-1.0 source and package compatibility is not guaranteed.

The language includes typed functions and records, modules, immutable lists and maps, JSON, `Option` and `Result`, file and byte operations, CSV and timestamp parsing, JSONL streaming, SemVer parsing, Unicode operations, iCalendar parsing, TOML editing, SQLite transactions, HMAC-SHA256, byte-preserving HTTP, and bounded concurrent work. Capabilities such as file roots, exact HTTP hostnames, environment variables, and clock reads are granted by the caller. The language reference and host API in this bundle describe limits and unsupported surfaces.

Guion can create source-free packages on the targets documented in the compatibility sheet. Rust is needed to build the CLI from source; the core installed command does not need Python. Python interoperability is optional and requires a build with the bridge enabled.

The current evidence does not establish general performance superiority over Python. This choice packet contains no benchmark results or comparative implementation outcomes.

## builtin-signatures.json
{
  "schema_version": 1,
  "entries": [
    {
      "name": "Err",
      "parameters": [
        "$E"
      ],
      "result": "Result[$T, $E]",
      "operation": "MakeErr"
    },
    {
      "name": "Ok",
      "parameters": [
        "$T"
      ],
      "result": "Result[$T, $E]",
      "operation": "MakeOk"
    },
    {
      "name": "Some",
      "parameters": [
        "$T"
      ],
      "result": "Option[$T]",
      "operation": "MakeSome"
    },
    {
      "name": "args",
      "parameters": [],
      "result": "List[Text]",
      "operation": "Args"
    },
    {
      "name": "args_from",
      "parameters": [
        "Int"
      ],
      "result": "List[Text]",
      "operation": "ArgsFrom"
    },
    {
      "name": "assert",
      "parameters": [
        "Bool"
      ],
      "result": "Unit",
      "operation": "Assert"
    },
    {
      "name": "bytes_from_ints",
      "parameters": [
        "List[Int]"
      ],
      "result": "Bytes",
      "operation": "BytesFromInts"
    },
    {
      "name": "bytes_from_text",
      "parameters": [
        "Text"
      ],
      "result": "Bytes",
      "operation": "BytesFromText"
    },
    {
      "name": "bytes_join",
      "parameters": [
        "List[Bytes]"
      ],
      "result": "Bytes",
      "operation": "BytesJoin"
    },
    {
      "name": "cli_parse",
      "parameters": [
        "List[Text]",
        "List[Record[CliOption]]"
      ],
      "result": "Result[Record[CliArgs], FileError]",
      "operation": "CliParse"
    },
    {
      "name": "csv_encode",
      "parameters": [
        "List[List[Text]]"
      ],
      "result": "Result[Text, FileError]",
      "operation": "CsvEncode"
    },
    {
      "name": "csv_next",
      "parameters": [
        "FileReader"
      ],
      "result": "Result[Option[Record[CsvRow]], FileError]",
      "operation": "CsvNext"
    },
    {
      "name": "csv_open",
      "parameters": [
        "Text"
      ],
      "result": "Result[FileReader, FileError]",
      "operation": "CsvOpen"
    },
    {
      "name": "env",
      "parameters": [
        "Text"
      ],
      "result": "Option[Text]",
      "operation": "Environment"
    },
    {
      "name": "eprint",
      "parameters": [
        "Any"
      ],
      "result": "Unit",
      "operation": "PrintErr"
    },
    {
      "name": "error",
      "parameters": [
        "Result[$T, $E]"
      ],
      "result": "$E",
      "operation": "GetError"
    },
    {
      "name": "file_close",
      "parameters": [
        "FileReader"
      ],
      "result": "Result[Unit, FileError]",
      "operation": "FileClose"
    },
    {
      "name": "file_error_code",
      "parameters": [
        "FileError"
      ],
      "result": "Text",
      "operation": "FileErrorCode"
    },
    {
      "name": "file_error_column",
      "parameters": [
        "FileError"
      ],
      "result": "Int",
      "operation": "FileErrorColumn"
    },
    {
      "name": "file_error_line",
      "parameters": [
        "FileError"
      ],
      "result": "Int",
      "operation": "FileErrorLine"
    },
    {
      "name": "file_error_message",
      "parameters": [
        "FileError"
      ],
      "result": "Text",
      "operation": "FileErrorMessage"
    },
    {
      "name": "file_next_line",
      "parameters": [
        "FileReader"
      ],
      "result": "Result[Option[Record[SourceLine]], FileError]",
      "operation": "FileNextLine"
    },
    {
      "name": "file_open_lines",
      "parameters": [
        "Text"
      ],
      "result": "Result[FileReader, FileError]",
      "operation": "FileOpenLines"
    },
    {
      "name": "float64_bits",
      "parameters": [
        "Float64"
      ],
      "result": "Int",
      "operation": "Float64Bits"
    },
    {
      "name": "fs_walk",
      "parameters": [
        "Text"
      ],
      "result": "Result[List[Record[DirectoryEntry]], FileError]",
      "operation": "FsWalk"
    },
    {
      "name": "http_error_code",
      "parameters": [
        "HttpError"
      ],
      "result": "Text",
      "operation": "HttpErrorCode"
    },
    {
      "name": "http_error_message",
      "parameters": [
        "HttpError"
      ],
      "result": "Text",
      "operation": "HttpErrorMessage"
    },
    {
      "name": "http_get",
      "parameters": [
        "Text"
      ],
      "result": "Result[Text, HttpError]",
      "operation": "HttpGet"
    },
    {
      "name": "http_request",
      "parameters": [
        "Text",
        "Text",
        "Map[Text, Text]",
        "Text"
      ],
      "result": "Result[Json, HttpError]",
      "operation": "HttpRequest"
    },
    {
      "name": "http_request_bytes",
      "parameters": [
        "Text",
        "Text",
        "Map[Text, Text]",
        "Bytes",
        "Int"
      ],
      "result": "Result[Record[HttpResponse], HttpError]",
      "operation": "HttpRequestBytes"
    },
    {
      "name": "http_request_with_timeout",
      "parameters": [
        "Text",
        "Text",
        "Map[Text, Text]",
        "Text",
        "Int"
      ],
      "result": "Result[Json, HttpError]",
      "operation": "HttpRequestWithTimeout"
    },
    {
      "name": "is_ok",
      "parameters": [
        "Result[$T, $E]"
      ],
      "result": "Bool",
      "operation": "IsOk"
    },
    {
      "name": "is_some",
      "parameters": [
        "Option[$T]"
      ],
      "result": "Bool",
      "operation": "IsSome"
    },
    {
      "name": "json_array",
      "parameters": [],
      "result": "Json",
      "operation": "JsonArray"
    },
    {
      "name": "json_as_bool",
      "parameters": [
        "Json"
      ],
      "result": "Bool",
      "operation": "JsonAsBool"
    },
    {
      "name": "json_as_float",
      "parameters": [
        "Json"
      ],
      "result": "Float64",
      "operation": "JsonAsFloat"
    },
    {
      "name": "json_as_int",
      "parameters": [
        "Json"
      ],
      "result": "Int",
      "operation": "JsonAsInt"
    },
    {
      "name": "json_as_int_list",
      "parameters": [
        "Json"
      ],
      "result": "List[Int]",
      "operation": "JsonAsIntList"
    },
    {
      "name": "json_as_text",
      "parameters": [
        "Json"
      ],
      "result": "Text",
      "operation": "JsonAsText"
    },
    {
      "name": "json_bool",
      "parameters": [
        "Bool"
      ],
      "result": "Json",
      "operation": "JsonBool"
    },
    {
      "name": "json_error_code",
      "parameters": [
        "JsonError"
      ],
      "result": "Text",
      "operation": "JsonErrorCode"
    },
    {
      "name": "json_error_column",
      "parameters": [
        "JsonError"
      ],
      "result": "Int",
      "operation": "JsonErrorColumn"
    },
    {
      "name": "json_error_line",
      "parameters": [
        "JsonError"
      ],
      "result": "Int",
      "operation": "JsonErrorLine"
    },
    {
      "name": "json_error_message",
      "parameters": [
        "JsonError"
      ],
      "result": "Text",
      "operation": "JsonErrorMessage"
    },
    {
      "name": "json_float",
      "parameters": [
        "Float64"
      ],
      "result": "Json",
      "operation": "JsonFloat"
    },
    {
      "name": "json_get",
      "parameters": [
        "Json",
        "Text"
      ],
      "result": "Json",
      "operation": "JsonGet"
    },
    {
      "name": "json_has",
      "parameters": [
        "Json",
        "Text"
      ],
      "result": "Bool",
      "operation": "JsonHas"
    },
    {
      "name": "json_index",
      "parameters": [
        "Json",
        "Int"
      ],
      "result": "Json",
      "operation": "JsonIndex"
    },
    {
      "name": "json_int",
      "parameters": [
        "Int"
      ],
      "result": "Json",
      "operation": "JsonInt"
    },
    {
      "name": "json_int_column",
      "parameters": [
        "Json",
        "Text"
      ],
      "result": "List[Int]",
      "operation": "JsonIntColumn"
    },
    {
      "name": "json_is_null",
      "parameters": [
        "Json"
      ],
      "result": "Bool",
      "operation": "JsonIsNull"
    },
    {
      "name": "json_len",
      "parameters": [
        "Json"
      ],
      "result": "Int",
      "operation": "JsonLength"
    },
    {
      "name": "json_null",
      "parameters": [],
      "result": "Json",
      "operation": "JsonNull"
    },
    {
      "name": "json_object",
      "parameters": [],
      "result": "Json",
      "operation": "JsonObject"
    },
    {
      "name": "json_parse",
      "parameters": [
        "Text"
      ],
      "result": "Result[Json, JsonError]",
      "operation": "JsonParse"
    },
    {
      "name": "json_project_columns",
      "parameters": [
        "Text",
        "List[Text]",
        "List[Text]"
      ],
      "result": "Result[Json, JsonError]",
      "operation": "JsonProjectColumns"
    },
    {
      "name": "json_push",
      "parameters": [
        "Json",
        "Json"
      ],
      "result": "Json",
      "operation": "JsonPush"
    },
    {
      "name": "json_set",
      "parameters": [
        "Json",
        "Text",
        "Json"
      ],
      "result": "Json",
      "operation": "JsonSet"
    },
    {
      "name": "json_stringify",
      "parameters": [
        "Json"
      ],
      "result": "Text",
      "operation": "JsonStringify"
    },
    {
      "name": "json_text",
      "parameters": [
        "Text"
      ],
      "result": "Json",
      "operation": "JsonText"
    },
    {
      "name": "json_text_categories",
      "parameters": [
        "Json",
        "Text"
      ],
      "result": "Json",
      "operation": "JsonTextCategories"
    },
    {
      "name": "json_text_categories_values",
      "parameters": [
        "Json"
      ],
      "result": "Json",
      "operation": "JsonTextCategoriesValues"
    },
    {
      "name": "json_try_bool",
      "parameters": [
        "Json"
      ],
      "result": "Result[Bool, JsonError]",
      "operation": "JsonTryBool"
    },
    {
      "name": "json_try_float",
      "parameters": [
        "Json"
      ],
      "result": "Result[Float64, JsonError]",
      "operation": "JsonTryFloat"
    },
    {
      "name": "json_try_get",
      "parameters": [
        "Json",
        "Text"
      ],
      "result": "Result[Json, JsonError]",
      "operation": "JsonTryGet"
    },
    {
      "name": "json_try_index",
      "parameters": [
        "Json",
        "Int"
      ],
      "result": "Result[Json, JsonError]",
      "operation": "JsonTryIndex"
    },
    {
      "name": "json_try_int",
      "parameters": [
        "Json"
      ],
      "result": "Result[Int, JsonError]",
      "operation": "JsonTryInt"
    },
    {
      "name": "json_try_text",
      "parameters": [
        "Json"
      ],
      "result": "Result[Text, JsonError]",
      "operation": "JsonTryText"
    },
    {
      "name": "len",
      "parameters": [
        "Sized"
      ],
      "result": "Int",
      "operation": "Length"
    },
    {
      "name": "list_get",
      "parameters": [
        "List[$T]",
        "Int"
      ],
      "result": "Option[$T]",
      "operation": "ListGetOption"
    },
    {
      "name": "list_push",
      "parameters": [
        "List[$T]",
        "$T"
      ],
      "result": "List[$T]",
      "operation": "ListPush"
    },
    {
      "name": "list_set",
      "parameters": [
        "List[$T]",
        "Int",
        "$T"
      ],
      "result": "List[$T]",
      "operation": "ListSet"
    },
    {
      "name": "map_get",
      "parameters": [
        "Map[Text, $T]",
        "Text"
      ],
      "result": "Option[$T]",
      "operation": "MapGetOption"
    },
    {
      "name": "map_has",
      "parameters": [
        "Map[$T]",
        "Text"
      ],
      "result": "Bool",
      "operation": "MapHas"
    },
    {
      "name": "map_set",
      "parameters": [
        "Map[$T]",
        "Text",
        "$T"
      ],
      "result": "Map[$T]",
      "operation": "MapSet"
    },
    {
      "name": "native_calendar_parse",
      "parameters": [
        "Text"
      ],
      "result": "Result[List[Record[CalendarEvent]], Record[StdError]]",
      "operation": "CalendarParse"
    },
    {
      "name": "native_crypto_hmac_sha256",
      "parameters": [
        "Bytes",
        "Bytes"
      ],
      "result": "Bytes",
      "operation": "CryptoHmacSha256"
    },
    {
      "name": "native_crypto_verify_hmac_sha256",
      "parameters": [
        "Bytes",
        "Bytes",
        "Bytes"
      ],
      "result": "Bool",
      "operation": "CryptoVerifyHmacSha256"
    },
    {
      "name": "native_sqlite_begin",
      "parameters": [
        "Db"
      ],
      "result": "Result[Transaction, Record[StdError]]",
      "operation": "SqliteBegin"
    },
    {
      "name": "native_sqlite_close",
      "parameters": [
        "Db"
      ],
      "result": "Result[Unit, Record[StdError]]",
      "operation": "SqliteClose"
    },
    {
      "name": "native_sqlite_commit",
      "parameters": [
        "Transaction"
      ],
      "result": "Result[Unit, Record[StdError]]",
      "operation": "SqliteCommit"
    },
    {
      "name": "native_sqlite_execute",
      "parameters": [
        "Transaction",
        "Text",
        "List[Record[SqlValue]]"
      ],
      "result": "Result[Int, Record[StdError]]",
      "operation": "SqliteExecute"
    },
    {
      "name": "native_sqlite_open",
      "parameters": [
        "Text"
      ],
      "result": "Result[Db, Record[StdError]]",
      "operation": "SqliteOpen"
    },
    {
      "name": "native_sqlite_query",
      "parameters": [
        "Transaction",
        "Text",
        "List[Record[SqlValue]]"
      ],
      "result": "Result[List[Record[SqlRow]], Record[StdError]]",
      "operation": "SqliteQuery"
    },
    {
      "name": "native_sqlite_rollback",
      "parameters": [
        "Transaction"
      ],
      "result": "Result[Unit, Record[StdError]]",
      "operation": "SqliteRollback"
    },
    {
      "name": "native_text_casefold",
      "parameters": [
        "Text"
      ],
      "result": "Text",
      "operation": "TextCaseFold"
    },
    {
      "name": "native_text_collapse_whitespace",
      "parameters": [
        "Text"
      ],
      "result": "Text",
      "operation": "TextCollapseWhitespace"
    },
    {
      "name": "native_text_normalize",
      "parameters": [
        "Text",
        "Text"
      ],
      "result": "Result[Text, Record[StdError]]",
      "operation": "TextNormalize"
    },
    {
      "name": "native_toml_get_bool",
      "parameters": [
        "TomlDocument",
        "List[Text]"
      ],
      "result": "Result[Bool, Record[StdError]]",
      "operation": "TomlGetBool"
    },
    {
      "name": "native_toml_get_int",
      "parameters": [
        "TomlDocument",
        "List[Text]"
      ],
      "result": "Result[Int, Record[StdError]]",
      "operation": "TomlGetInt"
    },
    {
      "name": "native_toml_get_text",
      "parameters": [
        "TomlDocument",
        "List[Text]"
      ],
      "result": "Result[Text, Record[StdError]]",
      "operation": "TomlGetText"
    },
    {
      "name": "native_toml_has",
      "parameters": [
        "TomlDocument",
        "List[Text]"
      ],
      "result": "Bool",
      "operation": "TomlHas"
    },
    {
      "name": "native_toml_parse",
      "parameters": [
        "Text"
      ],
      "result": "Result[TomlDocument, Record[StdError]]",
      "operation": "TomlParse"
    },
    {
      "name": "native_toml_put_bool",
      "parameters": [
        "TomlDocument",
        "List[Text]",
        "Bool"
      ],
      "result": "Result[Unit, Record[StdError]]",
      "operation": "TomlPutBool"
    },
    {
      "name": "native_toml_put_int",
      "parameters": [
        "TomlDocument",
        "List[Text]",
        "Int"
      ],
      "result": "Result[Unit, Record[StdError]]",
      "operation": "TomlPutInt"
    },
    {
      "name": "native_toml_put_text",
      "parameters": [
        "TomlDocument",
        "List[Text]",
        "Text"
      ],
      "result": "Result[Unit, Record[StdError]]",
      "operation": "TomlPutText"
    },
    {
      "name": "native_toml_render",
      "parameters": [
        "TomlDocument"
      ],
      "result": "Text",
      "operation": "TomlRender"
    },
    {
      "name": "parallel_map",
      "parameters": [
        "List[$T]",
        "NamedWorker[$T, $R]",
        "Int"
      ],
      "result": "Result[List[$R], TaskError]",
      "operation": "ParallelMap"
    },
    {
      "name": "print",
      "parameters": [
        "Any"
      ],
      "result": "Unit",
      "operation": "Print"
    },
    {
      "name": "read_bytes",
      "parameters": [
        "Text"
      ],
      "result": "Result[Bytes, FileError]",
      "operation": "ReadBytes"
    },
    {
      "name": "read_text",
      "parameters": [
        "Text"
      ],
      "result": "Result[Text, FileError]",
      "operation": "ReadText"
    },
    {
      "name": "semver_compare",
      "parameters": [
        "Record[SemVer]",
        "Record[SemVer]"
      ],
      "result": "Int",
      "operation": "SemVerCompare"
    },
    {
      "name": "semver_parse",
      "parameters": [
        "Text"
      ],
      "result": "Result[Record[SemVer], FileError]",
      "operation": "SemVerParse"
    },
    {
      "name": "sort_by",
      "parameters": [
        "List[$T]",
        "NamedComparator[$T]"
      ],
      "result": "List[$T]",
      "operation": "SortBy"
    },
    {
      "name": "sqrt",
      "parameters": [
        "Float64"
      ],
      "result": "Float64",
      "operation": "FloatSqrt"
    },
    {
      "name": "task_code",
      "parameters": [
        "TaskError"
      ],
      "result": "Text",
      "operation": "TaskCode"
    },
    {
      "name": "task_index",
      "parameters": [
        "TaskError"
      ],
      "result": "Int",
      "operation": "TaskIndex"
    },
    {
      "name": "task_message",
      "parameters": [
        "TaskError"
      ],
      "result": "Text",
      "operation": "TaskMessage"
    },
    {
      "name": "text_chars",
      "parameters": [
        "Text"
      ],
      "result": "List[Text]",
      "operation": "TextChars"
    },
    {
      "name": "text_parse_float",
      "parameters": [
        "Text"
      ],
      "result": "Result[Float64, FileError]",
      "operation": "TextParseFloat"
    },
    {
      "name": "text_parse_int",
      "parameters": [
        "Text"
      ],
      "result": "Result[Int, FileError]",
      "operation": "TextParseInt"
    },
    {
      "name": "time_format_utc",
      "parameters": [
        "Record[Instant]"
      ],
      "result": "Text",
      "operation": "TimeFormatUtc"
    },
    {
      "name": "time_parse_date",
      "parameters": [
        "Text"
      ],
      "result": "Result[Record[Date], FileError]",
      "operation": "TimeParseDate"
    },
    {
      "name": "time_parse_timestamp",
      "parameters": [
        "Text"
      ],
      "result": "Result[Record[Instant], FileError]",
      "operation": "TimeParseTimestamp"
    },
    {
      "name": "time_today_local",
      "parameters": [],
      "result": "Record[Date]",
      "operation": "TimeTodayLocal"
    },
    {
      "name": "unix_time_ms",
      "parameters": [],
      "result": "Int",
      "operation": "UnixTimeMs"
    },
    {
      "name": "unwrap",
      "parameters": [
        "Result[$T, $E]|Option[$T]"
      ],
      "result": "$T",
      "operation": "Unwrap"
    },
    {
      "name": "write_bytes",
      "parameters": [
        "Text",
        "Bytes"
      ],
      "result": "Result[Unit, FileError]",
      "operation": "WriteBytes"
    },
    {
      "name": "write_text",
      "parameters": [
        "Text",
        "Text"
      ],
      "result": "Result[Unit, FileError]",
      "operation": "WriteText"
    },
    {
      "name": "write_text_atomic",
      "parameters": [
        "Text",
        "Text"
      ],
      "result": "Result[Unit, FileError]",
      "operation": "WriteTextAtomic"
    }
  ]
}

## compatibility-and-migrations.md
# Compatibility and migration

Guion is pre-1.0. Source syntax, public APIs, bytecode, and package metadata may change before 1.0; there is no long-term compatibility promise yet.

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

## Supported artifact and package boundaries

The current compiler writes artifact format 39 and the runtime reads artifact formats 14 through 39. Current packages use schema 2; the verifier accepts schemas 1 and 2. Artifact format compatibility does not promise that the source compiler version is checked at runtime. Hashes detect accidental changes but do not authenticate the publisher.

The Guion-identity pre-release matrix passed on macOS ARM64 and Debian 12-compatible GNU/Linux ARM64 with glibc, including same-target relocation. Other operating systems, CPU architectures, Linux distributions, and libc implementations remain unverified. Consult [status](status.md) and the [pre-release readiness report](pre-release-readiness.md) before relying on a target.

## Python interoperability

The default core build does not discover, launch, or bundle Python or `uv`. The optional bridge requires a build with `--features python-interop` and uses the exact interpreter and dependencies declared by the project. New Guion configuration uses `guion.toml`; `GUION_PYTHON` is the explicit interpreter override when no managed interpreter is configured. Old Veritas project configuration is read for transition compatibility.

For an existing project, first keep a backup of its source, manifest, lock files, and application data. Run `guion doctor`, then `guion check` and the project's tests. Preserve its existing locks unless you intentionally change dependencies. A lock migration or dependency update must be explicit and reviewed before packaging. Application data migration remains the application's responsibility.

## Package and installation behavior

`guion package PROJECT --output DIRECTORY` builds a current-host application bundle. `guion verify-package DIRECTORY` checks its recorded hashes. `guion install DIRECTORY [--prefix PATH]` installs a verified bundle under the chosen prefix, defaulting to `~/.local`; it does not edit shell startup files. The current release candidate has not been publicly published, so these commands are development interfaces, not a public installation channel.

## guion-profile.md
# Guion capability profile

The reference in this bundle includes the current built-in and standard-library signatures, syntax, stable diagnostics, and five runnable examples. The host API describes permission grants, resource limits, deadlines, redirects, and cancellation behavior.

Closed capabilities carried forward from the switch-condition audit include JSONL with recoverable typed JSON results; CSV and strict date/timestamp parsing; iCalendar and timezone parsing; SQLite transactions; comment-preserving TOML edits; bounded HTTP and concurrency; byte-preserving HTTP; HMAC-SHA256 verification; Unicode normalization and case-folding; SemVer parsing; and runtime effect/integrity reporting. These are product capabilities, not comparative results.

Known limits include narrow pre-release platform support, task-specific package and ecosystem evidence, no recoverable conversion for managed Python bridge failures, no source-declared network-host requirements, and no crash-atomic transaction across multiple output files.

## guion-reference.md
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
R_ALLOCATION_LIMIT
R_ARGUMENT_INDEX
R_ARTIFACT_INVALID
R_ARTIFACT_VERSION
R_ASSERTION
R_BUILD_ID
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

<!-- Generated reference content digest: sha256:9bcaf31ac0bcfb24c9df7d39a7715bf12d674840bf6d6f014a2f10602977654f. Validated by `cargo xtask agent-reference --check`. -->

## host-api.md
# Guion host API contract

Status: executable contract for the current build. `print`, `args`, `args_from`, exact-grant environment reads, text and byte file operations, default-deny bounded HTTP requests, explicitly granted clock reads, `assert`, `len`, JSON operations, and the initial bounded `parallel_map` are implemented.

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
| clock | `unix_time_ms() -> Int`, `time_today_local() -> Date` | Explicit clock grant; `unix_time_ms` is milliseconds since Unix epoch and local date follows the host process time zone | Implemented in artifact format 39 |
| structured work | `parallel_map(items: List[T], worker: fn(T)->U, limit: Int) -> Result[List[U], TaskError]` | Bounded workers, isolated VM state, ordered outputs, joined scope | Implemented in named-function form |

`HttpError` is an implemented structured error with stable code/message and a source span; inspect it with `http_error_code` and `http_error_message`. `HttpResponse` is declared in `std:types` and contains integer `status`, lowercase text `headers`, and a byte-preserving `body`. The legacy general HTTP API uses a checked UTF-8 `Json` envelope. `Bytes` is an immutable byte-preserving value introduced in artifact format 17. `len` accepts Bytes and returns the byte count. Foreign calls are recorded as opaque effects even when declarations include types.

CLI projects grant exact HTTP hostnames in `guion.toml` under `[network].hosts`. A one-run grant can be added with `guion run --allow-http-host api.example.test`; repeated options add multiple explicit hostnames. `guion inspect --json` reports whether the entry reaches HTTP and points to the manifest key and run option. Runtime URL hosts still require an explicit grant; no arbitrary host is granted automatically.

`Json`, `JsonError`, and `FileError` are implemented value types. `json_parse`, `read_text`, `write_text`, and `write_text_atomic` return typed Results for syntax/duplicate-key and filesystem access failures; `is_ok`, `unwrap`, and generic `error` inspect them. Missing files return `R_FILE_NOT_FOUND`; permission failures return `R_FILE_PERMISSION`; other read/write failures keep their operation code. `json_error_code/message/line/column` and `file_error_code/message/line/column` expose details. File error locations point to the source host call and survive `?` propagation. `json_get`, `json_index`, `json_has`, `json_len`, `json_as_text`, `json_as_int`, `json_as_bool`, and `json_is_null` access parsed values. Artifact format 20 adds typed JSON constructors and immutable `json_push`/`json_set` updates so programs can create and persist JSON state without a Python bridge. Format 21 adds atomic text replacement. Format 22 adds `args_from(start)` for subcommand-based CLI programs. It prevents readers from observing a partially written replacement but does not coordinate concurrent read-modify-write sequences; simultaneous writers remain last-complete-rename wins. A process crash can leave the sibling temporary file, and directory-entry crash durability is not promised. Missing-key, wrong-container, conversion, and resource-limit errors still fail the invocation.

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

## language-spec.md
# Guion language contract, current executable subset

This is the current implementation contract for the approved north-star design. It defines the subset implemented in the current candidate; unsupported syntax must produce a source diagnostic instead of being ignored.

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

## Contracts for planned surfaces

These rules are design commitments for the approved build; the source compiler must continue to reject unsupported constructs. A surface becomes available only when it has parser/type/runtime and acceptance coverage.

- Arbitrary-precision integer arithmetic remains exact subject to a configured digit ceiling. Division by zero is currently a traced runtime error; a future checked division API may expose this as a typed `Result`.
- Text is UTF-8. Length and indexing count Unicode scalar values, not bytes or grapheme clusters. Indexing is zero based and out of range is an error. Byte-oriented operations use `Bytes` explicitly. Normalization is not implicit; equality compares exact scalar sequences.
- Lists and maps are immutable values. `list_push`, `list_set`, and `map_set` return updated copies; `list_set` addresses an existing zero-based index. `Map[Text, T]` is the initial map form; iteration is in Unicode scalar lexicographic key order. Updating a collection returns a new value. Records have declared named fields, exact field types, and structural equality; unknown fields are errors at construction/deserialization. Mutable sharing across workers is not supported.
- `Option[T]` has `None` and `Some(value)`; `Result[T, E]` has `Ok(value)` and `Err(error)`. Absence and failure are distinct. `Some` and `None` construct options inside a function declared to return `Option[T]`; inspect them with `is_some` and `unwrap`, which reports absence as a runtime error. `Ok(value)` and `Err(error)` construct results inside a function declared to return `Result[T, E]`. Postfix `?` unwraps an `Ok` or returns its `Err` from the current function; the error type must match the function's declared `Result` error type. A function still writes `Ok(value)` around a successful return expression. A `match` statement must cover exactly one `Some` and one `None` arm for an `Option`, or one `Ok` and one `Err` arm for a `Result`. The bound name is visible only in that arm; arm order is unrestricted. Wildcards, nested patterns, other type matches, and match expressions are not implemented. No implicit exception-to-Result conversion occurs. Current host results cover file operations, JSON parsing, and `parallel_map`; inspect them with `is_ok`, `unwrap`, and `error`.
- The executable import form is `import "relative-path.gu"`. Every imported module contributes only `pub fn` and `pub record` declarations to its direct importer; private declarations can still call their own module's private functions and direct imports. Names are imported unqualified. Ambiguous public names from multiple direct imports are rejected at the conflicting import; private declarations are qualified internally and remain module-local, even when different modules use the same helper name. Absolute paths, `.`/`..` components, missing files, root escapes and cycles fail before execution. CLI project builds resolve one module graph from the entry file and store per-file source identifiers in artifacts; diagnostics and runtime traces point to the originating module. External declarations specify a Python module/callable and typed signature; calls are dynamically validated and remain opaque effects. Calls may use positional arguments followed by named arguments matching the declaration's parameter names; positional calls to Guion functions remain positional. Omitted external parameters are passed through as omitted for Python defaults. Details of the Python wire boundary are in [python-abi.md](python-abi.md).
- Public functions and externally visible records require annotations. Locals infer from their initializer and cannot silently change type. Generics in the first implementation are limited to `List`, `Map`, `Option`, and `Result`; user-defined generic functions are not implied.

`json_parse(text) -> Result[Json, JsonError]`, `json_stringify(json) -> Text`, `json_get(json, key) -> Json`, `json_index(json, index) -> Json`, `json_has(json, key) -> Bool`, `json_len(json) -> Int`, `json_as_text/int/float/bool(json)`, and `json_is_null(json)` are available. JSON construction in artifact format 20 adds `json_null()`, `json_bool(Bool)`, `json_int(Int)`, `json_float(Float64)`, `json_text(Text)`, `json_array()`, `json_object()`, `json_push(Json, Json)`, and `json_set(Json, Text, Json)`. These APIs build immutable JSON values; push and set return updated copies and reject a value of the wrong container kind. Constructors enforce the same nesting, collection, text and integer limits as parsed JSON. `json_as_float` explicitly converts a JSON number to finite `Float64`. Malformed JSON and duplicate object keys return `Err(JsonError)` with a stable code, message, line, and byte column, inspectable through `json_error_code/message/line/column`. Use `unwrap(json_parse(...))` when invalid input should stop the program. Resource-limit failures and invalid typed access still raise runtime errors.

Artifact format 29 adds `json_int_column(rows: Json, field: Text) -> List[Int]`, `json_text_categories(rows: Json, field: Text) -> Json`, and `json_as_int_list(values: Json) -> List[Int]`. Format 30 adds `json_project_columns(source: Text, rows_path: List[Text], field_names: List[Text]) -> Result[Json, JsonError]`, which streams through a JSON document and retains only the requested row fields as column arrays, plus `json_text_categories_values(values: Json) -> Json` for stable first-seen categorization of a projected text column. The projection still validates JSON and duplicate keys throughout ignored data, checks collection, integer and nesting limits, requires an array of row objects at the selected path, and reports missing or duplicate requested fields as `JsonError`. Projected values remain JSON values; use `json_as_int_list` or normal JSON accessors to convert them. Bulk conversion and projection remove per-row source-level JSON access work, but do not imply that Guion is faster than Python for every workload.

The CLI uses the value returned from `main`: `Int` becomes the process exit status and must be between 0 and 255; `Unit` exits successfully with status 0. Runtime failures and invalid exit values produce a nonzero status. `write_text_atomic(path, content) -> Result[Unit, FileError]` is available in artifact format 21. It writes a temporary sibling file and renames it over the target, preventing readers from seeing a partial replacement. Missing reads return `R_FILE_NOT_FOUND`; permission failures return `R_FILE_PERMISSION`. Atomic replacement does not lock read-modify-write sequences, and it does not promise that a completed rename survives sudden power loss.

Host capability signatures and operational limits are recorded in [host-api.md](host-api.md). Unimplemented signatures and types are proposals, not working APIs.

## Product profiles, imports, resources and lock migration

- The default `core` profile contains the Guion compiler, runtime and shipped standard library. Core commands do not discover, install, launch or bundle Python or `uv`. Existing external declarations remain representable in source, but a core build reports a requested external call as unavailable. Python interoperability is an explicit optional `python-interop` profile and is never selected by default.
- Relative imports resolve within the project or package root. `std:<name>` resolves only to the verified, version-matched standard library shipped with the selected compiler. `pkg:<name>/<path>` resolves only through the project's dependency lock and managed package cache; it never falls back to the Guion development checkout or the caller's current directory. Import resolution preserves the existing file, depth, size and root-escape limits.
- Native resources such as `FileReader`, `Db`, `Transaction` and `TomlDocument` are opaque runtime-owned handles. Programs cannot forge, serialize, bridge or pass them to parallel workers. A handle is valid only for its creating runtime invocation and resource kind. Closed, stale or foreign handles return typed errors. The runtime closes remaining resources at invocation end and rolls back open transactions on failure or cancellation.
- Existing Python-profile `guion.lock` files remain readable and are not silently reinterpreted or overwritten. `guion update --migrate-python-lock` is the explicit migration path to the versioned profile-aware lock format. Migration validates the complete old lock and resolves the resulting graph before publishing an atomic replacement; any failure preserves the old lock and runnable environment. The migrated profile remains `python-interop` until the project explicitly selects `core` and removes dependencies that require Python interoperability.
- Core and `python-interop` profile identity, compiler identity, standard-library identity and resolved package content identities are recorded in locks and artifacts. A build that requests an unavailable profile or package fails with a diagnostic before execution; it does not substitute another compiler, standard library or dependency source.

## Initial examples

See `examples/sum.gu`. This contract intentionally prefers a small executable core that is genuinely compiled and interpreted over unimplemented promises in the parser.

## setup-guide.md
# Getting started

Guion is in private pre-release development. For now, use the source checkout; there is no public installer or release download.

## Requirements

To build from source, install Rust and Cargo, then open a terminal in the Guion repository. Cargo builds the CLI and its Rust runtime. The standard core build does not need Python.

## Create and run a project

From the repository root, create a small project, check it, run its starter test, and run the program:

```sh
cargo run --locked -p guion-cli -- new /tmp/hello-guion
cargo run --locked -p guion-cli -- check /tmp/hello-guion/src/main.gu
cargo run --locked -p guion-cli -- test /tmp/hello-guion/src/main.gu
cargo run --locked -p guion-cli -- run /tmp/hello-guion
```

The program prints `Hello, Guion!`. The generated project has this shape:

```text
hello-guion/
├── guion.toml
├── README.md
└── src/
    └── main.gu
```

`guion.toml` names the entry source file and project version. `.gu` files contain Guion source; `.gub` files are compiled artifacts. To use a prebuilt `guion` executable, run the same commands without the leading `cargo run --locked -p guion-cli --`.

## Read and change the starter

A Guion function declares its input and result types. `let` makes a value that cannot be reassigned; `var` makes a value that can be changed with `set`.

```guion
fn main() -> Int:
    let greeting = "Hello, Guion!"
    print(greeting)
    return 0
```

Edit `src/main.gu`, then repeat `check`, `test`, and `run`. To see the language's current supported syntax, types, host permissions, and limits, read the [language contract](../language/spec.md). The broader [reference](reference.md) describes CLI commands and examples.

Source runs keep a bounded compiled cache in the project’s `.guion/cache/` directory. It is safe to delete; Guion rebuilds it when source, dependencies, compiler identity, target, or run permissions change. Use `guion run --no-cache PROJECT` to force compilation, or `guion cache clean PROJECT` to remove Guion’s cached source artifact.

## Host permissions

File access is limited to the project or artifact directory unless an extra root is explicitly granted. Network access is denied by default; a project must grant exact hostnames in `guion.toml`. Clock access also requires a grant. These limits help keep programs predictable, but they are not operating-system security sandboxes.

For the optional Python bridge, the CLI must be built with the `python-interop` feature. The bridge is not required for core Guion programs. See [compatibility and migration](compatibility-and-migrations.md) before using a project inherited from Veritas.

## Useful next steps

- Run `guion fmt FILE.gu` to format a source file.
- Run `guion lint FILE.gu` to check for unused imports and locals and unreachable statements.
- Run `guion build FILE.gu` to create a `.gub` artifact.
- `guion doctor` reports the current host and enabled profile in human-readable form. `guion doctor --json` emits the versioned report for the active build profile; the [agent index](agents/index.json) lists both report shapes.
- Use `guion package PROJECT --output DIRECTORY` to create a current-host application bundle.

The CLI command list and behavior are in the [reference](reference.md). Release packaging is still a pre-release gate; do not treat these development commands as a public download or support promise.

# Option 2: Python
## python-profile.md
# Python product profile

This campaign profile uses CPython 3.14.8 and the `python3` command. Python is a mature, general-purpose language with a broad standard library and package ecosystem. A task can use the standard library; any third-party package must be named in the task materials and available to the same evaluation environment. This is a language-choice profile, not a performance claim or a restriction on Python deployments outside this campaign.
