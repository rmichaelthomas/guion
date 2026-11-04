use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use guion_compiler::{compile, compile_project, compile_project_with_roots, Diagnostic};
use guion_model::{Artifact, SharedJson, Value as RuntimeValue};
use guion_runtime::{HostPolicy, Runtime};
use serde_json::Value;

struct TempProject(PathBuf);

#[test]
fn concurrent_temporary_projects_remain_distinct_with_the_same_clock_tick() {
    let projects = std::thread::scope(|scope| {
        let threads: Vec<_> = (0..24)
            .map(|_| scope.spawn(|| TempProject::with_nonce(123456789)))
            .collect();
        threads
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect::<Vec<_>>()
    });
    let paths: std::collections::HashSet<_> = projects.iter().map(|project| &project.0).collect();
    assert_eq!(paths.len(), projects.len());
    for project in &projects {
        assert!(fs::metadata(project.0.join("compiler.gub")).unwrap().len() > 0);
    }
}

#[test]
fn http_stdlib_exports_its_small_types_without_loading_time_or_json_modules() {
    let project = TempProject::new();
    project.write(
        "main.gu",
        "import \"std:http\"\n\nfn decode(response: HttpResponse) -> Result[Json, StdError]:\n    return http_response_json(response)\n\nfn main() -> Int:\n    return 0\n",
    );
    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let stdlib = repository.join("language/stdlib");
    let artifact = compile_project_with_roots(
        &project.0.join("main.gu"),
        &project.0,
        Some(&stdlib),
        &std::collections::HashMap::new(),
    )
    .expect("the typed HTTP module compiles from its own import");

    assert!(artifact.sources.iter().any(|source| source == "std:http"));
    assert!(artifact
        .sources
        .iter()
        .any(|source| source == "std:http_types"));
    assert!(!artifact.sources.iter().any(|source| source == "std:types"));
    assert!(!artifact.sources.iter().any(|source| source == "std:time"));
    assert!(!artifact.sources.iter().any(|source| source == "std:json"));

    let project_stdlib = project.0.join("stdlib");
    fs::create_dir_all(&project_stdlib).expect("create minimal test standard library");
    for module in ["http.gu", "http_types.gu"] {
        fs::copy(stdlib.join(module), project_stdlib.join(module))
            .expect("copy HTTP-only standard-library modules");
    }
    let output = run_self_host_args(&project, &["main.gu", "--stdlib-root", "stdlib"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let self_host_output = String::from_utf8_lossy(&output.stdout);
    assert!(
        self_host_output.contains("std:http_types"),
        "self-hosted compiler should load the re-exported HTTP type module: {self_host_output}"
    );
    assert!(
        !self_host_output.contains("E_TYPE_UNKNOWN"),
        "self-hosted compiler should expose HttpResponse and StdError: {self_host_output}"
    );
}

#[test]
fn self_hosted_compiler_emits_nested_typed_map_record_updates() {
    let project = TempProject::new();
    let source = "record Summary:\n    count: Int\n\nfn main() -> Int:\n    var dates: Map[Text, Summary] = {}\n    set dates = map_set(dates, \"day\", Summary(1))\n    var groups: Map[Text, Map[Text, Summary]] = {}\n    set groups = map_set(groups, \"svc\", dates)\n    set groups = map_record_set(groups, \"svc\", \"day\", \"count\", 2)\n    return map_record_get(groups, \"svc\", \"day\", \"count\")\n";
    project.write("main.gu", source);
    project.write("nested.gu", source);

    let bootstrap = project
        .compile()
        .expect("Rust compiler accepts nested map-record operations");
    assert!(bootstrap.functions[0]
        .code
        .iter()
        .any(|instruction| matches!(&instruction.op, guion_model::Op::MapRecordSet(_, _, true))));

    let output = run_self_host(&project, "main.gu");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let ir = String::from_utf8_lossy(&output.stdout);
    assert!(ir.contains("\"op\":\"MapRecordSet\""), "{ir}");
    assert!(ir.contains("\"op\":\"MapRecordGet\""), "{ir}");
    assert!(ir.contains("\"nested\":true"), "{ir}");

    let seed = Artifact::from_bytes(include_bytes!("../../../language/self_host/compiler.gub"))
        .expect("format-49 self-host seed decodes");
    let mut runtime = Runtime::new()
        .load(
            seed,
            HostPolicy::default().with_file_access(&project.0, true, false),
        )
        .expect("self-host seed loads");
    let compiled = runtime
        .invoke(
            "compiler_compile_project",
            vec![
                RuntimeValue::Text("nested.gu".into()),
                RuntimeValue::Text(source.into()),
                RuntimeValue::Text(String::new().into()),
                RuntimeValue::Json(SharedJson::from_serde(serde_json::json!({}))),
            ],
        )
        .expect("self-host compiler returns a Result");
    let bytes = match compiled {
        RuntimeValue::ResultOk(bytes) => bytes,
        other => panic!("self-host compiler rejected nested map-record source: {other:?}"),
    };
    let bytes = match *bytes {
        RuntimeValue::Bytes(bytes) => bytes,
        other => panic!("self-host compiler success must contain artifact bytes: {other:?}"),
    };
    let artifact = Artifact::from_bytes(bytes.as_ref()).expect("self-host artifact decodes");
    assert!(artifact.functions[0]
        .code
        .iter()
        .any(|instruction| matches!(&instruction.op, guion_model::Op::MapRecordSet(_, _, true))));
}

impl TempProject {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock is after Unix epoch")
            .as_nanos();
        Self::with_nonce(nonce)
    }

    fn with_nonce(nonce: u128) -> Self {
        static NEXT_PROJECT: AtomicU64 = AtomicU64::new(0);
        let sequence = NEXT_PROJECT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "guion-compiler-conformance-{}-{nonce}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("create unique temporary project");
        let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        fs::copy(
            repository.join("language/self_host/compiler.gub"),
            path.join("compiler.gub"),
        )
        .expect("copy checked self-host compiler artifact");
        Self(path)
    }

    fn write(&self, path: &str, source: &str) {
        let target = self.0.join(path);
        fs::create_dir_all(target.parent().expect("source has a parent"))
            .expect("create source parent");
        fs::write(target, source).expect("write source");
    }

    fn compile(&self) -> Result<guion_model::Artifact, Diagnostic> {
        compile_project(&self.0.join("main.gu"), &self.0)
    }
}

impl Drop for TempProject {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn assert_self_host_diagnostic(project: &TempProject, entry: &str, source: &str, code: &str) {
    project.write(entry, source);
    let bootstrap_error = guion_compiler::compile_project(&project.0.join(entry), &project.0)
        .expect_err("bootstrap compiler must reject the invalid program");
    assert_eq!(bootstrap_error.code, code);

    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", "compiler.gub", entry])
        .current_dir(&project.0)
        .output()
        .expect("run self-host compiler");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let diagnostic = String::from_utf8_lossy(&output.stdout);
    assert!(diagnostic.contains(code), "expected {code}: {diagnostic}");
    assert!(!String::from_utf8_lossy(&output.stderr).contains("R_ASSERTION"));
}

fn install_self_host_compiler(project: &TempProject) {
    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let self_host = repository.join("language/self_host");
    for filename in [
        "guion.toml",
        "compiler.gu",
        "lexer.gu",
        "parser.gu",
        "builtin_catalog.gu",
        "artifact.gu",
    ] {
        fs::copy(self_host.join(filename), project.0.join(filename))
            .expect("copy self-host compiler source");
    }
}

fn run_self_host(project: &TempProject, entry: &str) -> std::process::Output {
    run_self_host_args(project, &[entry])
}

fn run_self_host_args(project: &TempProject, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", "compiler.gub"])
        .args(args)
        .current_dir(&project.0)
        .output()
        .expect("run self-host compiler")
}

fn catalog_example(expression_type: &str) -> Option<String> {
    match expression_type.replace(' ', "").as_str() {
        "Int" => Some("1".into()),
        "Float64" => Some("1.0".into()),
        "Bool" => Some("true".into()),
        "Text" => Some("\"value\"".into()),
        "Bytes" => Some("unwrap(read_bytes(\"value\"))".into()),
        "Unit" => Some("()".into()),
        "Json" => Some("json_null()".into()),
        "JsonError" => Some("error(json_parse(\"\"))".into()),
        "FileError" => Some("error(read_text(\"value\"))".into()),
        "HttpError" => Some("error(http_get(\"https://example.invalid\"))".into()),
        "TaskError" => Some("error(parallel_map([1], worker, 1))".into()),
        "List[Int]" => Some("[1]".into()),
        "List[Text]" => Some("[\"value\"]".into()),
        "Map[Text,Int]" => Some("{\"key\": 1}".into()),
        "Map[Text,Text]" => Some("{\"key\": \"value\"}".into()),
        _ => None,
    }
}

fn catalog_wrong_example(expression_type: &str) -> String {
    match expression_type.replace(' ', "").as_str() {
        "Int" => "\"not-an-int\"".into(),
        "Text" => "1".into(),
        _ => "0".into(),
    }
}

fn catalog_language_type(type_name: &str) -> String {
    let mut result = type_name.to_owned();
    while let Some(start) = result.find("Record[") {
        let inner_start = start + "Record[".len();
        let mut depth = 0usize;
        let mut closing = None;
        for (offset, character) in result[inner_start..].char_indices() {
            match character {
                '[' => depth += 1,
                ']' if depth == 0 => {
                    closing = Some(inner_start + offset);
                    break;
                }
                ']' => depth -= 1,
                _ => {}
            }
        }
        let Some(closing) = closing else { break };
        result.remove(closing);
        result.replace_range(start..inner_start, "");
    }
    result
}

#[test]
fn rejects_the_current_type_and_control_flow_failures_with_stable_codes() {
    let cases = [
        (
            "let mutation",
            "fn main() -> Int:\n    let value = 1\n    set value = 2\n    return value\n",
            "E_ASSIGN_IMMUTABLE",
        ),
        (
            "assignment type",
            "fn main() -> Int:\n    var value: Int = 1\n    set value = \"two\"\n    return value\n",
            "E_TYPE_ASSIGN",
        ),
        (
            "return type",
            "fn main() -> Int:\n    return \"one\"\n",
            "E_TYPE_RETURN",
        ),
        (
            "argument type",
            "fn take(value: Int) -> Int:\n    return value\n\nfn main() -> Int:\n    return take(\"one\")\n",
            "E_TYPE_ARGUMENT",
        ),
        (
            "missing return",
            "fn main() -> Int:\n    let value = 1\n",
            "E_RETURN_MISSING",
        ),
        (
            "condition type",
            "fn main() -> Int:\n    if 1:\n        return 1\n    else:\n        return 2\n",
            "E_TYPE_CONDITION",
        ),
        (
            "record field",
            "record Point:\n    x: Int\n    y: Int\n\nfn main() -> Int:\n    let point = Point(1, 2)\n    return point.z\n",
            "E_FIELD_UNKNOWN",
        ),
        (
            "generic key type",
            "fn main() -> Int:\n    let values: Map[Int, Text] = {}\n    return 0\n",
            "E_TYPE_MAP_KEY",
        ),
        (
            "incomplete match",
            "fn maybe() -> Option[Int]:\n    return Some(1)\n\nfn main() -> Int:\n    let value = maybe()\n    match value:\n        Some(number):\n            return number\n",
            "E_MATCH_NONEXHAUSTIVE",
        ),
        (
            "trailing expression tokens",
            "fn main() -> Int:\n    return 1 trailing\n",
            "E_SYNTAX",
        ),
        (
            "malformed unicode escape",
            r#"fn main() -> Int:
    return len("\u{}")
"#,
            "E_STRING_ESCAPE",
        ),
        (
            "surrogate unicode escape",
            r#"fn main() -> Int:
    return len("\u{D800}")
"#,
            "E_STRING_ESCAPE",
        ),
        (
            "out-of-range unicode escape",
            r#"fn main() -> Int:
    return len("\u{110000}")
"#,
            "E_STRING_ESCAPE",
        ),
    ];

    for (label, source, expected_code) in cases {
        let error = match compile("conformance.gu", source) {
            Err(error) => error,
            Ok(_) => panic!("{label} must be rejected"),
        };
        assert_eq!(error.code, expected_code, "{label}: {error:?}");
        assert_eq!(
            error.source_name.as_deref(),
            Some("conformance.gu"),
            "{label}"
        );
        assert!(error.span.line > 0, "{label} must identify a source line");
    }
}

#[test]
fn python_syntax_recovery_cases_match_bootstrap_and_self_hosted_compilers() {
    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let case_file = repository.join("language/conformance/python-recovery-cases.json");
    let corpus: Value =
        serde_json::from_slice(&fs::read(&case_file).expect("read recovery corpus"))
            .expect("parse recovery corpus");
    assert_eq!(corpus["schema_version"], 1);
    let cases = corpus["cases"].as_array().expect("recovery cases");
    assert_eq!(cases.len(), 11);

    let binary = PathBuf::from(env!("CARGO_BIN_EXE_guion"));
    for case in cases {
        let id = case["id"].as_str().expect("case ID");
        let project = TempProject::new();
        project.write("helpers.gu", "pub fn answer() -> Int:\n    return 42\n");
        project.write("main.gu", case["source"].as_str().expect("source"));

        let bootstrap = project.compile();
        let expected_code = case["diagnostic_code"].as_str();
        match expected_code {
            Some(expected_code) => {
                let error =
                    bootstrap.expect_err("Python syntax must receive a recovery diagnostic");
                assert_eq!(error.code, expected_code, "{id}: {error:?}");
                let expected_message = case["message_contains"]
                    .as_str()
                    .expect("diagnostic message substring");
                assert!(
                    error.message.contains(expected_message),
                    "{id}: expected message containing {expected_message:?}, got {:?}",
                    error.message
                );

                let output = Command::new(&binary)
                    .args(["run", "--json", "--file-root"])
                    .arg(&project.0)
                    .arg(project.0.join("compiler.gub"))
                    .arg("main.gu")
                    .current_dir(&project.0)
                    .output()
                    .expect("run self-hosted compiler");
                assert!(
                    output.status.success(),
                    "{id}: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
                let report: Value = serde_json::from_slice(&output.stdout)
                    .unwrap_or_else(|error| panic!("{id}: invalid self-host report: {error}"));
                assert_eq!(report["exit_code"], 0, "{id}: {report}");
                let compiler_output = report["program_stdout"].as_str().expect("compiler output");
                let compiler_result: Value =
                    serde_json::from_str(compiler_output).unwrap_or_else(|error| {
                        panic!("{id}: invalid compiler result: {error}: {compiler_output}")
                    });
                let diagnostic = compiler_result
                    .get("diagnostic")
                    .unwrap_or_else(|| panic!("{id}: no self-host diagnostic: {compiler_result}"));
                assert_eq!(diagnostic["code"], expected_code, "{id}: {diagnostic}");
                assert!(
                    diagnostic["message"].as_str().unwrap_or_default().contains(expected_message),
                    "{id}: expected self-host message containing {expected_message:?}, got {diagnostic}"
                );
            }
            None => {
                bootstrap.expect("None is already the native empty Option constructor");
                let output = Command::new(&binary)
                    .args(["run", "--json", "--file-root"])
                    .arg(&project.0)
                    .arg(project.0.join("compiler.gub"))
                    .arg("main.gu")
                    .current_dir(&project.0)
                    .output()
                    .expect("run self-hosted compiler");
                assert!(
                    output.status.success(),
                    "{id}: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
                let report: Value =
                    serde_json::from_slice(&output.stdout).expect("self-host report JSON");
                let compiler_output = report["program_stdout"].as_str().expect("compiler output");
                let compiler_result: Value =
                    serde_json::from_str(compiler_output).expect("compiler result JSON");
                assert!(
                    compiler_result.get("diagnostic").is_none(),
                    "{id}: {compiler_result}"
                );
            }
        }

        if let Some(replacement) = case["replacement"].as_str() {
            project.write("main.gu", replacement);
            project.compile().unwrap_or_else(|error| {
                panic!("{id}: documented replacement does not compile: {error:?}")
            });
            let checked = Command::new(&binary)
                .args(["check", "main.gu"])
                .current_dir(&project.0)
                .output()
                .expect("run guion check on documented replacement");
            assert!(
                checked.status.success(),
                "{id}: replacement failed `guion check`: {}{}",
                String::from_utf8_lossy(&checked.stdout),
                String::from_utf8_lossy(&checked.stderr)
            );
        }
    }
}

#[test]
fn self_host_compiler_accepts_unicode_scalar_escapes() {
    let project = TempProject::new();
    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let self_host = repository.join("language/self_host");
    for filename in [
        "guion.toml",
        "compiler.gu",
        "lexer.gu",
        "parser.gu",
        "builtin_catalog.gu",
        "artifact.gu",
    ] {
        fs::copy(self_host.join(filename), project.0.join(filename))
            .expect("copy self-host compiler source");
    }
    project.write(
        "unicode.gu",
        r#"fn main() -> Int:
    print("\u{1F642}")
    return 0
"#,
    );

    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", "compiler.gub", "unicode.gu"])
        .current_dir(&project.0)
        .output()
        .expect("run self-host compiler");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("🙂"),
        "compiled IR should contain the decoded Unicode scalar"
    );
}

#[test]
fn self_host_lexer_preserves_unicode_escape_diagnostic_code_and_span() {
    let project = TempProject::new();
    install_self_host_compiler(&project);
    assert_self_host_diagnostic(
        &project,
        "invalid-escape.gu",
        r#"fn main() -> Int:
    return len("\u{D800}")
"#,
        "E_STRING_ESCAPE",
    );
}

#[test]
fn self_host_duplicate_map_keys_use_the_stable_duplicate_key_code() {
    let project = TempProject::new();
    install_self_host_compiler(&project);
    assert_self_host_diagnostic(
        &project,
        "duplicate-map-key.gu",
        "fn main() -> Int:\n    let values = {\"answer\": 1, \"answer\": 2}\n    return len(values)\n",
        "E_MAP_DUPLICATE_KEY",
    );
}

#[test]
fn self_host_compiler_lowers_list_for_loops() {
    let project = TempProject::new();
    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let self_host = repository.join("language/self_host");
    for filename in [
        "guion.toml",
        "compiler.gu",
        "lexer.gu",
        "parser.gu",
        "builtin_catalog.gu",
        "artifact.gu",
    ] {
        fs::copy(self_host.join(filename), project.0.join(filename))
            .expect("copy self-host compiler source");
    }
    let source = "fn sum(values: List[Int]) -> Int:\n    var total = 0\n    for item in values:\n        set total = total + item\n    return total\n\nfn key_width(values: Map[Text, Int]) -> Int:\n    var total = 0\n    for key in values:\n        set total = total + len(key) + values[key]\n    return total\n\nfn empty_collections() -> Int:\n    let numbers: List[Int] = []\n    let values: Map[Text, Int] = {}\n    return len(numbers) + len(values)\n\nfn main() -> Int:\n    return sum([20, 22]) + key_width({\"ab\": 1}) + empty_collections()\n";
    let bootstrap =
        compile("loops.gu", source).expect("bootstrap compiler accepts list and map loops");
    let bootstrap_ir = format!("{:?}", bootstrap.functions);
    assert!(
        bootstrap_ir.contains("MapKeys"),
        "bootstrap compiler must lower map iteration"
    );
    assert!(
        bootstrap_ir.contains("Length"),
        "bootstrap compiler must lower list iteration"
    );
    project.write("loops.gu", source);

    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", "compiler.gub", "loops.gu"])
        .current_dir(&project.0)
        .output()
        .expect("run self-host compiler");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let ir = String::from_utf8_lossy(&output.stdout);
    assert!(
        ir.contains("\"op\":\"Length\""),
        "loop must inspect list length: {ir}"
    );
    assert!(
        ir.contains("\"op\":\"Index\""),
        "loop must read each list item: {ir}"
    );
    assert!(
        ir.contains("\"op\":\"JumpIfFalse\""),
        "loop must exit at its end: {ir}"
    );
    assert!(
        ir.contains("\"op\":\"MapKeys\""),
        "map iteration must visit its keys: {ir}"
    );
    assert!(
        ir.contains("\"op\":\"MakeMap\""),
        "map literals must build a map: {ir}"
    );
    assert!(
        ir.contains("\"op\":\"MakeTypedList\""),
        "empty lists must inherit their annotation: {ir}"
    );
    assert!(
        ir.contains("\"op\":\"MakeTypedMap\""),
        "empty maps must inherit their annotation: {ir}"
    );
}

#[test]
fn self_host_compiler_lowers_exhaustive_option_match() {
    let project = TempProject::new();
    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let self_host = repository.join("language/self_host");
    for filename in [
        "guion.toml",
        "compiler.gu",
        "lexer.gu",
        "parser.gu",
        "builtin_catalog.gu",
        "artifact.gu",
    ] {
        fs::copy(self_host.join(filename), project.0.join(filename))
            .expect("copy self-host compiler source");
    }
    let source = "fn choose(value: Option[Int]) -> Int:\n    match value:\n        Some(number):\n            return number\n        None:\n            return 0\n\nfn forty_two() -> Option[Int]:\n    return Some(42)\n\nfn main() -> Int:\n    return choose(forty_two())\n";
    compile("match.gu", source).expect("bootstrap compiler accepts exhaustive Option match");
    project.write("match.gu", source);

    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", "compiler.gub", "match.gu"])
        .current_dir(&project.0)
        .output()
        .expect("run self-host compiler");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let ir = String::from_utf8_lossy(&output.stdout);
    assert!(
        ir.contains("\"op\":\"IsSome\""),
        "match must discriminate Option values: {ir}"
    );
    assert!(
        ir.contains("\"op\":\"Unwrap\""),
        "Some bindings must unwrap their payload: {ir}"
    );
    assert!(
        ir.contains("\"op\":\"JumpIfFalse\""),
        "match must select an arm: {ir}"
    );
}

#[test]
fn self_host_compiler_lowers_result_match_and_propagation() {
    let project = TempProject::new();
    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let self_host = repository.join("language/self_host");
    for filename in [
        "guion.toml",
        "compiler.gu",
        "lexer.gu",
        "parser.gu",
        "builtin_catalog.gu",
        "artifact.gu",
    ] {
        fs::copy(self_host.join(filename), project.0.join(filename))
            .expect("copy self-host compiler source");
    }
    let source = "fn source_result() -> Result[List[Int], Text]:\n    return Ok([])\n\nfn propagate() -> Result[List[Int], Text]:\n    let value = source_result()?\n    return Ok(value)\n\nfn main() -> Int:\n    match propagate():\n        Ok(values):\n            return len(values)\n        Err(message):\n            return len(message)\n";
    compile("result.gu", source).expect("bootstrap compiler accepts Result propagation and match");
    project.write("result.gu", source);

    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", "compiler.gub", "result.gu"])
        .current_dir(&project.0)
        .output()
        .expect("run self-host compiler");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let ir = String::from_utf8_lossy(&output.stdout);
    assert!(
        ir.contains("\"op\":\"IsOk\""),
        "Result match must discriminate variants: {ir}"
    );
    assert!(
        ir.contains("\"op\":\"GetError\""),
        "Err bindings must extract the error: {ir}"
    );
    assert!(
        ir.contains("\"op\":\"TryResult\""),
        "postfix ? must propagate errors: {ir}"
    );
    assert!(
        ir.contains("\"op\":\"MakeOk\""),
        "Ok constructors must be emitted: {ir}"
    );
    assert!(
        ir.contains("\"op\":\"MakeTypedList\""),
        "Ok must pass its expected payload type into empty lists: {ir}"
    );
}

#[test]
fn self_host_compiler_emits_float64_literals() {
    let project = TempProject::new();
    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let self_host = repository.join("language/self_host");
    for filename in [
        "guion.toml",
        "compiler.gu",
        "lexer.gu",
        "parser.gu",
        "builtin_catalog.gu",
        "artifact.gu",
    ] {
        fs::copy(self_host.join(filename), project.0.join(filename))
            .expect("copy self-host compiler source");
    }
    let source = "fn calculate(value: Float64) -> Float64:\n    return value * 2.5 + 0.5\n\nfn main() -> Int:\n    if calculate(1.5) == 3.5:\n        return 0\n    else:\n        return 1\n";
    compile("float.gu", source).expect("bootstrap compiler accepts Float64 arithmetic");
    project.write("float.gu", source);

    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", "compiler.gub", "float.gu"])
        .current_dir(&project.0)
        .output()
        .expect("run self-host compiler");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let ir = String::from_utf8_lossy(&output.stdout);
    assert!(
        ir.contains("\"op\":\"ConstFloat64\""),
        "Float64 literals must retain their type: {ir}"
    );
}

#[test]
fn self_host_compiler_lowers_parallel_map_with_a_named_worker() {
    let project = TempProject::new();
    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let self_host = repository.join("language/self_host");
    for filename in [
        "guion.toml",
        "compiler.gu",
        "lexer.gu",
        "parser.gu",
        "builtin_catalog.gu",
        "artifact.gu",
    ] {
        fs::copy(self_host.join(filename), project.0.join(filename))
            .expect("copy self-host compiler source");
    }
    let source = "fn double(value: Int) -> Int:\n    return value * 2\n\nfn map_values(values: List[Int]) -> Result[List[Int], TaskError]:\n    return parallel_map(values, double, 1)\n\nfn main() -> Int:\n    match map_values([21]):\n        Ok(values):\n            return values[0]\n        Err(problem):\n            return 0\n";
    compile("parallel.gu", source).expect("bootstrap compiler accepts a named parallel worker");
    project.write("parallel.gu", source);

    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", "compiler.gub", "parallel.gu"])
        .current_dir(&project.0)
        .output()
        .expect("run self-host compiler");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let ir = String::from_utf8_lossy(&output.stdout);
    assert!(
        ir.contains("\"op\":\"ParallelMap\""),
        "parallel_map must target its named worker: {ir}"
    );
    assert!(
        ir.contains("\"function\":\"double\""),
        "parallel worker identity must be preserved: {ir}"
    );

    let ir_path = project.0.join("parallel.gui.json");
    let artifact_path = project.0.join("parallel.gub");
    fs::write(&ir_path, output.stdout).expect("save self-hosted IR");
    let assembled = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["assemble-ir", ir_path.to_str().unwrap(), "--output"])
        .arg(&artifact_path)
        .output()
        .expect("assemble self-hosted IR");
    assert!(
        assembled.status.success(),
        "{}",
        String::from_utf8_lossy(&assembled.stderr)
    );
    let run = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", artifact_path.to_str().unwrap()])
        .output()
        .expect("run parallel_map program");
    assert_eq!(run.status.code(), Some(42));
}

#[test]
fn parallel_workers_cannot_accept_or_return_native_resources() {
    let project = TempProject::new();
    let source = "fn identity(value: Db) -> Db:\n    return value\n\nfn map_databases(values: List[Db]) -> Result[List[Db], TaskError]:\n    return parallel_map(values, identity, 2)\n\nfn main() -> Int:\n    return 0\n";
    project.write("main.gu", source);
    let error = project
        .compile()
        .expect_err("native handles cannot cross worker boundaries");
    assert_eq!(error.code, "E_PARALLEL_RESOURCE");
}

#[test]
fn self_host_compiler_rejects_native_resource_parallel_workers() {
    let project = TempProject::new();
    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let self_host = repository.join("language/self_host");
    for filename in [
        "guion.toml",
        "compiler.gu",
        "lexer.gu",
        "parser.gu",
        "builtin_catalog.gu",
        "artifact.gu",
    ] {
        fs::copy(self_host.join(filename), project.0.join(filename))
            .expect("copy self-host compiler source");
    }
    assert_self_host_diagnostic(
        &project,
        "parallel-resource.gu",
        "fn identity(value: Db) -> Db:\n    return value\n\nfn map_databases(values: List[Db]) -> Result[List[Db], TaskError]:\n    return parallel_map(values, identity, 2)\n\nfn main() -> Int:\n    return 0\n",
        "E_PARALLEL_RESOURCE",
    );
}

#[test]
fn self_host_compiler_preserves_python_external_declarations() {
    let project = TempProject::new();
    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let self_host = repository.join("language/self_host");
    for filename in [
        "guion.toml",
        "compiler.gu",
        "lexer.gu",
        "parser.gu",
        "builtin_catalog.gu",
        "artifact.gu",
    ] {
        fs::copy(self_host.join(filename), project.0.join(filename))
            .expect("copy self-host compiler source");
    }
    let source = "extern python \"math:pow\" fn python_power(x: Float64, y: Float64) -> Float64\n\nfn main() -> Int:\n    let result = python_power(y=3.0, x=2.0)\n    return 0\n";
    compile("external.gu", source)
        .expect("bootstrap compiler accepts a Python external declaration");
    project.write("external.gu", source);

    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", "compiler.gub", "external.gu"])
        .current_dir(&project.0)
        .output()
        .expect("run self-host compiler");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let ir = String::from_utf8_lossy(&output.stdout);
    assert!(
        ir.contains("\"op\":\"PythonCallKw\""),
        "external keyword calls must emit PythonCallKw: {ir}"
    );
    assert!(
        ir.contains("\"keyword_names\":[\"y\",\"x\"]"),
        "external keyword order must be retained: {ir}"
    );
    assert!(
        ir.contains("\"name\":\"python_power\""),
        "external signature must be retained: {ir}"
    );
    assert!(
        ir.contains("\"module\":\"math\""),
        "external module target must be retained: {ir}"
    );
    assert!(
        ir.contains("\"callable\":\"pow\""),
        "external callable target must be retained: {ir}"
    );
    let ir_path = project.0.join("external.gui.json");
    let artifact_path = project.0.join("external.gub");
    fs::write(&ir_path, output.stdout).expect("save self-hosted external IR");
    let assembled = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["assemble-ir", ir_path.to_str().unwrap(), "--output"])
        .arg(&artifact_path)
        .output()
        .expect("assemble external IR");
    assert!(
        assembled.status.success(),
        "{}",
        String::from_utf8_lossy(&assembled.stderr)
    );
}

#[test]
fn self_host_compiler_lowers_bytes_file_operations_and_result_propagation() {
    let project = TempProject::new();
    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let self_host = repository.join("language/self_host");
    for filename in [
        "guion.toml",
        "compiler.gu",
        "lexer.gu",
        "parser.gu",
        "builtin_catalog.gu",
        "artifact.gu",
    ] {
        fs::copy(self_host.join(filename), project.0.join(filename))
            .expect("copy self-host compiler source");
    }
    let source = "fn copy_bytes(source: Text, destination: Text) -> Result[Unit, FileError]:\n    let bytes = read_bytes(source)?\n    return write_bytes(destination, bytes)\n\nfn main() -> Int:\n    return 0\n";
    compile("bytes.gu", source).expect("bootstrap compiler accepts byte file operations");
    project.write("bytes.gu", source);

    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", "compiler.gub", "bytes.gu"])
        .current_dir(&project.0)
        .output()
        .expect("run self-host compiler");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let ir = String::from_utf8_lossy(&output.stdout);
    assert!(
        ir.contains("\"op\":\"ReadBytes\""),
        "read_bytes must lower to its byte primitive: {ir}"
    );
    assert!(
        ir.contains("\"op\":\"TryResult\""),
        "byte errors must propagate with `?`: {ir}"
    );
    assert!(
        ir.contains("\"op\":\"WriteBytes\""),
        "write_bytes must lower to its byte primitive: {ir}"
    );
}

#[test]
fn self_host_compiler_lowers_remaining_public_host_builtins() {
    let project = TempProject::new();
    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let self_host = repository.join("language/self_host");
    for filename in [
        "guion.toml",
        "compiler.gu",
        "lexer.gu",
        "parser.gu",
        "builtin_catalog.gu",
        "artifact.gu",
    ] {
        fs::copy(self_host.join(filename), project.0.join(filename))
            .expect("copy self-host compiler source");
    }
    let source = r#"record HttpResponse:
    status: Int
    headers: Map[Text, Text]
    body: Bytes

fn worker(value: Int) -> Int:
    return value

fn main() -> Int:
    let floating = json_as_float(json_int(3))
    let integers: List[Int] = json_as_int_list(json_array())
    let column: List[Int] = json_int_column(json_array(), "score")
    let categories = json_text_categories(json_array(), "status")
    let projected = json_project_columns("{}", ["rows"], ["score"])
    let category_values = json_text_categories_values(categories)
    let options: Map[Text, Text] = {"accept": "application/json"}
    let request = http_request("GET", "https://example.test", options, "")
    let timed_request = http_request_with_timeout("GET", "https://example.test", options, "", 1000)
    let byte_request = http_request_bytes("GET", "https://example.test", options, bytes_from_text(""), 1000)
    let response = http_get("https://example.test")
    let http_error = error(response)
    let http_code = http_error_code(http_error)
    let task = parallel_map(integers, worker, 2)
    let task_error = error(task)
    let task_index_value = task_index(task_error)
    let task_code_value = task_code(task_error)
    let task_message_value = task_message(task_error)
    let environment = env("PATH")
    let arguments = args_from(0)
    let flags = map_set({"first": 1}, "second", 2)
    let has_first = map_has(flags, "first")
    return len(arguments) + task_index_value
"#;
    compile("builtins.gu", source)
        .expect("bootstrap compiler accepts the builtin conformance source");
    project.write("builtins.gu", source);

    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", "compiler.gub", "builtins.gu"])
        .current_dir(&project.0)
        .output()
        .expect("run self-host compiler");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let ir = String::from_utf8_lossy(&output.stdout);
    for opcode in [
        "JsonAsFloat",
        "JsonAsIntList",
        "JsonIntColumn",
        "JsonTextCategories",
        "JsonProjectColumns",
        "JsonTextCategoriesValues",
        "HttpRequest",
        "HttpRequestWithTimeout",
        "HttpRequestBytes",
        "HttpGet",
        "HttpErrorCode",
        "ParallelMap",
        "TaskIndex",
        "TaskCode",
        "TaskMessage",
        "Environment",
        "ArgsFrom",
        "MapSet",
        "MapHas",
    ] {
        assert!(ir.contains(opcode), "missing {opcode} lowering: {ir}");
    }
    assert!(!String::from_utf8_lossy(&output.stderr).contains("R_ASSERTION"));
}

#[test]
fn self_host_compiler_returns_a_source_diagnostic_for_parse_errors() {
    let project = TempProject::new();
    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let self_host = repository.join("language/self_host");
    for filename in [
        "guion.toml",
        "compiler.gu",
        "lexer.gu",
        "parser.gu",
        "builtin_catalog.gu",
        "artifact.gu",
    ] {
        fs::copy(self_host.join(filename), project.0.join(filename))
            .expect("copy self-host compiler source");
    }
    let source = "fn main() -> Int:\n    return 1 trailing\n";
    assert!(compile("invalid.gu", source).is_err());
    project.write("invalid.gu", source);

    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", "compiler.gub", "invalid.gu"])
        .current_dir(&project.0)
        .output()
        .expect("run self-host compiler");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let diagnostic = String::from_utf8_lossy(&output.stdout);
    assert!(
        diagnostic.contains("guion-compile-failure/v1"),
        "failure must be structured: {diagnostic}"
    );
    assert!(
        diagnostic.contains("E_SYNTAX"),
        "failure must carry a stable code: {diagnostic}"
    );
    assert!(
        diagnostic.contains("invalid.gu"),
        "failure must identify its source: {diagnostic}"
    );
    assert!(
        diagnostic.contains("\"line\":2"),
        "failure must identify its source line: {diagnostic}"
    );
    assert!(!String::from_utf8_lossy(&output.stderr).contains("R_ASSERTION"));
}

#[test]
fn self_host_compiler_driver_reports_source_and_argument_errors_without_assertions() {
    let project = TempProject::new();
    install_self_host_compiler(&project);

    let missing = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", "compiler.gub", "missing.gu"])
        .current_dir(&project.0)
        .output()
        .expect("run self-host compiler with a missing source");
    assert!(
        missing.status.success(),
        "{}",
        String::from_utf8_lossy(&missing.stderr)
    );
    let missing_output = String::from_utf8_lossy(&missing.stdout);
    assert!(missing_output.contains("E_SOURCE_READ"), "{missing_output}");
    assert!(!missing_output.contains("R_ASSERTION"), "{missing_output}");
    assert!(!String::from_utf8_lossy(&missing.stderr).contains("R_ASSERTION"));

    project.write("one.gu", "fn main() -> Int:\n    return 0\n");
    let extra = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", "compiler.gub", "one.gu", "two.gu"])
        .current_dir(&project.0)
        .output()
        .expect("run self-host compiler with an extra argument");
    assert!(
        extra.status.success(),
        "{}",
        String::from_utf8_lossy(&extra.stderr)
    );
    let extra_output = String::from_utf8_lossy(&extra.stdout);
    assert!(
        extra_output.contains("E_COMPILE_ARGUMENTS"),
        "{extra_output}"
    );
    assert!(!extra_output.contains("R_ASSERTION"), "{extra_output}");
    assert!(!String::from_utf8_lossy(&extra.stderr).contains("R_ASSERTION"));
}

#[test]
fn self_host_compiler_checks_entry_declarations_and_visible_types() {
    let project = TempProject::new();
    install_self_host_compiler(&project);
    assert_self_host_diagnostic(
        &project,
        "missing-main.gu",
        "fn helper() -> Int:\n    return 0\n",
        "E_ENTRY_MISSING",
    );
    assert_self_host_diagnostic(
        &project,
        "main-parameters.gu",
        "fn main(value: Int) -> Int:\n    return value\n",
        "E_ENTRY_SIGNATURE",
    );
    assert_self_host_diagnostic(
        &project,
        "main-result.gu",
        "fn main() -> Bool:\n    return true\n",
        "E_ENTRY_SIGNATURE",
    );
    assert_self_host_diagnostic(
        &project,
        "unknown-type.gu",
        "fn take(value: MissingType) -> Int:\n    return 0\n\nfn main() -> Int:\n    return 0\n",
        "E_TYPE_UNKNOWN",
    );
    project.write(
        "private_record.gu",
        "record Hidden:\n    value: Int\n\npub fn make() -> Int:\n    return 0\n",
    );
    assert_self_host_diagnostic(
        &project,
        "private-record-type.gu",
        "import \"private_record.gu\"\n\nfn accept(value: Hidden) -> Int:\n    return 0\n\nfn main() -> Int:\n    return 0\n",
        "E_TYPE_UNKNOWN",
    );
    assert_self_host_diagnostic(
        &project,
        "duplicate-functions.gu",
        "fn helper() -> Int:\n    return 1\n\nfn helper() -> Int:\n    return 2\n\nfn main() -> Int:\n    return helper()\n",
        "E_NAME_DUPLICATE",
    );
    assert_self_host_diagnostic(
        &project,
        "duplicate-fields.gu",
        "record Point:\n    x: Int\n    x: Int\n\nfn main() -> Int:\n    return 0\n",
        "E_RECORD_FIELD_DUPLICATE",
    );
    assert_self_host_diagnostic(
        &project,
        "unsupported-external-type.gu",
        "extern python \"example:call\" fn call(value: Option[Int]) -> Text\n\nfn main() -> Int:\n    return 0\n",
        "E_EXTERN_TYPE",
    );
    assert_self_host_diagnostic(
        &project,
        "duplicate-external-parameters.gu",
        "extern python \"example:call\" fn call(value: Int, value: Int) -> Int\n\nfn main() -> Int:\n    return 0\n",
        "E_NAME_DUPLICATE",
    );
    assert_self_host_diagnostic(
        &project,
        "invalid-external-target.gu",
        "extern python \"example:call..bad\" fn call() -> Int\n\nfn main() -> Int:\n    return 0\n",
        "E_EXTERN_TARGET",
    );
    assert_self_host_diagnostic(
        &project,
        "public-external.gu",
        "pub extern python \"example:call\" fn call() -> Int\n\nfn main() -> Int:\n    return 0\n",
        "E_EXTERN_PUBLIC",
    );
}

#[test]
fn self_host_compiler_returns_a_source_diagnostic_for_unknown_functions() {
    let project = TempProject::new();
    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let self_host = repository.join("language/self_host");
    for filename in [
        "guion.toml",
        "compiler.gu",
        "lexer.gu",
        "parser.gu",
        "builtin_catalog.gu",
        "artifact.gu",
    ] {
        fs::copy(self_host.join(filename), project.0.join(filename))
            .expect("copy self-host compiler source");
    }
    let source = "fn main() -> Int:\n    return missing()\n";
    assert_eq!(
        compile("invalid.gu", source).unwrap_err().code,
        "E_NAME_UNKNOWN"
    );
    project.write("invalid.gu", source);

    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", "compiler.gub", "invalid.gu"])
        .current_dir(&project.0)
        .output()
        .expect("run self-host compiler");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let diagnostic = String::from_utf8_lossy(&output.stdout);
    assert!(
        diagnostic.contains("E_NAME_UNKNOWN"),
        "unknown function must produce a diagnostic: {diagnostic}"
    );
    assert!(
        diagnostic.contains("invalid.gu"),
        "diagnostic must retain its source name: {diagnostic}"
    );
    assert!(
        diagnostic.contains("\"line\":2"),
        "diagnostic must identify its source line: {diagnostic}"
    );
    assert!(!String::from_utf8_lossy(&output.stderr).contains("R_ASSERTION"));

    let source = "fn main() -> Int:\n    return absent\n";
    assert_eq!(
        compile("invalid.gu", source).unwrap_err().code,
        "E_NAME_UNKNOWN"
    );
    project.write("invalid.gu", source);
    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", "compiler.gub", "invalid.gu"])
        .current_dir(&project.0)
        .output()
        .expect("rerun self-host compiler");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let diagnostic = String::from_utf8_lossy(&output.stdout);
    assert!(
        diagnostic.contains("E_NAME_UNKNOWN"),
        "unknown local must produce a diagnostic: {diagnostic}"
    );
    assert!(
        diagnostic.contains("\"line\":2"),
        "diagnostic must identify its source line: {diagnostic}"
    );
    assert!(!String::from_utf8_lossy(&output.stderr).contains("R_ASSERTION"));
}

#[test]
fn self_host_compiler_checks_builtin_arity_and_parallel_worker_names() {
    let project = TempProject::new();
    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let self_host = repository.join("language/self_host");
    for filename in [
        "guion.toml",
        "compiler.gu",
        "lexer.gu",
        "parser.gu",
        "builtin_catalog.gu",
        "artifact.gu",
    ] {
        fs::copy(self_host.join(filename), project.0.join(filename))
            .expect("copy self-host compiler source");
    }
    assert_self_host_diagnostic(
        &project,
        "wrong-arity.gu",
        "fn main() -> Int:\n    return len()\n",
        "E_CALL_ARITY",
    );
    assert_self_host_diagnostic(
        &project,
        "unknown-worker.gu",
        "fn main() -> Int:\n    let values: List[Int] = []\n    let result = parallel_map(values, missing_worker, 1)\n    return 0\n",
        "E_NAME_UNKNOWN",
    );
}

#[test]
fn self_host_compiler_rejects_immutable_assignment_and_duplicate_locals() {
    let project = TempProject::new();
    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let self_host = repository.join("language/self_host");
    for filename in [
        "guion.toml",
        "compiler.gu",
        "lexer.gu",
        "parser.gu",
        "builtin_catalog.gu",
        "artifact.gu",
    ] {
        fs::copy(self_host.join(filename), project.0.join(filename))
            .expect("copy self-host compiler source");
    }
    assert_self_host_diagnostic(
        &project,
        "immutable.gu",
        "fn main() -> Int:\n    let value = 1\n    set value = 2\n    return value\n",
        "E_ASSIGN_IMMUTABLE",
    );
    assert_self_host_diagnostic(
        &project,
        "duplicate.gu",
        "fn main() -> Int:\n    let value = 1\n    let value = 2\n    return value\n",
        "E_NAME_DUPLICATE",
    );
}

#[test]
fn self_host_compiler_returns_type_and_field_diagnostics() {
    let project = TempProject::new();
    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let self_host = repository.join("language/self_host");
    for filename in [
        "guion.toml",
        "compiler.gu",
        "lexer.gu",
        "parser.gu",
        "builtin_catalog.gu",
        "artifact.gu",
    ] {
        fs::copy(self_host.join(filename), project.0.join(filename))
            .expect("copy self-host compiler source");
    }
    assert_self_host_diagnostic(
        &project,
        "wrong-return.gu",
        "fn main() -> Int:\n    return \"one\"\n",
        "E_TYPE_RETURN",
    );
    assert_self_host_diagnostic(
        &project,
        "missing-return.gu",
        "fn main() -> Int:\n    let value = 1\n",
        "E_RETURN_MISSING",
    );
    assert_self_host_diagnostic(
        &project,
        "wrong-condition.gu",
        "fn main() -> Int:\n    if 1:\n        return 1\n    else:\n        return 2\n",
        "E_TYPE_CONDITION",
    );
    assert_self_host_diagnostic(
        &project,
        "wrong-argument.gu",
        "fn take(value: Int) -> Int:\n    return value\n\nfn main() -> Int:\n    return take(\"one\")\n",
        "E_TYPE_ARGUMENT",
    );
    assert_self_host_diagnostic(
        &project,
        "wrong-assignment.gu",
        "fn main() -> Int:\n    var value: Int = 1\n    set value = \"two\"\n    return value\n",
        "E_TYPE_ASSIGN",
    );
    assert_self_host_diagnostic(
        &project,
        "wrong-field.gu",
        "record Point:\n    x: Int\n\nfn main() -> Int:\n    let point = Point(1)\n    return point.y\n",
        "E_FIELD_UNKNOWN",
    );
    assert_self_host_diagnostic(
        &project,
        "wrong-builtin.gu",
        "fn main() -> Int:\n    return len(1)\n",
        "E_TYPE_ARGUMENT",
    );
    assert_self_host_diagnostic(
        &project,
        "wrong-environment.gu",
        "fn main() -> Int:\n    let value = env(1)\n    return 0\n",
        "E_TYPE_ARGUMENT",
    );
    assert_self_host_diagnostic(
        &project,
        "wrong-byte-write.gu",
        "fn main() -> Int:\n    let result = write_bytes(\"out.bin\", \"text\")\n    return 0\n",
        "E_TYPE_ARGUMENT",
    );
    assert_self_host_diagnostic(
        &project,
        "uncontextual-none.gu",
        "fn main() -> Int:\n    let value = None\n    return 0\n",
        "E_OPTION_CONSTRUCTOR_CONTEXT",
    );
    assert_self_host_diagnostic(
        &project,
        "wrong-some.gu",
        "fn maybe() -> Option[Int]:\n    return Some(\"text\")\n\nfn main() -> Int:\n    return 0\n",
        "E_OPTION_CONSTRUCTOR_TYPE",
    );
    assert_self_host_diagnostic(
        &project,
        "mixed-list.gu",
        "fn main() -> Int:\n    let values = [1, \"two\"]\n    return 0\n",
        "E_TYPE_LIST_ITEM",
    );
    assert_self_host_diagnostic(
        &project,
        "mixed-map.gu",
        "fn main() -> Int:\n    let values = {\"one\": 1, \"two\": \"two\"}\n    return 0\n",
        "E_TYPE_MAP_VALUE",
    );
    assert_self_host_diagnostic(
        &project,
        "invalid-try.gu",
        "fn main() -> Int:\n    return 1?\n",
        "E_RESULT_TRY_TYPE",
    );
}

#[test]
fn self_host_compiler_reexports_public_imported_records_and_functions() {
    let project = TempProject::new();
    project.write(
        "main.gu",
        "import \"first.gu\"\nimport \"facade.gu\"\nfn main() -> Int:\n    let token = Token(41)\n    let incremented = increment(token)\n    assert(incremented.value == 42)\n    return 0\n",
    );
    project.write(
        "first.gu",
        "import \"shared.gu\"\npub fn first_value() -> Int:\n    return 0\n",
    );
    project.write(
        "facade.gu",
        "pub import \"shared.gu\"\npub fn facade_value() -> Int:\n    return 0\n",
    );
    project.write(
        "shared.gu",
        "pub import \"leaf.gu\"\npub fn shared_value() -> Int:\n    return 0\n",
    );
    project.write(
        "leaf.gu",
        "pub record Token:\n    value: Int\n\npub fn increment(token: Token) -> Token:\n    return Token(token.value + 1)\n",
    );

    project
        .compile()
        .expect("Rust compiler accepts the public re-export project");
    let output = run_self_host(&project, "main.gu");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let artifact = String::from_utf8_lossy(&output.stdout);
    assert!(
        artifact.contains("increment"),
        "the self-host compiler should include the re-exported function: {artifact}"
    );
    assert!(
        artifact.contains("MakeRecord"),
        "the self-host compiler should make the re-exported record constructible: {artifact}"
    );

    let execution = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", "--json", "main.gu"])
        .current_dir(&project.0)
        .output()
        .expect("run the public-import program through the product CLI");
    assert!(
        execution.status.success(),
        "{}",
        String::from_utf8_lossy(&execution.stderr)
    );
    let report: Value =
        serde_json::from_slice(&execution.stdout).expect("parse public-import execution report");
    assert_eq!(report["exit_code"], 0, "{report}");
}

#[test]
fn self_host_resolver_enforces_direct_public_visibility_and_qualifies_private_helpers() {
    let project = TempProject::new();
    install_self_host_compiler(&project);

    project.write("private.gu", "fn hidden() -> Int:\n    return 7\n");
    project.write(
        "main.gu",
        "import \"private.gu\"\n\nfn main() -> Int:\n    return hidden()\n",
    );
    let private = run_self_host(&project, "main.gu");
    assert!(
        private.status.success(),
        "{}",
        String::from_utf8_lossy(&private.stderr)
    );
    assert!(String::from_utf8_lossy(&private.stdout).contains("E_NAME_UNKNOWN"));

    project.write(
        "left.gu",
        "fn helper() -> Int:\n    return 2\n\npub fn left() -> Int:\n    return helper()\n",
    );
    project.write(
        "right.gu",
        "fn helper() -> Int:\n    return 3\n\npub fn right() -> Int:\n    return helper()\n",
    );
    project.write(
        "main.gu",
        "import \"left.gu\"\nimport \"right.gu\"\n\nfn main() -> Int:\n    return left() + right()\n",
    );
    let duplicate_private = run_self_host(&project, "main.gu");
    assert!(
        duplicate_private.status.success(),
        "{}",
        String::from_utf8_lossy(&duplicate_private.stderr)
    );
    let ir: serde_json::Value = serde_json::from_slice(&duplicate_private.stdout)
        .expect("private helper project should produce compiler IR");
    let names = ir["functions"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|function| function["name"].as_str())
        .collect::<Vec<_>>();
    assert!(names.contains(&"@module/1/helper"), "{names:?}");
    assert!(names.contains(&"@module/2/helper"), "{names:?}");

    project.write("left_public.gu", "pub fn value() -> Int:\n    return 1\n");
    project.write("right_public.gu", "pub fn value() -> Int:\n    return 2\n");
    project.write(
        "main.gu",
        "import \"left_public.gu\"\nimport \"right_public.gu\"\n\nfn main() -> Int:\n    return value()\n",
    );
    let ambiguous = run_self_host(&project, "main.gu");
    assert!(
        ambiguous.status.success(),
        "{}",
        String::from_utf8_lossy(&ambiguous.stderr)
    );
    assert!(String::from_utf8_lossy(&ambiguous.stdout).contains("E_NAME_AMBIGUOUS"));
}

#[test]
fn self_host_module_loader_reports_import_cycles() {
    let project = TempProject::new();
    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let self_host = repository.join("language/self_host");
    for filename in [
        "guion.toml",
        "compiler.gu",
        "lexer.gu",
        "parser.gu",
        "builtin_catalog.gu",
        "artifact.gu",
    ] {
        fs::copy(self_host.join(filename), project.0.join(filename))
            .expect("copy self-host compiler source");
    }
    project.write(
        "main.gu",
        "import \"helper.gu\"\n\nfn main() -> Int:\n    return helper()\n",
    );
    project.write(
        "helper.gu",
        "import \"main.gu\"\n\npub fn helper() -> Int:\n    return 42\n",
    );
    assert_eq!(project.compile().unwrap_err().code, "E_MODULE_CYCLE");

    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", "compiler.gub", "main.gu"])
        .current_dir(&project.0)
        .output()
        .expect("run self-host compiler");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let diagnostic = String::from_utf8_lossy(&output.stdout);
    assert!(
        diagnostic.contains("E_MODULE_CYCLE"),
        "cycle must be a structured diagnostic: {diagnostic}"
    );
    assert!(!String::from_utf8_lossy(&output.stderr).contains("R_ASSERTION"));
}

#[test]
fn self_host_module_loader_rejects_project_root_escapes() {
    let project = TempProject::new();
    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let self_host = repository.join("language/self_host");
    for filename in [
        "guion.toml",
        "compiler.gu",
        "lexer.gu",
        "parser.gu",
        "builtin_catalog.gu",
        "artifact.gu",
    ] {
        fs::copy(self_host.join(filename), project.0.join(filename))
            .expect("copy self-host compiler source");
    }
    let source = "import \"../outside.gu\"\n\nfn main() -> Int:\n    return 0\n";
    project.write("escape.gu", source);
    let bootstrap_error = guion_compiler::compile_project(&project.0.join("escape.gu"), &project.0)
        .expect_err("bootstrap compiler must reject a project-root escape");
    assert_eq!(bootstrap_error.code, "E_IMPORT_PATH");

    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", "compiler.gub", "escape.gu"])
        .current_dir(&project.0)
        .output()
        .expect("run self-host compiler");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let diagnostic = String::from_utf8_lossy(&output.stdout);
    assert!(
        diagnostic.contains("E_IMPORT_PATH"),
        "escaping import must produce a diagnostic: {diagnostic}"
    );
    assert!(!String::from_utf8_lossy(&output.stderr).contains("R_ASSERTION"));
}

#[test]
fn self_host_module_loader_reports_missing_imports_without_asserting() {
    let project = TempProject::new();
    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let self_host = repository.join("language/self_host");
    for filename in [
        "guion.toml",
        "compiler.gu",
        "lexer.gu",
        "parser.gu",
        "builtin_catalog.gu",
        "artifact.gu",
    ] {
        fs::copy(self_host.join(filename), project.0.join(filename))
            .expect("copy self-host compiler source");
    }
    project.write(
        "main.gu",
        "import \"missing.gu\"\n\nfn main() -> Int:\n    return 0\n",
    );
    let bootstrap_error = guion_compiler::compile_project(&project.0.join("main.gu"), &project.0)
        .expect_err("bootstrap compiler must reject a missing imported module");
    assert_eq!(bootstrap_error.code, "E_IMPORT_READ");

    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", "compiler.gub", "main.gu"])
        .current_dir(&project.0)
        .output()
        .expect("run self-host compiler");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let diagnostic = String::from_utf8_lossy(&output.stdout);
    assert!(
        diagnostic.contains("E_IMPORT_READ"),
        "missing import must produce a diagnostic: {diagnostic}"
    );
    assert!(!String::from_utf8_lossy(&output.stderr).contains("R_ASSERTION"));
}

#[test]
fn self_host_module_loader_enforces_import_depth_limit() {
    let project = TempProject::new();
    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let self_host = repository.join("language/self_host");
    for filename in [
        "guion.toml",
        "compiler.gu",
        "lexer.gu",
        "parser.gu",
        "builtin_catalog.gu",
        "artifact.gu",
    ] {
        fs::copy(self_host.join(filename), project.0.join(filename))
            .expect("copy self-host compiler source");
    }
    project.write(
        "main.gu",
        "import \"module-1.gu\"\n\nfn main() -> Int:\n    return helper()\n",
    );
    for index in 1..129 {
        project.write(
            &format!("module-{index}.gu"),
            &format!(
                "import \"module-{}.gu\"\n\npub fn module_{index}() -> Int:\n    return {index}\n",
                index + 1
            ),
        );
    }
    project.write("module-129.gu", "pub fn helper() -> Int:\n    return 42\n");
    let bootstrap_error = guion_compiler::compile_project(&project.0.join("main.gu"), &project.0)
        .expect_err("bootstrap compiler must enforce the 128-level import limit");
    assert_eq!(bootstrap_error.code, "E_MODULE_DEPTH");

    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", "compiler.gub", "main.gu"])
        .current_dir(&project.0)
        .output()
        .expect("run self-host compiler");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let diagnostic = String::from_utf8_lossy(&output.stdout);
    assert!(
        diagnostic.contains("E_MODULE_DEPTH"),
        "deep import graph must produce a diagnostic: {diagnostic}"
    );
    assert!(!String::from_utf8_lossy(&output.stderr).contains("R_ASSERTION"));
}

#[test]
fn accepts_unicode_scalar_escapes_in_text_literals() {
    let artifact = compile(
        "unicode-escape.gu",
        r#"fn main() -> Int:
    return len("\u{1F642}")
"#,
    )
    .expect("valid Unicode scalar escape should compile");
    assert_eq!(artifact.entry(), "main");
}

#[test]
fn option_and_result_constructors_use_nested_expected_types() {
    let source = "fn first() -> Option[Int]:\n    return Some(1)\n\nfn second() -> Option[Int]:\n    return None\n\nfn ok() -> Result[List[Int], Text]:\n    return Ok([])\n\nfn err() -> Result[List[Int], Text]:\n    return Err(\"unavailable\")\n\nfn main() -> Int:\n    return 0\n";
    compile("constructors.gu", source)
        .expect("constructors and empty collections inherit nested expected types");
}

#[test]
fn private_helpers_with_the_same_name_stay_module_local() {
    let project = TempProject::new();
    project.write(
        "main.gu",
        "import \"left.gu\"\nimport \"right.gu\"\n\nfn main() -> Int:\n    return left_value() + right_value()\n",
    );
    project.write(
        "left.gu",
        "pub fn left_value() -> Int:\n    return helper()\n\nfn helper() -> Int:\n    return 20\n",
    );
    project.write(
        "right.gu",
        "pub fn right_value() -> Int:\n    return helper()\n\nfn helper() -> Int:\n    return 22\n",
    );

    let artifact = project
        .compile()
        .expect("duplicate private helper names in separate modules are valid");
    let names: Vec<_> = artifact
        .functions
        .iter()
        .map(|function| function.name.as_str())
        .collect();
    assert!(names.contains(&"main"));
    assert_eq!(
        names.iter().filter(|name| name.ends_with("helper")).count(),
        2
    );
}

#[test]
fn public_import_collisions_are_reported_as_ambiguity() {
    let project = TempProject::new();
    project.write(
        "main.gu",
        "import \"left.gu\"\nimport \"right.gu\"\n\nfn main() -> Int:\n    return shared()\n",
    );
    project.write("left.gu", "pub fn shared() -> Int:\n    return 1\n");
    project.write("right.gu", "pub fn shared() -> Int:\n    return 2\n");

    let error = project.compile().unwrap_err();
    assert_eq!(error.code, "E_NAME_AMBIGUOUS");
    assert_eq!(error.span.line, 2);
    assert_eq!(error.source_name.as_deref(), Some("main.gu"));
}

#[test]
fn cyclic_and_escaping_imports_fail_at_the_import_site() {
    let cycle = TempProject::new();
    cycle.write(
        "main.gu",
        "import \"helper.gu\"\n\nfn main() -> Int:\n    return 0\n",
    );
    cycle.write(
        "helper.gu",
        "import \"main.gu\"\n\npub fn helper() -> Int:\n    return 1\n",
    );
    assert_eq!(cycle.compile().unwrap_err().code, "E_MODULE_CYCLE");

    let escaping = TempProject::new();
    escaping.write(
        "main.gu",
        "import \"../outside.gu\"\n\nfn main() -> Int:\n    return 0\n",
    );
    let error = escaping.compile().unwrap_err();
    assert_eq!(error.code, "E_IMPORT_PATH");
    assert_eq!(error.span.line, 1);
}

#[test]
fn private_imported_functions_are_not_visible_to_the_importer() {
    let project = TempProject::new();
    project.write(
        "main.gu",
        r#"import "library.gu"

fn main() -> Int:
    return hidden()
"#,
    );
    project.write(
        "library.gu",
        r#"fn hidden() -> Int:
    return 7
"#,
    );

    let error = project.compile().unwrap_err();
    assert_eq!(error.code, "E_NAME_UNKNOWN");
    assert_eq!(error.source_name.as_deref(), Some("main.gu"));
    assert_eq!(error.span.line, 4);
}

#[test]
fn repeated_imports_resolve_to_one_module_instance() {
    let project = TempProject::new();
    project.write(
        "main.gu",
        "import \"left.gu\"\nimport \"right.gu\"\n\nfn main() -> Int:\n    return left_value() + right_value()\n",
    );
    project.write(
        "left.gu",
        "import \"shared.gu\"\n\npub fn left_value() -> Int:\n    return shared_value()\n",
    );
    project.write(
        "right.gu",
        "import \"shared.gu\"\n\npub fn right_value() -> Int:\n    return shared_value()\n",
    );
    project.write(
        "shared.gu",
        "pub fn shared_value() -> Int:\n    return 21\n",
    );

    let artifact = project
        .compile()
        .expect("the shared import is resolved once");
    assert_eq!(
        artifact
            .functions
            .iter()
            .filter(|function| function.name == "shared_value")
            .count(),
        1
    );
}

#[test]
fn unicode_source_paths_and_independent_projects_compile() {
    let first = TempProject::new();
    first.write(
        "main.gu",
        "import \"café.gu\"\n\nfn main() -> Int:\n    return value()\n",
    );
    first.write("café.gu", "pub fn value() -> Int:\n    return 1\n");
    first.compile().expect("Unicode module path compiles");

    let second = TempProject::new();
    second.write(
        "main.gu",
        "import \"café.gu\"\n\nfn main() -> Int:\n    return value()\n",
    );
    second.write("café.gu", "pub fn value() -> Int:\n    return 2\n");
    second
        .compile()
        .expect("a second project resolves its own library");
}

#[test]
fn bootstrap_resolver_uses_explicit_standard_and_locked_package_roots() {
    let project = TempProject::new();
    let stdlib_root = project.0.join("installed-stdlib");
    let package_root = project.0.join("package-cache/report_helpers");
    fs::create_dir_all(&stdlib_root).expect("create test standard-library root");
    fs::create_dir_all(package_root.join("render")).expect("create test package root");
    fs::write(
        stdlib_root.join("csv.gu"),
        "pub fn csv_rows() -> Int:\n    return 3\n",
    )
    .expect("write standard-library module");
    fs::write(
        package_root.join("render/table.gu"),
        "pub fn rendered_rows() -> Int:\n    return 4\n",
    )
    .expect("write package module");
    project.write(
        "main.gu",
        "import \"std:csv\"\nimport \"pkg:report_helpers/render/table.gu\"\n\nfn main() -> Int:\n    return csv_rows() + rendered_rows()\n",
    );
    let package_roots =
        std::collections::HashMap::from([("report_helpers".to_string(), package_root.clone())]);
    let artifact = guion_compiler::compile_project_with_roots(
        &project.0.join("main.gu"),
        &project.0,
        Some(&stdlib_root),
        &package_roots,
    )
    .expect("resolve explicit standard and locked package roots");
    assert_eq!(
        artifact.sources,
        ["main.gu", "std:csv", "pkg:report_helpers/render/table.gu"]
    );

    let missing_stdlib = guion_compiler::compile_project_with_roots(
        &project.0.join("main.gu"),
        &project.0,
        None,
        &package_roots,
    )
    .expect_err("standard imports require an explicit standard-library root");
    assert_eq!(missing_stdlib.code, "E_IMPORT_ROOT");

    project.write(
        "unlocked.gu",
        "import \"pkg:unknown/module.gu\"\n\nfn main() -> Int:\n    return 0\n",
    );
    let missing_package = guion_compiler::compile_project_with_roots(
        &project.0.join("unlocked.gu"),
        &project.0,
        Some(&stdlib_root),
        &package_roots,
    )
    .expect_err("package imports require a resolved locked package root");
    assert_eq!(missing_package.code, "E_PACKAGE_NOT_LOCKED");

    fs::write(
        stdlib_root.join("csv.gu"),
        "import \"../main.gu\"\n\npub fn csv_rows() -> Int:\n    return 3\n",
    )
    .expect("replace standard module with an escaping import");
    let escaped_stdlib = guion_compiler::compile_project_with_roots(
        &project.0.join("main.gu"),
        &project.0,
        Some(&stdlib_root),
        &package_roots,
    )
    .expect_err("relative imports from a standard module remain in the standard root");
    assert_eq!(escaped_stdlib.code, "E_IMPORT_PATH");
}

#[test]
fn self_host_resolver_uses_explicit_standard_and_locked_package_roots() {
    let project = TempProject::new();
    install_self_host_compiler(&project);
    project.write("stdlib/csv.gu", "pub fn csv_rows() -> Int:\n    return 3\n");
    project.write(
        "packages/report_helpers/render/table.gu",
        "pub fn rendered_rows() -> Int:\n    return 4\n",
    );
    project.write(
        "main.gu",
        "import \"std:csv\"\nimport \"pkg:report_helpers/render/table.gu\"\n\nfn main() -> Int:\n    return csv_rows() + rendered_rows()\n",
    );
    let compiled = run_self_host_args(
        &project,
        &[
            "main.gu",
            "--stdlib-root",
            "stdlib",
            "--package-root",
            "report_helpers=packages/report_helpers",
        ],
    );
    assert!(
        compiled.status.success(),
        "{}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    let ir = String::from_utf8_lossy(&compiled.stdout);
    assert!(ir.contains("\"name\":\"main\""), "{ir}");
    assert!(ir.contains("\"name\":\"csv_rows\""), "{ir}");
    assert!(ir.contains("\"name\":\"rendered_rows\""), "{ir}");

    let missing_stdlib = run_self_host(&project, "main.gu");
    assert!(
        missing_stdlib.status.success(),
        "{}",
        String::from_utf8_lossy(&missing_stdlib.stderr)
    );
    assert!(String::from_utf8_lossy(&missing_stdlib.stdout).contains("E_IMPORT_ROOT"));

    project.write(
        "unlocked.gu",
        "import \"pkg:unknown/module.gu\"\n\nfn main() -> Int:\n    return 0\n",
    );
    let missing_package = run_self_host_args(&project, &["unlocked.gu"]);
    assert!(
        missing_package.status.success(),
        "{}",
        String::from_utf8_lossy(&missing_package.stderr)
    );
    assert!(String::from_utf8_lossy(&missing_package.stdout).contains("E_PACKAGE_NOT_LOCKED"));

    project.write(
        "scoped.gu",
        "import \"pkg:@acme/widgets/module.gu\"\n\nfn main() -> Int:\n    return 0\n",
    );
    let scoped_package = run_self_host_args(&project, &["scoped.gu"]);
    assert!(
        scoped_package.status.success(),
        "{}",
        String::from_utf8_lossy(&scoped_package.stderr)
    );
    assert!(String::from_utf8_lossy(&scoped_package.stdout).contains("E_PACKAGE_SCOPE_RESERVED"));

    project.write(
        "stdlib/csv.gu",
        "import \"../main.gu\"\n\npub fn csv_rows() -> Int:\n    return 3\n",
    );
    let escaped_stdlib = run_self_host_args(
        &project,
        &[
            "main.gu",
            "--stdlib-root",
            "stdlib",
            "--package-root",
            "report_helpers=packages/report_helpers",
        ],
    );
    assert!(
        escaped_stdlib.status.success(),
        "{}",
        String::from_utf8_lossy(&escaped_stdlib.stderr)
    );
    assert!(String::from_utf8_lossy(&escaped_stdlib.stdout).contains("E_IMPORT_PATH"));
}

#[test]
fn both_compilers_consume_fixed_builtin_types_from_the_shared_catalog() {
    let catalog: serde_json::Value =
        serde_json::from_str(include_str!("../../../language/builtin-signatures.json"))
            .expect("builtin signature catalog must be valid JSON");
    let entries = catalog["entries"].as_array().expect("catalog entries");
    let mut cases = Vec::new();
    for entry in entries {
        let name = entry["name"].as_str().expect("builtin name");
        let parameters = entry["parameters"].as_array().expect("builtin parameters");
        let result = entry["result"].as_str().expect("builtin result");
        if result.contains('$') || result.contains('|') {
            continue;
        }
        let Some(arguments) = parameters
            .iter()
            .map(|parameter| catalog_example(parameter.as_str()?))
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        cases.push((
            name.to_string(),
            parameters.clone(),
            catalog_language_type(result),
            arguments,
        ));
    }
    assert!(
        cases.len() >= 40,
        "expected broad concrete builtin coverage, found {}",
        cases.len()
    );

    let project = TempProject::new();
    install_self_host_compiler(&project);
    let mut source = String::from(
        "record CalendarEvent:\n    uid: Text\n\nrecord Date:\n    year: Int\n\nrecord DirectoryEntry:\n    path: Text\n\nrecord HttpResponse:\n    status: Int\n    headers: Map[Text, Text]\n    body: Bytes\n\nrecord Instant:\n    seconds: Int\n\nrecord SemVer:\n    major: Int\n\nrecord StdError:\n    code: Text\n\nfn worker(value: Int) -> Int:\n    return value\n\n",
    );
    for (index, (name, _, result, arguments)) in cases.iter().enumerate() {
        let wrapper = format!("catalog_call_{index}");
        let joined = arguments.join(", ");
        source.push_str(&format!(
            "fn {wrapper}() -> {result}:\n    let result: {result} = {name}({joined})\n    return result\n\n"
        ));
    }
    source.push_str("fn main() -> Int:\n");
    for index in 0..cases.len() {
        source.push_str(&format!("    catalog_call_{index}()\n"));
    }
    source.push_str("    return 0\n");
    project.write("main.gu", &source);
    project
        .compile()
        .expect("bootstrap compiler accepts every fixed catalog signature");
    let self_host = run_self_host(&project, "main.gu");
    assert!(
        self_host.status.success(),
        "{}",
        String::from_utf8_lossy(&self_host.stderr)
    );
    let ir: serde_json::Value = serde_json::from_slice(&self_host.stdout)
        .expect("self-host compiler should return JSON IR");
    assert!(ir.get("compile_error").is_none(), "{ir}");
    assert_eq!(
        ir["functions"].as_array().unwrap().len(),
        cases.len() + 2,
        "{ir}"
    );

    for (name, parameters, _, arguments) in &cases {
        let Some(first_type) = parameters.first().and_then(serde_json::Value::as_str) else {
            continue;
        };
        if first_type == "Any"
            || first_type == "Sized"
            || first_type.contains('$')
            || first_type.contains('|')
        {
            continue;
        }
        let mut wrong_arguments = arguments.clone();
        wrong_arguments[0] = catalog_wrong_example(first_type);
        let invalid = format!(
            "fn worker(value: Int) -> Int:\n    return value\n\nfn main() -> Int:\n    let result = {name}({})\n    return 0\n",
            wrong_arguments.join(", ")
        );
        project.write("main.gu", &invalid);
        let bootstrap_error = project
            .compile()
            .expect_err("bootstrap compiler rejects wrong catalog argument types");
        assert_eq!(
            bootstrap_error.code, "E_TYPE_ARGUMENT",
            "{name}: {bootstrap_error:?}"
        );
        let output = run_self_host(&project, "main.gu");
        assert!(
            output.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("E_TYPE_ARGUMENT"),
            "{name}: {}",
            String::from_utf8_lossy(&output.stdout)
        );
        assert!(
            !String::from_utf8_lossy(&output.stderr).contains("R_ASSERTION"),
            "{name}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
