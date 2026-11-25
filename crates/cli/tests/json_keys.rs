//! `json_keys` (artifact format 90): the top-level keys of a JSON object, in the order
//! `json_stringify` writes them.

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
/// on read, while format 89 and lower artifacts without the operation still load.
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
        "import \"std:json\"\nimport \"std:text\"\nimport \"std:types\"\n\nfn keys_of(text: Text) -> Text:\n    match json_keys(unwrap(json_parse(text))):\n        Ok(keys):\n            return \"[\" + text_join(keys, \"|\") + \"]\"\n        Err(problem):\n            return json_error_code(problem) + \": \" + json_error_message(problem)\n\nfn main() -> Int:\n{body}    return 0\n"
    )
}

#[test]
fn empty_object_has_no_keys() {
    let out = stdout_of("keys-empty", &program("    print(keys_of(\"{}\"))\n"));
    assert_eq!(out, "[]\n");
}

#[test]
fn keys_come_back_in_the_order_stringify_writes_them() {
    // json_stringify writes object members in ascending byte order of the key, whatever order
    // the text had.
    let out = stdout_of(
        "keys-order",
        &program(
            "    let value = unwrap(json_parse(\"{\\\"b\\\":1,\\\"a\\\":2,\\\"B\\\":3,\\\"\\u{00e9}\\\":4,\\\"aa\\\":5}\"))\n    print(json_stringify(value))\n    print(keys_of(\"{\\\"b\\\":1,\\\"a\\\":2,\\\"B\\\":3,\\\"\\u{00e9}\\\":4,\\\"aa\\\":5}\"))\n",
        ),
    );
    assert_eq!(
        out,
        "{\"B\":3,\"a\":2,\"aa\":5,\"b\":1,\"\u{00e9}\":4}\n[B|a|aa|b|\u{00e9}]\n"
    );
}

#[test]
fn only_the_top_level_is_listed() {
    let out = stdout_of(
        "keys-nested",
        &program("    print(keys_of(\"{\\\"outer\\\":{\\\"inner\\\":1},\\\"list\\\":[{\\\"x\\\":1}],\\\"z\\\":null}\"))\n"),
    );
    assert_eq!(out, "[list|outer|z]\n");
}

#[test]
fn keys_that_need_escapes_come_back_unescaped() {
    let out = stdout_of(
        "keys-escapes",
        &program(
            "    match json_keys(unwrap(json_parse(\"{\\\"a\\\\\\\"b\\\":1,\\\"line\\\\nbreak\\\":2,\\\"\\\":3,\\\"tab\\\\t\\\":4}\"))):\n        Ok(keys):\n            print(len(keys))\n            for key in keys:\n                print(len(key))\n        Err(problem):\n            print(json_error_code(problem))\n",
        ),
    );
    // sorted: \"\" (0), \"a\\\"b\" (3), \"line\\nbreak\" (10), \"tab\\t\" (4)
    assert_eq!(out, "4\n0\n3\n10\n4\n");
}

#[test]
fn a_key_that_needs_escaping_matches_the_stringified_text() {
    let out = stdout_of(
        "keys-escape-roundtrip",
        &program(
            "    let value = unwrap(json_parse(\"{\\\"a\\\\\\\"b\\\":1}\"))\n    match json_keys(value):\n        Ok(keys):\n            print(json_stringify(json_text(keys[0])))\n        Err(problem):\n            print(json_error_code(problem))\n",
        ),
    );
    assert_eq!(out, "\"a\\\"b\"\n");
}

#[test]
fn values_that_are_not_objects_are_r_json_type_errors_naming_the_type() {
    let out = stdout_of(
        "keys-errors",
        &program(
            "    print(keys_of(\"[1,2]\"))\n    print(keys_of(\"3\"))\n    print(keys_of(\"null\"))\n    print(keys_of(\"\\\"text\\\"\"))\n    print(keys_of(\"true\"))\n",
        ),
    );
    assert_eq!(
        out,
        "R_JSON_TYPE: json_keys expects a JSON object, found array\nR_JSON_TYPE: json_keys expects a JSON object, found number\nR_JSON_TYPE: json_keys expects a JSON object, found null\nR_JSON_TYPE: json_keys expects a JSON object, found string\nR_JSON_TYPE: json_keys expects a JSON object, found boolean\n"
    );
}

#[test]
fn old_format_rejects_json_keys() {
    assert_old_format_rejects(
        "keys-old",
        "import \"std:json\"\n\nfn main() -> Int:\n    match json_keys(json_object()):\n        Ok(keys):\n            print(len(keys))\n        Err(problem):\n            print(json_error_code(problem))\n    return 0\n",
        90,
        |op| matches!(op, guion_model::Op::JsonKeys),
    );
}

#[test]
fn doc_lists_json_keys() {
    let scratch = Scratch::new("keys-doc");
    let output = guion(&scratch, &["doc", "std:json"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        text.contains("json_keys(value: Json) -> Result[List[Text], JsonError]"),
        "{text}"
    );
}
