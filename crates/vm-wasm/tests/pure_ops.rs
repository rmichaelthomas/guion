//! Pure host operations in the browser VM. Each program runs through the wasm ABI and through the
//! native runtime, and the two must give the same transcript, error codes included. These are the
//! operations that once answered `R_HOST_UNSUPPORTED` in the browser though they need no host.

use guion_compiler::compile_project_with_roots;
use guion_model::{Artifact, Value};
use guion_runtime::{HostPolicy, Runtime};
use guion_vm_wasm::*;
use std::path::PathBuf;

fn repository() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn gub_project(name: &str, source: &str) -> Vec<u8> {
    let dir =
        std::env::temp_dir().join(format!("guion-vm-wasm-pure-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("main.gu"), source).unwrap();
    let stdlib = repository().join("language/stdlib");
    let artifact = compile_project_with_roots(
        &dir.join("main.gu"),
        &dir,
        Some(&stdlib),
        &std::collections::HashMap::new(),
    )
    .unwrap_or_else(|error| panic!("{name} does not compile: {error:?}"));
    let _ = std::fs::remove_dir_all(&dir);
    artifact.to_bytes().expect("artifact serializes")
}

#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    Text(String),
    Int(String),
    Error(String),
}

fn native(artifact: &[u8], entry: &str) -> Outcome {
    let artifact = Artifact::from_bytes(artifact).expect("artifact reads");
    let mut runtime = Runtime::new()
        .load(artifact, HostPolicy::default())
        .expect("native verifier accepts the artifact");
    match runtime.invoke(entry, Vec::new()) {
        Ok(Value::Text(text)) => Outcome::Text(text.to_string()),
        Ok(Value::Int(value)) => Outcome::Int(value.to_string()),
        Ok(other) => panic!("unexpected native value {other:?}"),
        Err(error) => Outcome::Error(error.code),
    }
}

fn browser(artifact: &[u8], entry: &str) -> Outcome {
    let request = serde_json::json!({
        "abi": {"major": 1, "minor": 0}, "entry": entry, "arguments": [],
        "capabilities": [], "limits": {"steps": 5_000_000u64, "allowRaisedSteps": true},
    });
    let response: serde_json::Value = serde_json::from_slice(&execute_artifact_request(
        artifact,
        request.to_string().as_bytes(),
    ))
    .unwrap();
    if response["ok"] == true {
        let value = &response["value"];
        match value["type"].as_str().unwrap() {
            "Text" => Outcome::Text(value["value"].as_str().unwrap().to_owned()),
            "Int" => Outcome::Int(value["value"].as_str().unwrap().to_owned()),
            other => panic!("unexpected browser value type {other}"),
        }
    } else {
        Outcome::Error(response["code"].as_str().unwrap().to_owned())
    }
}

/// Runs `entry` natively and in the browser; both must give the same outcome, and a transcript
/// must contain every expected fragment (so each case is pinned to a value, not just to agreement).
fn agree(name: &str, source: &str, entry: &str, fragments: &[&str]) {
    let artifact = gub_project(name, source);
    let native_run = native(&artifact, entry);
    let browser_run = browser(&artifact, entry);
    assert_eq!(
        browser_run, native_run,
        "{name}: browser differs from native"
    );
    let Outcome::Text(transcript) = native_run else {
        panic!("{name}: expected a transcript, got {native_run:?}");
    };
    for fragment in fragments {
        assert!(
            transcript.contains(fragment),
            "{name}: `{fragment}` missing from {transcript}"
        );
    }
}

#[test]
fn browser_file_error_accessors_and_numeric_parsing_match_native() {
    agree(
        "file-errors",
        r#"import "std:text"
fn describe(result: Result[Int, FileError]) -> Text:
    match result:
        Ok(value):
            return "ok " + int_to_text(value)
        Err(problem):
            return file_error_code(problem) + "@" + int_to_text(file_error_line(problem)) + ":" + int_to_text(file_error_column(problem)) + " " + file_error_message(problem)

fn describe_float(result: Result[Float64, FileError]) -> Text:
    match result:
        Ok(value):
            return "ok " + float_to_text(value)
        Err(problem):
            return file_error_code(problem) + " " + file_error_message(problem)

fn transcript() -> Text:
    return describe(text_parse_int("42")) + "|" + describe(text_parse_int("-17")) + "|" + describe(text_parse_int("4x")) + "|" + describe(text_parse_int("123456789012345678901234567890")) + "|" + describe_float(text_parse_float("2.5")) + "|" + describe_float(text_parse_float("nope")) + "|" + describe_float(text_parse_float("1e999"))

fn main() -> Int:
    return 0
"#,
        "transcript",
        &[
            "ok 42|ok -17|E_INT_PARSE@",
            " expected a signed base-10 integer|ok 123456789012345678901234567890|ok 2.5|",
            "E_FLOAT_PARSE expected a decimal floating-point number|E_FLOAT_PARSE floating-point value must be finite",
        ],
    );
}

#[test]
fn browser_option_lookups_csv_semver_cli_and_time_match_native() {
    agree(
        "data-library",
        r#"import "std:collections"
import "std:csv"
import "std:semver"
import "std:cli"
import "std:time"
import "std:text"

fn show_option(value: Option[Text]) -> Text:
    match value:
        Some(found):
            return "some " + found
        None:
            return "none"

fn show_date(result: Result[Date, FileError]) -> Text:
    match result:
        Ok(date):
            return int_to_text(date.year) + "-" + int_to_text(date.month) + "-" + int_to_text(date.day)
        Err(problem):
            return file_error_code(problem)

fn show_instant(result: Result[Instant, FileError]) -> Text:
    match result:
        Ok(instant):
            return format_utc(instant) + " " + format_utc_date(instant) + " " + int_to_text(instant.unix_microseconds)
        Err(problem):
            return file_error_code(problem)

fn show_version(result: Result[SemVer, FileError]) -> Text:
    match result:
        Ok(version):
            return version.major + "." + version.minor + "." + version.patch + "/" + text_join(version.prerelease, ".")
        Err(problem):
            return file_error_code(problem)

fn compare(left: Text, right: Text) -> Text:
    return int_to_text(compare_versions(unwrap(parse_version(left)), unwrap(parse_version(right))))

fn show_cli(result: Result[CliArgs, FileError]) -> Text:
    match result:
        Ok(parsed):
            return text_join(parsed.positionals, ",") + "/" + text_join(unwrap(map_get(parsed.values, "name")), ",")
        Err(problem):
            return file_error_code(problem)

fn show_csv(result: Result[Text, FileError]) -> Text:
    match result:
        Ok(encoded):
            return encoded
        Err(problem):
            return file_error_code(problem)

fn transcript() -> Text:
    let names = ["a", "b"]
    let counts = {"x": "1"}
    let options = [CliOption("name", true, true, false)]
    return show_option(list_get_text(names, 1)) + "|" + show_option(list_get_text(names, 2)) + "|" + show_option(list_get_text(names, -1)) + "|" + show_option(map_get_text(counts, "x")) + "|" + show_option(map_get_text(counts, "y")) + "\n" + show_csv(csv_encode_rows([["a", "b,c"], ["say \"hi\"", "d"]])) + "\n" + show_date(parse_date("2026-11-25")) + "|" + show_date(parse_date("2026-13-01")) + "|" + show_instant(parse_timestamp("2026-11-25T10:30:00Z")) + "|" + show_instant(parse_timestamp("nope")) + "\n" + show_version(parse_version("1.2.3-rc.1+b5")) + "|" + show_version(parse_version("1.2")) + "|" + compare("1.0.0", "1.0.0-rc.1") + "|" + compare("1.0.0", "2.0.0") + "|" + compare("1.0.0", "1.0.0+x") + "\n" + show_cli(parse_cli(["--name", "a", "p", "--name", "b"], options)) + "|" + show_cli(parse_cli(["--bogus"], options)) + "|" + time_zone_data_version()

fn main() -> Int:
    return 0
"#,
        "transcript",
        &[
            "some b|none|none|some 1|none\n",
            "a,\"b,c\"\n\"say \"\"hi\"\"\",d\n",
            "2026-11-25|",
            "|2026-11-25T10:30:00",
            "1.2.3/rc.1|",
            "|1|-1|0\n",
            "p/a,b|",
        ],
    );
}

#[test]
fn browser_core_count_is_one() {
    let artifact = gub_project(
        "core-count",
        "import \"std:process\"\nfn count() -> Int:\n    return core_count()\nfn main() -> Int:\n    return 0\n",
    );
    assert_eq!(browser(&artifact, "count"), Outcome::Int("1".to_owned()));
}

#[test]
fn browser_unicode_text_functions_match_native() {
    agree(
        "unicode-text",
        r#"import "std:text"

fn normalized(value: Text, form: Text) -> Text:
    match text_normalize(value, form):
        Ok(done):
            return int_to_text(len(done))
        Err(problem):
            return problem.code

fn transcript() -> Text:
    return text_casefold("Stra\u{df}e \u{130} SIGMA \u{3a3}") + "|" + text_collapse_whitespace("  a \t b\n\nc  ") + "|" + normalized("e\u{301}", "NFC") + normalized("\u{e9}", "NFD") + normalized("x", "NFX")

fn main() -> Int:
    return 0
"#,
        "transcript",
        &["strasse i\u{307} sigma \u{3c3}|a b c|12E_NORMALIZATION_FORM"],
    );
}

#[test]
fn browser_hmac_sha256_matches_native_and_rfc_4231() {
    agree(
        "hmac",
        r#"import "std:crypto"
import "std:encoding"

fn yes(value: Bool) -> Text:
    if value:
        return "true"
    return "false"

fn transcript() -> Text:
    let key = bytes_from_text("Jefe")
    let body = bytes_from_text("what do ya want for nothing?")
    let tag = crypto_hmac_sha256(key, body)
    let ok = crypto_verify_hmac_sha256(key, body, tag)
    let bad = crypto_verify_hmac_sha256(key, body, bytes_from_text("nope"))
    return hex_encode(tag) + "|" + yes(ok) + "|" + yes(bad)

fn main() -> Int:
    return 0
"#,
        "transcript",
        &["5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843|true|false"],
    );
}

#[test]
fn browser_gzip_and_zlib_match_native() {
    agree(
        "compress",
        r#"import "std:compress"
import "std:encoding"

fn failure(result: Result[Bytes, StdError]) -> Text:
    match result:
        Ok(data):
            return "ok"
        Err(problem):
            return problem.code

fn transcript() -> Text:
    let data = bytes_from_text("hello hello hello hello")
    let gz = gzip_compress(data)
    let zl = zlib_compress(data)
    let first = gzip_decompress(gz, 1000)
    let second = zlib_decompress(zl, 1000)
    return failure(gzip_decompress(gz, 5)) + "|" + failure(zlib_decompress(bytes_from_text("junk"), 100)) + "|" + failure(gzip_decompress(gz, -1)) + "|" + failure(first) + "|" + failure(second) + "|" + text_from_bytes_lossy(unwrap(first)) + "|" + text_from_bytes_lossy(unwrap(second))

fn main() -> Int:
    return 0
"#,
        "transcript",
        &[
            "E_COMPRESS_LIMIT|E_COMPRESS_CORRUPT|",
            "|E_COMPRESS_ARGUMENT|ok|ok|hello hello hello hello|hello hello hello hello",
        ],
    );
}

#[test]
fn browser_yaml_and_toml_match_native() {
    agree(
        "yaml-toml",
        r#"import "std:yaml"
import "std:toml"
import "std:json"
import "std:types"

fn yaml_text(source: Text) -> Text:
    match yaml_parse(source):
        Ok(value):
            return json_stringify(value)
        Err(problem):
            return problem.code + "@" + int_to_text(problem.line) + ":" + int_to_text(problem.column)

fn yes(value: Bool) -> Text:
    if value:
        return "true"
    return "false"

fn toml_text(source: Text) -> Text:
    match toml_parse(source):
        Err(problem):
            return problem.code
        Ok(document):
            let title = unwrap(toml_get_text(document, ["title"]))
            let port = unwrap(toml_get_int(document, ["server", "port"]))
            let debug = unwrap(toml_get_bool(document, ["server", "debug"]))
            let has_missing = toml_has(document, ["nope"])
            let put = toml_put_text(document, ["server", "name"], "edge")
            let more = toml_put_int(document, ["server", "workers"], 4)
            let flag = toml_put_bool(document, ["server", "tls"], true)
            return title + "|" + int_to_text(port) + "|" + yes(debug) + "|" + yes(has_missing) + "|" + toml_render(document)

fn transcript() -> Text:
    return yaml_text("a: 1\nb: [x, true]\n") + "|" + yaml_text("a: [") + "|" + toml_text("title = \"t\"\n[server]\nport = 8080\ndebug = false\n") + "|" + toml_text("= broken")

fn main() -> Int:
    return 0
"#,
        "transcript",
        &[
            "{\"a\":1,\"b\":[\"x\",true]}|YAML_SYNTAX@1:4|t|8080|false|false|title = \"t\"",
            "name = \"edge\"\nworkers = 4\ntls = true\n|E_TOML_PARSE",
        ],
    );
}

#[test]
fn browser_http_response_accessors_match_native() {
    agree(
        "http-response",
        r#"import "std:http"
import "std:json"
import "std:encoding"

fn response(body: Bytes) -> HttpResponse:
    return HttpResponse(200, {}, body)

fn show_text(result: Result[Text, HttpError]) -> Text:
    match result:
        Ok(value):
            return "text " + value
        Err(problem):
            return http_error_code(problem) + " " + http_error_message(problem)

fn show_json(result: Result[Json, StdError]) -> Text:
    match result:
        Ok(value):
            return "json " + json_stringify(value)
        Err(problem):
            return problem.code

fn transcript() -> Text:
    return show_text(http_response_text(response(bytes_from_text("hello")))) + "|" + show_text(http_response_text(response(bytes_from_ints([255])))) + "|" + show_json(http_response_json(response(bytes_from_text("{\"a\": 1}")))) + "|" + show_json(http_response_json(response(bytes_from_text("{"))))

fn main() -> Int:
    return 0
"#,
        "transcript",
        &["text hello|R_HTTP_ENCODING HTTP response body is not valid UTF-8|json {\"a\":1}|R_JSON_PARSE"],
    );
}

#[test]
fn browser_json_column_and_pairs_operations_match_native() {
    agree(
        "json-columns",
        r#"import "std:json"
import "std:text"

fn ints(values: List[Int]) -> Text:
    var out = ""
    var index = 0
    while index < len(values):
        set out = out + int_to_text(values[index]) + ","
        set index = index + 1
    return out

fn projected(source: Text) -> Text:
    match json_project_columns(source, ["rows"], ["score"]):
        Ok(value):
            return json_stringify(value)
        Err(problem):
            return json_error_code(problem)

fn transcript() -> Text:
    let rows = unwrap(json_parse("[{\"n\":9007199254740993,\"kind\":\"b\"},{\"n\":-7,\"kind\":\"a\"},{\"n\":12,\"kind\":\"b\"}]"))
    let categories = json_text_categories(rows, "kind")
    let groups = {"x": ["1", "2"], "y": ["3"]}
    return ints(json_int_column(rows, "n")) + "|" + ints(json_as_int_list(json_get(categories, "codes"))) + "|" + json_stringify(json_get(categories, "labels")) + "|" + json_stringify(json_text_categories_values(json_array())) + "|" + json_stringify_pairs(groups, "k", "v") + "|" + projected("{\"rows\":[{\"score\":1.5}]}") + "|" + projected("{")

fn main() -> Int:
    return 0
"#,
        "transcript",
        &[
            "9007199254740993,-7,12,|0,1,0,|[\"b\",\"a\"]|",
            "[{\"k\":\"x\",\"v\":\"1\"},{\"k\":\"x\",\"v\":\"2\"},{\"k\":\"y\",\"v\":\"3\"}]|",
        ],
    );
}

/// Column operations fail the same way in both VMs when the data has the wrong shape.
#[test]
fn browser_json_column_errors_match_native() {
    for (name, body, code) in [
        (
            "int-column-type",
            "len(json_int_column(unwrap(json_parse(\"[{\\\"n\\\":1.5}]\")), \"n\"))",
            "R_JSON_TYPE",
        ),
        (
            "int-list-type",
            "len(json_as_int_list(unwrap(json_parse(\"[1, 2.5]\"))))",
            "R_JSON_TYPE",
        ),
    ] {
        let artifact = gub_project(
            name,
            &format!("import \"std:json\"\nfn run() -> Int:\n    return {body}\nfn main() -> Int:\n    return 0\n"),
        );
        let expected = Outcome::Error(code.to_owned());
        assert_eq!(native(&artifact, "run"), expected, "native {name}");
        assert_eq!(browser(&artifact, "run"), expected, "browser {name}");
    }
}

/// `try_map_list` runs in the browser and gives a `TaskError` for an item that faults, so the
/// accessors on it must run there too.
#[test]
fn browser_task_error_accessors_match_native() {
    agree(
        "task-errors",
        r#"import "std:text"

fn inverse(x: Int) -> Int:
    assert(x > 0)
    return 10 + x

fn describe(result: Result[Int, TaskError]) -> Text:
    match result:
        Ok(value):
            return "ok " + int_to_text(value)
        Err(problem):
            return task_code(problem) + "@" + int_to_text(task_index(problem)) + " " + task_message(problem)

fn transcript() -> Text:
    let results = try_map_list([5, 0, 2], inverse)
    return describe(results[0]) + "|" + describe(results[1]) + "|" + describe(results[2])

fn main() -> Int:
    return 0
"#,
        "transcript",
        &["ok 15|R_ASSERTION@1 assertion failed|", "ok 12"],
    );
}

/// Everything that needs a host capability still answers `R_HOST_UNSUPPORTED` in the browser, one
/// operation per class listed in `docs/browser.md`.
#[test]
fn host_capability_operations_stay_unsupported_in_the_browser() {
    let cases: [(&str, &str); 22] = [
        ("args", "let value = args()"),
        ("env", "let value = env(\"HOME\")"),
        ("read_text", "let value = read_text(\"a\")"),
        ("write_text", "let value = write_text(\"a\", \"b\")"),
        ("fs_walk", "let value = fs_walk(\".\")"),
        ("file_open_lines", "let value = file_open_lines(\"a\")"),
        (
            "csv_read_columns",
            "let value = csv_read_columns(\"a\", {})",
        ),
        (
            "http_get",
            "let value = http_get(\"http://example.invalid\")",
        ),
        ("unix_time_ms", "let value = unix_time_ms()"),
        ("time_today_local", "let value = time_today_local()"),
        ("sleep_ms", "sleep_ms(1)"),
        (
            "crypto_random_bytes",
            "let value = native_crypto_random_bytes(4)",
        ),
        ("stdin", "let value = native_stdin_read_line()"),
        ("eprint", "eprint(\"x\")"),
        ("sqlite_open", "let value = native_sqlite_open(\"a\")"),
        ("log_append", "let value = native_log_append(\"a\", \"b\")"),
        (
            "log_default_level",
            "let value = native_log_default_level()",
        ),
        ("calendar_parse", "let value = native_calendar_parse(\"\")"),
        ("fs_exists", "let value = native_fs_exists(\"a\")"),
        (
            "process_run",
            "let value = native_process_run(\"a\", [], \"\")",
        ),
        (
            "net_tcp_listen",
            "let value = native_net_tcp_listen(\"127.0.0.1\", 0)",
        ),
        (
            "tls_connect",
            "let value = native_tls_connect(\"a\", 1, 1, \"b\")",
        ),
    ];
    for (name, body) in cases {
        let artifact = gub_project(
            &format!("unsupported-{name}"),
            &format!(
                "fn run() -> Int:\n    {body}\n    return 0\nfn main() -> Int:\n    return 0\n"
            ),
        );
        assert_eq!(
            browser(&artifact, "run"),
            Outcome::Error("R_HOST_UNSUPPORTED".to_owned()),
            "{name}"
        );
    }
}

/// Resolving a local time needs the packaged timezone library, a native C library.
#[test]
fn resolve_local_is_unsupported_in_the_browser() {
    let artifact = gub_project(
        "resolve-local",
        "import \"std:time\"\nfn run() -> Int:\n    let value = resolve_local(LocalDateTime(2026, 1, 1, 0, 0, 0, 0), \"UTC\")\n    return 0\nfn main() -> Int:\n    return 0\n",
    );
    assert_eq!(
        browser(&artifact, "run"),
        Outcome::Error("R_HOST_UNSUPPORTED".to_owned())
    );
}
