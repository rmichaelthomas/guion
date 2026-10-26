### Task 4 — Build the everyday data, text and file library

**Files:** `language/stdlib/{cli,text,collections,json,jsonl,csv,time,semver,fs}.vr`; runtime standard-library bytes/fs modules and `resources.rs`; model builtin catalog; both compilers; `language/stdlib/tests/`; new `crates/runtime/tests/stdlib_data.rs`.

**Interfaces:**

| API | Required behavior |
|---|---|
| `json_try_get(Json, Text) -> Result[Json, StdError]`; `json_try_index(Json, Int)`; `json_try_text/int/bool/float` | Wrong shape, missing member and wrong type are values; distinguish absent from null; bool is not int; retain duplicate-key rejection and exact integers |
| `file_open_lines(Text) -> Result[FileReader, StdError]`; `file_next_line(FileReader) -> Result[Option[SourceLine], StdError]`; `file_close(FileReader) -> Result[Unit, StdError]` | SourceLine has physical `number: Int`, `text: Text`; bounded streaming, strict UTF-8, CRLF handling, closed-resource errors |
| `csv_open(Text) -> Result[FileReader, StdError]`; `csv_next(FileReader) -> Result[Option[CsvRow], StdError]`; `csv_encode(List[List[Text]]) -> Result[Text, StdError]` | CsvRow has starting physical line and fields; quoted commas/newlines, doubled quotes, empty fields, LF/CRLF; malformed quoting is typed failure; applications enforce their header policy |
| `time_parse_date(Text) -> Result[Date, StdError]`; `time_parse_timestamp(Text) -> Result[Instant, StdError]`; `time_format_utc(Instant) -> Text` | Date has year/month/day; Instant has exact Unix microseconds. Timestamp profile is the briefs' strict grammar: uppercase T/Z, mandatory offset, valid Gregorian date, seconds 00–59, 1–6 fractional digits, canonical trimmed UTC output |
| `semver_parse(Text) -> Result[SemVer, StdError]`; `semver_compare(SemVer, SemVer) -> Int` | Full SemVer 2.0; record major/minor/patch, prerelease identifier list, build identifier list, original text; no machine-integer overflow or build-metadata precedence |
| `sort_by(List[T], named_compare) -> List[T]` | Builtin generic signature; named `(T,T)->Int` comparator; stable sorting, deterministic equal keys, no mutation of aliases; both compilers validate it and reject comparators with I/O/foreign/resource effects through their call graph |
| `fs_walk(Text) -> Result[List[DirectoryEntry], StdError]` | DirectoryEntry has relative path/kind; bounded traversal, no following symlink files/directories, deterministic output, caller-root policy, explicit inaccessible-file errors |
| `cli_parse(List[Text], List[CliOption]) -> Result[CliArgs, StdError]` | CliOption has name/takes_value/repeatable/required; CliArgs has `values: Map[Text,List[Text]]`, `positionals: List[Text]`; distinguish omitted and supplied-empty options, recognize `--`, reject unknown/duplicate forbidden options |

Common text APIs must include scalar and byte comparison, split/join/replace/trim, strict numeric conversion, and list/map lookup returning Option. Implement reusable JSONL iteration over SourceLine and safe JSON access; do not force callers to collect the whole file. JSONL event policy belongs in applications.

- [ ] Add data-driven conformance cases for CSV multiline records, Unicode boundaries, malformed JSON shapes, retained physical line numbers, explicit empty CLI values, leap years, offset-equivalent instants, fractional precision, huge SemVer components and prerelease precedence.
- [ ] Introduce the bounded native-resource registry for FileReader before exposing file/CSV iteration. It validates owner, kind and liveness and closes readers on invocation completion/error. Calling a line operation on a CSV-mode reader returns a typed mode error. Task 5 extends this same registry for database and TOML handles.
- [ ] Implement portable parsing/formatting and convenience APIs in Veritas, using bounded native byte/file primitives for I/O. Return typed errors for ordinary invalid input; retain runtime failures for resource-budget exhaustion.
- [ ] Register new primitives in both compilers, verifier and VM; preserve JIT fallback semantics until optimized. Check public APIs through a second independent consumer in addition to the motivating application.
- [ ] Verify that invalid data cannot overwrite an existing output and that large files are consumed incrementally. Keep failed byte/line offsets meaningful for repair.

**Verify:** `cargo xtask stdlib --group data` passes through the installed self-hosted compile path, including one file larger than the configured input buffer and adversarial malformed rows. Compiler differential cases remain green for new primitives.

