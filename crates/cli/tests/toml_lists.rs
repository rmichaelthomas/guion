//! `toml_get_text_list` and `toml_keys` (artifact format 91): the strings of a TOML array and
//! the keys of a TOML table in document order.

use std::path::PathBuf;
use std::process::{Command, Output};

struct Scratch(PathBuf);
impl Scratch {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("guion-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn guion(scratch: &Scratch, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(args)
        .current_dir(&scratch.0)
        .env("GUION_NO_PROMPT", "1")
        .output()
        .expect("guion starts")
}

fn run(scratch: &Scratch, source: &str) -> Output {
    std::fs::write(scratch.0.join("main.gu"), source).unwrap();
    guion(scratch, &["run", "main.gu"])
}

fn stdout_of(name: &str, source: &str) -> String {
    let scratch = Scratch::new(name);
    let output = run(&scratch, source);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Builds `source` to a `.gub`, checks it carries `op` at the current format, and checks that
/// the same artifact stamped one format below `first_format` is `E_ARTIFACT_VERSION` on write and
/// on read, while older artifacts without the operation still load.
fn assert_old_format_rejects(
    name: &str,
    source: &str,
    first_format: u32,
    has_op: fn(&guion_model::Op) -> bool,
) {
    use guion_model::Artifact;
    let scratch = Scratch::new(name);
    std::fs::write(scratch.0.join("main.gu"), source).unwrap();
    let built = scratch.0.join("main.gub");
    let output = guion(
        &scratch,
        &["build", "main.gu", "--output", built.to_str().unwrap()],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let current = std::fs::read(&built).unwrap();
    let mut artifact = Artifact::from_bytes(&current).unwrap();
    assert_eq!(artifact.format_version, Artifact::CURRENT_VERSION);
    assert!(artifact.format_version >= first_format);
    assert!(artifact
        .functions
        .iter()
        .any(|function| function.code.iter().any(|i| has_op(&i.op))));
    let previous = first_format - 1;
    artifact.format_version = previous;
    assert_eq!(artifact.to_bytes().unwrap_err().code, "E_ARTIFACT_VERSION");
    let mut old = current;
    old[4..8].copy_from_slice(&previous.to_le_bytes());
    assert_eq!(
        Artifact::from_bytes(&old).unwrap_err().code,
        "E_ARTIFACT_VERSION"
    );
}

fn program(body: &str) -> String {
    format!(
        "import \"std:toml\"\nimport \"std:text\"\nimport \"std:types\"\n\nfn list_of(source: Text, path: List[Text]) -> Text:\n    let document = unwrap(toml_parse(source))\n    match toml_get_text_list(document, path):\n        Ok(items):\n            return \"[\" + text_join(items, \"|\") + \"]\" + int_to_text(len(items))\n        Err(problem):\n            return problem.code + \": \" + problem.message\n\nfn keys_of(source: Text, path: List[Text]) -> Text:\n    let document = unwrap(toml_parse(source))\n    match toml_keys(document, path):\n        Ok(items):\n            return \"[\" + text_join(items, \"|\") + \"]\"\n        Err(problem):\n            return problem.code + \": \" + problem.message\n\nfn main() -> Int:\n{body}    return 0\n"
    )
}

#[test]
fn text_list_reads_strings_in_order() {
    let out = stdout_of(
        "toml-list-order",
        &program("    print(list_of(\"keep = [\\\"perf-session\\\", \\\"completion\\\"]\\n\", [\"keep\"]))\n"),
    );
    assert_eq!(out, "[perf-session|completion]2\n");
}

#[test]
fn text_list_empty_array() {
    let out = stdout_of(
        "toml-list-empty",
        &program("    print(list_of(\"keep = []\\n\", [\"keep\"]))\n"),
    );
    assert_eq!(out, "[]0\n");
}

#[test]
fn text_list_nested_table_path() {
    let out = stdout_of(
        "toml-list-nested",
        &program(
            "    print(list_of(\"[rule.guion-target]\\nkeep = [\\\"a\\\", \\\"b\\\", \\\"c\\\"]\\n\", [\"rule\", \"guion-target\", \"keep\"]))\n",
        ),
    );
    assert_eq!(out, "[a|b|c]3\n");
}

#[test]
fn text_list_missing_key_error_code() {
    // The same code toml_get_text gives for a key that is not there.
    let out = stdout_of(
        "toml-list-missing",
        &program(
            "    let document = unwrap(toml_parse(\"a = 1\\n\"))\n    match toml_get_text(document, [\"keep\"]):\n        Ok(text):\n            print(text)\n        Err(problem):\n            print(problem.code)\n    print(list_of(\"a = 1\\n\", [\"keep\"]))\n",
        ),
    );
    assert_eq!(
        out,
        "E_TOML_TYPE\nE_TOML_TYPE: keep must be an array of text\n"
    );
}

#[test]
fn text_list_rejects_non_string_element() {
    let out = stdout_of(
        "toml-list-element",
        &program("    print(list_of(\"keep = [1, \\\"a\\\"]\\n\", [\"keep\"]))\n    print(list_of(\"keep = [\\\"a\\\", true]\\n\", [\"keep\"]))\n"),
    );
    assert_eq!(
        out,
        "E_TOML_TYPE: keep[0] must be text\nE_TOML_TYPE: keep[1] must be text\n"
    );
}

#[test]
fn text_list_rejects_non_array() {
    let out = stdout_of(
        "toml-list-scalar",
        &program(
            "    print(list_of(\"keep = \\\"one\\\"\\n\", [\"keep\"]))\n    print(list_of(\"[keep]\\nx = 1\\n\", [\"keep\"]))\n",
        ),
    );
    assert_eq!(
        out,
        "E_TOML_TYPE: keep must be an array of text\nE_TOML_TYPE: keep must be an array of text\n"
    );
}

#[test]
fn keys_lists_root_in_document_order() {
    let out = stdout_of(
        "toml-keys-root",
        &program(
            "    print(keys_of(\"b = 1\\na = 2\\n[t]\\nz = 1\\n\", []))\n    print(keys_of(\"\", []))\n",
        ),
    );
    assert_eq!(out, "[b|a|t]\n[]\n");
}

#[test]
fn keys_lists_subtables() {
    let out = stdout_of(
        "toml-keys-sub",
        &program(
            "    print(keys_of(\"[rule.x]\\na = 1\\n[rule.y]\\nb = 2\\n\", [\"rule\"]))\n    print(keys_of(\"[rule.y]\\nb = 2\\n[rule.x]\\na = 1\\n\", [\"rule\"]))\n    print(keys_of(\"[rule.x]\\nz = 1\\ny = 2\\n\", [\"rule\", \"x\"]))\n",
        ),
    );
    assert_eq!(out, "[x|y]\n[y|x]\n[z|y]\n");
}

#[test]
fn keys_rejects_non_table() {
    let out = stdout_of(
        "toml-keys-errors",
        &program(
            "    print(keys_of(\"a = 1\\n\", [\"a\"]))\n    print(keys_of(\"a = 1\\n\", [\"nope\"]))\n    print(keys_of(\"a = [1]\\n\", [\"a\"]))\n",
        ),
    );
    assert_eq!(
        out,
        "E_TOML_TYPE: a must be a table\nE_TOML_TYPE: nope must be a table\nE_TOML_TYPE: a must be a table\n"
    );
}

#[test]
fn format_90_artifact_using_toml_keys_is_rejected() {
    assert_old_format_rejects(
        "toml-keys-old",
        "import \"std:toml\"\n\nfn main() -> Int:\n    match toml_parse(\"a = 1\\n\"):\n        Ok(document):\n            match toml_keys(document, []):\n                Ok(keys):\n                    print(len(keys))\n                Err(problem):\n                    print(problem.code)\n        Err(problem):\n            print(problem.code)\n    return 0\n",
        91,
        |op| matches!(op, guion_model::Op::TomlKeys),
    );
}

#[test]
fn format_90_artifact_using_toml_get_text_list_is_rejected() {
    assert_old_format_rejects(
        "toml-list-old",
        "import \"std:toml\"\n\nfn main() -> Int:\n    match toml_parse(\"a = [\\\"x\\\"]\\n\"):\n        Ok(document):\n            match toml_get_text_list(document, [\"a\"]):\n                Ok(items):\n                    print(len(items))\n                Err(problem):\n                    print(problem.code)\n        Err(problem):\n            print(problem.code)\n    return 0\n",
        91,
        |op| matches!(op, guion_model::Op::TomlGetTextList),
    );
}

#[test]
fn doc_lists_toml_list_functions() {
    let scratch = Scratch::new("toml-list-doc");
    let output = guion(&scratch, &["doc", "std:toml"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        text.contains("toml_get_text_list(document: TomlDocument, path: List[Text]) -> Result[List[Text], StdError]"),
        "{text}"
    );
    assert!(
        text.contains(
            "toml_keys(document: TomlDocument, path: List[Text]) -> Result[List[Text], StdError]"
        ),
        "{text}"
    );
}
