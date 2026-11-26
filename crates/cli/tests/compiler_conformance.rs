use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use guion_compiler::{compile, compile_project, compile_project_with_roots, Diagnostic};
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
            "fn main() -> Int:\n    let values: Map[Bool, Text] = {}\n    return 0\n",
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
fn python_syntax_recovery_cases_report_diagnostics_and_replacements() {
    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let case_file = repository.join("language/conformance/python-recovery-cases.json");
    let corpus: Value =
        serde_json::from_slice(&fs::read(&case_file).expect("read recovery corpus"))
            .expect("parse recovery corpus");
    assert_eq!(corpus["schema_version"], 1);
    let cases = corpus["cases"].as_array().expect("recovery cases");
    assert_eq!(cases.len(), 9);

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
            }
            None => {
                bootstrap.expect("None is already the native empty Option constructor");
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
fn rust_compiler_consumes_fixed_builtin_types_from_the_shared_catalog() {
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
    let mut source = String::from(
        "record CalendarEvent:\n    uid: Text\n\nrecord CsvColumn:\n    name: Text\n\nrecord Date:\n    year: Int\n\nrecord DirectoryEntry:\n    path: Text\n\nrecord HttpResponse:\n    status: Int\n    headers: Map[Text, Text]\n    body: Bytes\n\nrecord Instant:\n    seconds: Int\n\nrecord SemVer:\n    major: Int\n\nrecord StdError:\n    code: Text\n\nfn worker(value: Int) -> Int:\n    return value\n\n",
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
    let artifact = project
        .compile()
        .expect("the Rust compiler accepts every fixed catalog signature");
    // The source declares one wrapper per concrete catalog signature, plus `worker` and `main`.
    assert!(
        artifact.functions.len() > cases.len(),
        "every catalog wrapper must compile into the artifact"
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
    }
}
