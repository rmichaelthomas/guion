//! `json_stringify_pretty` (artifact format 90): indented JSON text.

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

fn failure_of(name: &str, source: &str) -> String {
    let scratch = Scratch::new(name);
    let output = run(&scratch, source);
    assert!(!output.status.success());
    String::from_utf8_lossy(&output.stderr).into_owned()
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
        "import \"std:json\"\nimport \"std:types\"\n\nfn doc() -> Json:\n    return unwrap(json_parse(\"{{\\\"b\\\":[1,2.50,{{\\\"k\\\":null}}],\\\"a\\\":{{}},\\\"c\\\":[],\\\"d\\\":\\\"x\\\\ny\\\\\\\"z\\\",\\\"e\\\":true}}\"))\n\nfn main() -> Int:\n{body}    return 0\n"
    )
}

#[test]
fn indent_two_lays_out_one_member_per_line() {
    let out = stdout_of(
        "pretty-2",
        &program("    print(json_stringify_pretty(doc(), 2))\n"),
    );
    assert_eq!(
        out,
        "{\n  \"a\": {},\n  \"b\": [\n    1,\n    2.50,\n    {\n      \"k\": null\n    }\n  ],\n  \"c\": [],\n  \"d\": \"x\\ny\\\"z\",\n  \"e\": true\n}\n"
    );
}

#[test]
fn indent_four_and_sixteen_scale_per_level() {
    let out = stdout_of(
        "pretty-widths",
        &program("    print(json_stringify_pretty(unwrap(json_parse(\"{\\\"a\\\":[1]}\")), 4))\n    print(json_stringify_pretty(unwrap(json_parse(\"{\\\"a\\\":[1]}\")), 16))\n"),
    );
    let sixteen = " ".repeat(16);
    let thirty_two = " ".repeat(32);
    assert_eq!(
        out,
        format!("{{\n    \"a\": [\n        1\n    ]\n}}\n{{\n{sixteen}\"a\": [\n{thirty_two}1\n{sixteen}]\n}}\n")
    );
}

#[test]
fn indent_zero_still_breaks_lines_without_indenting() {
    let out = stdout_of(
        "pretty-0",
        &program("    print(json_stringify_pretty(unwrap(json_parse(\"{\\\"a\\\":[1,2],\\\"b\\\":{}}\")), 0))\n"),
    );
    assert_eq!(out, "{\n\"a\": [\n1,\n2\n],\n\"b\": {}\n}\n");
}

#[test]
fn empty_containers_and_scalars_are_one_line_with_no_trailing_newline() {
    let out = stdout_of(
        "pretty-small",
        &program(
            "    print(\"<\" + json_stringify_pretty(json_object(), 2) + \">\")\n    print(\"<\" + json_stringify_pretty(json_array(), 2) + \">\")\n    print(\"<\" + json_stringify_pretty(json_int(7), 2) + \">\")\n    print(\"<\" + json_stringify_pretty(json_null(), 2) + \">\")\n    print(\"<\" + json_stringify_pretty(json_text(\"hi\"), 2) + \">\")\n",
        ),
    );
    assert_eq!(out, "<{}>\n<[]>\n<7>\n<null>\n<\"hi\">\n");
}

#[test]
fn non_ascii_text_is_written_as_is_and_escapes_match_stringify() {
    let out = stdout_of(
        "pretty-unicode",
        &program(
            "    let value = json_set(json_set(json_object(), \"caf\u{00e9}\", json_text(\"na\u{00ef}ve \u{1f331}\")), \"ctl\", json_text(\"a\\u{1}\\tb/\"))\n    print(json_stringify_pretty(value, 1))\n    print(json_stringify(value))\n",
        ),
    );
    assert_eq!(
        out,
        "{\n \"caf\u{00e9}\": \"na\u{00ef}ve \u{1f331}\",\n \"ctl\": \"a\\u0001\\tb/\"\n}\n{\"caf\u{00e9}\":\"na\u{00ef}ve \u{1f331}\",\"ctl\":\"a\\u0001\\tb/\"}\n"
    );
}

#[test]
fn numbers_keep_the_text_json_stringify_writes() {
    let out = stdout_of(
        "pretty-numbers",
        &program(
            "    let value = unwrap(json_parse(\"[123456789012345678901234567890,1.0,-0.5,1e3]\"))\n    print(json_stringify(value))\n    print(json_stringify_pretty(value, 1))\n",
        ),
    );
    assert_eq!(
        out,
        "[123456789012345678901234567890,1.0,-0.5,1e+3]\n[\n 123456789012345678901234567890,\n 1.0,\n -0.5,\n 1e+3\n]\n"
    );
}

#[test]
fn indent_outside_zero_to_sixteen_is_r_json_indent() {
    for (name, indent) in [
        ("neg", "-1"),
        ("over", "17"),
        ("huge", "100000000000000000000"),
    ] {
        let err = failure_of(
            &format!("pretty-range-{name}"),
            &program(&format!(
                "    print(json_stringify_pretty(doc(), {indent}))\n"
            )),
        );
        assert!(err.contains("R_JSON_INDENT"), "{indent}: {err}");
    }
}

#[test]
fn old_format_rejects_json_stringify_pretty() {
    assert_old_format_rejects(
        "pretty-old",
        "import \"std:json\"\n\nfn main() -> Int:\n    print(json_stringify_pretty(json_object(), 2))\n    return 0\n",
        90,
        |op| matches!(op, guion_model::Op::JsonStringifyPretty),
    );
}

#[test]
fn doc_lists_json_stringify_pretty() {
    let scratch = Scratch::new("pretty-doc");
    let output = guion(&scratch, &["doc", "std:json"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        text.contains("json_stringify_pretty(value: Json, indent: Int) -> Text"),
        "{text}"
    );
}
