//! `std:yaml` (artifact format 72): a read-only YAML subset. The reader is hand-written in
//! `crates/runtime/src/yaml.rs`; these tests run real Guion programs against it and pin the
//! scalar policy, the rejected features and the typed errors.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct Scratch(PathBuf);
impl Scratch {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("guion-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("root")).unwrap();
        Scratch(dir)
    }
    fn root(&self) -> PathBuf {
        self.0.join("root")
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/yaml")
        .join(name)
}

fn run(scratch: &Scratch, source: &str) -> Output {
    std::fs::write(scratch.0.join("main.gu"), source).unwrap();
    let root = scratch.root();
    Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", "--file-root", root.to_str().unwrap(), "main.gu"])
        .current_dir(&scratch.0)
        .output()
        .expect("guion starts")
}

const PROGRAM: &str = r#"import "std:yaml"
import "std:json"
import "std:types"

fn main() -> Int:
    match yaml_parse(unwrap(read_text("in.yml"))):
        Ok(value):
            print("OK " + json_stringify(value))
        Err(problem):
            print("ERR " + problem.code + "@@@" + problem.message + "@@@" + int_to_text(problem.line) + "@@@" + int_to_text(problem.column))
    return 0
"#;

#[derive(Debug)]
enum Parsed {
    Value(Value),
    Error {
        code: String,
        message: String,
        line: usize,
        column: usize,
    },
}

fn parse_in(scratch: &Scratch, text: &str) -> Parsed {
    std::fs::write(scratch.root().join("in.yml"), text).unwrap();
    let output = run(scratch, PROGRAM);
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(
        output.status.success(),
        "stdout: {stdout}\nstderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let line = stdout.trim_end();
    if let Some(json_text) = line.strip_prefix("OK ") {
        Parsed::Value(serde_json::from_str(json_text).unwrap())
    } else {
        let rest = line
            .strip_prefix("ERR ")
            .unwrap_or_else(|| panic!("{line}"));
        let parts: Vec<&str> = rest.splitn(4, "@@@").collect();
        Parsed::Error {
            code: parts[0].into(),
            message: parts[1].into(),
            line: parts[2].parse().unwrap(),
            column: parts[3].parse().unwrap(),
        }
    }
}

fn value_of(scratch: &Scratch, text: &str) -> Value {
    match parse_in(scratch, text) {
        Parsed::Value(value) => value,
        other => panic!("expected a value for {text:?}, got {other:?}"),
    }
}

fn error_of(scratch: &Scratch, text: &str) -> (String, String, usize, usize) {
    match parse_in(scratch, text) {
        Parsed::Error {
            code,
            message,
            line,
            column,
        } => (code, message, line, column),
        other => panic!("expected an error for {text:?}, got {other:?}"),
    }
}

#[test]
fn github_actions_workflow_parses_to_the_expected_structure() {
    let scratch = Scratch::new("yaml-workflow");
    let text = std::fs::read_to_string(fixture("workflow.yml")).unwrap();
    let value = value_of(&scratch, &text);
    let expected = json!({
        "name": "CI",
        "on": {
            "push": {"branches": ["main"]},
            "pull_request": {"branches": ["main", "release/**"]}
        },
        "env": {
            "RUST_BACKTRACE": 1,
            "RATIO": 0.5,
            "FLAG": true,
            "NOTHING": null,
            "TILDE": null,
            "ANSWER": "NO"
        },
        "jobs": {"build": {
            "runs-on": "${{ matrix.os }}",
            "strategy": {"matrix": {
                "os": ["ubuntu-latest", "macos-latest"],
                "rust": ["stable", "1.75"]
            }},
            "steps": [
                {"uses": "actions/checkout@v4"},
                {
                    "name": "Install toolchain",
                    "uses": "dtolnay/rust-toolchain@stable",
                    "with": {"toolchain": "${{ matrix.rust }}"}
                },
                {"name": "Test", "run": "cargo build --locked\ncargo test --locked\n"},
                {"name": "Summary", "run": "echo one two\nthree\n"}
            ]
        }}
    });
    assert_eq!(value, expected);
}

#[test]
fn docker_compose_file_parses_to_the_expected_structure() {
    let scratch = Scratch::new("yaml-compose");
    let text = std::fs::read_to_string(fixture("compose.yml")).unwrap();
    let value = value_of(&scratch, &text);
    let expected = json!({
        "version": "3.8",
        "services": {
            "web": {
                "image": "nginx:1.25",
                "ports": ["8080:80", "443:443"],
                "environment": ["KEY=value", "OTHER=thing"],
                "depends_on": ["db"],
                "healthcheck": {
                    "test": ["CMD", "curl", "-f", "http://localhost"],
                    "interval": "30s"
                }
            },
            "db": {
                "image": "postgres:16",
                "volumes": ["dbdata:/var/lib/postgresql/data"],
                "environment": {"POSTGRES_PASSWORD": "it's a secret"}
            }
        },
        "volumes": {"dbdata": {}}
    });
    assert_eq!(value, expected);
}

#[test]
fn anchors_and_tags_are_unsupported_with_line_and_column_and_no_value() {
    let scratch = Scratch::new("yaml-unsupported");
    let anchor = std::fs::read_to_string(fixture("anchor.yml")).unwrap();
    let (code, message, line, column) = error_of(&scratch, &anchor);
    assert_eq!(code, "YAML_UNSUPPORTED");
    assert!(message.contains("anchor"), "{message}");
    assert_eq!((line, column), (1, 11));

    let tag = std::fs::read_to_string(fixture("tag.yml")).unwrap();
    let (code, message, line, column) = error_of(&scratch, &tag);
    assert_eq!(code, "YAML_UNSUPPORTED");
    assert!(message.contains("tag"), "{message}");
    assert_eq!((line, column), (2, 8));

    let cases = [
        ("a: *x\n", "alias", 1, 4),
        ("<<: {a: 1}\n", "merge key", 1, 1),
        ("a: 1\n---\nb: 2\n", "multi-document", 2, 1),
        ("---\na: 1\n...\nb: 2\n", "multi-document", 3, 1),
        ("? a\n: b\n", "complex mapping key", 1, 1),
        ("%YAML 1.2\n---\na: 1\n", "directive", 1, 1),
        ("a: [&x 1]\n", "anchor", 1, 5),
        ("a: !!int 3\n", "tag", 1, 4),
        ("- &x a\n", "anchor", 1, 3),
    ];
    for (text, feature, line, column) in cases {
        let (code, message, got_line, got_column) = error_of(&scratch, text);
        assert_eq!(code, "YAML_UNSUPPORTED", "{text:?}: {message}");
        assert!(message.contains(feature), "{text:?}: {message}");
        assert_eq!((got_line, got_column), (line, column), "{text:?}");
    }
}

#[test]
fn every_scalar_style_reads_as_documented() {
    let scratch = Scratch::new("yaml-scalars");
    let text = "\
plain: hello world
time: 12:30
url: http://example.com/a:b
single: 'it''s # not a comment'
double: \"tab\\there \\u00e9 \\\"q\\\"\"
literal: |
  line one
    indented

  last
folded: >
  folded one
  folded two

  next paragraph
strip: |-
  no newline
keep: |+
  kept

empty_value:
quoted_number: \"42\"
quoted_bool: 'true'
";
    let value = value_of(&scratch, text);
    assert_eq!(
        value,
        json!({
            "plain": "hello world",
            "time": "12:30",
            "url": "http://example.com/a:b",
            "single": "it's # not a comment",
            "double": "tab\there \u{e9} \"q\"",
            "literal": "line one\n  indented\n\nlast\n",
            "folded": "folded one folded two\nnext paragraph\n",
            "strip": "no newline",
            "keep": "kept\n\n",
            "empty_value": null,
            "quoted_number": "42",
            "quoted_bool": "true"
        })
    );
}

#[test]
fn scalar_policy_only_converts_lowercase_json_spellings_and_plain_numbers() {
    let scratch = Scratch::new("yaml-policy");
    let text = "\
no_upper: NO
yes_lower: yes
yes_title: Yes
on_lower: 'on'
off_plain: off
version: 1.10
decimal: 2.75
negative: -7
int: 42
zero: 0
leading_zero: 007
big: 12345678901234567890123
hex: 0x1F
exp: 1e3
bool_true: true
bool_false: false
bool_title: True
null_word: null
null_tilde: ~
null_title: Null
";
    let value = value_of(&scratch, text);
    assert_eq!(
        value,
        json!({
            "no_upper": "NO",
            "yes_lower": "yes",
            "yes_title": "Yes",
            "on_lower": "on",
            "off_plain": "off",
            "version": "1.10",
            "decimal": 2.75,
            "negative": -7,
            "int": 42,
            "zero": 0,
            "leading_zero": "007",
            "big": "12345678901234567890123",
            "hex": "0x1F",
            "exp": "1e3",
            "bool_true": true,
            "bool_false": false,
            "bool_title": "True",
            "null_word": null,
            "null_tilde": null,
            "null_title": "Null"
        })
    );
    // Quoted scalars are always text, whatever they spell.
    let quoted = value_of(&scratch, "a: \"NO\"\nb: '1.10'\nc: \"null\"\nd: \"5\"\n");
    assert_eq!(
        quoted,
        json!({"a": "NO", "b": "1.10", "c": "null", "d": "5"})
    );
}

#[test]
fn nesting_flow_collections_and_comments() {
    let scratch = Scratch::new("yaml-nesting");
    let text = "\
# leading comment
list:
- a   # same indent as the key
- b
nested:
  - name: x
    items:
      - 1
      - [2, 3, {k: v}]
  -
    name: y
  - - deep
    - er
flow: {a: 1, b: [x, y], \"c d\": 'e', }
empty_flow: []
";
    let value = value_of(&scratch, text);
    assert_eq!(
        value,
        json!({
            "list": ["a", "b"],
            "nested": [
                {"name": "x", "items": [1, [2, 3, {"k": "v"}]]},
                {"name": "y"},
                ["deep", "er"]
            ],
            "flow": {"a": 1, "b": ["x", "y"], "c d": "e"},
            "empty_flow": []
        })
    );
    let scalar_root = value_of(&scratch, "just text\n");
    assert_eq!(scalar_root, json!("just text"));
    let sequence_root = value_of(&scratch, "---\n- 1\n- two\n...\n");
    assert_eq!(sequence_root, json!([1, "two"]));
    assert_eq!(value_of(&scratch, ""), Value::Null);
}

#[test]
fn malformed_input_gives_typed_errors_with_positions() {
    let scratch = Scratch::new("yaml-malformed");
    let cases = [
        ("a:\n\tb: 1\n", "YAML_SYNTAX", "tab", 2, 1),
        ("a: 1\n  b: 2\n", "YAML_SYNTAX", "mapping values", 2, 4),
        (
            "a:\n  b: 1\n   c: 2\n",
            "YAML_SYNTAX",
            "mapping values",
            3,
            5,
        ),
        (
            "a:\n  b: 1\n c: 2\n",
            "YAML_SYNTAX",
            "bad indentation",
            3,
            2,
        ),
        ("a: \"open\nb: 1\n", "YAML_SYNTAX", "unterminated", 1, 4),
        ("a: 'open\n", "YAML_SYNTAX", "unterminated", 1, 4),
        ("a: [1, 2\n", "YAML_SYNTAX", "unterminated", 1, 4),
        ("a: {b: 1\n", "YAML_SYNTAX", "unterminated", 1, 4),
        ("a: 1\na: 2\n", "YAML_DUPLICATE_KEY", "duplicate", 2, 1),
        ("a: \"\\q\"\n", "YAML_SYNTAX", "escape", 1, 5),
        ("a: b: c\n", "YAML_SYNTAX", "mapping values", 1, 5),
        ("- a\nb: 1\n", "YAML_SYNTAX", "unexpected", 2, 1),
        ("a: |2x\n  t\n", "YAML_SYNTAX", "block scalar", 1, 6),
        ("a: @x\n", "YAML_SYNTAX", "reserved", 1, 4),
    ];
    for (text, code, needle, line, column) in cases {
        let (got_code, message, got_line, got_column) = error_of(&scratch, text);
        assert_eq!(got_code, code, "{text:?}: {message}");
        assert!(message.contains(needle), "{text:?}: {message}");
        assert_eq!(
            (got_line, got_column),
            (line, column),
            "{text:?}: {message}"
        );
    }
}

#[test]
fn nesting_deeper_than_the_limit_is_a_typed_error_not_a_crash() {
    let scratch = Scratch::new("yaml-depth");
    let deep = "[".repeat(5000);
    let (code, _, line, _) = error_of(&scratch, &format!("a: {deep}\n"));
    assert_eq!(code, "YAML_LIMIT");
    assert_eq!(line, 1);
    let mut block = String::new();
    for level in 0..400 {
        block.push_str(&"  ".repeat(level));
        block.push_str("k:\n");
    }
    let (code, _, _, _) = error_of(&scratch, &block);
    assert_eq!(code, "YAML_LIMIT");
}

#[test]
fn a_ten_thousand_line_mapping_parses_inside_the_default_budget() {
    let scratch = Scratch::new("yaml-large");
    let mut text = String::new();
    for index in 0..10_000 {
        text.push_str(&format!("key_{index}: value {index}\n"));
    }
    let started = std::time::Instant::now();
    let value = value_of(&scratch, &text);
    let elapsed = started.elapsed();
    let object = value.as_object().unwrap();
    assert_eq!(object.len(), 10_000);
    assert_eq!(object["key_9999"], json!("value 9999"));
    assert!(
        elapsed < std::time::Duration::from_secs(10),
        "took {elapsed:?}"
    );
}

#[test]
fn guion_test_cases_assert_the_documented_behaviour() {
    let scratch = Scratch::new("yaml-guion-test");
    std::fs::copy(fixture("workflow.yml"), scratch.0.join("workflow.yml")).unwrap();
    std::fs::copy(fixture("anchor.yml"), scratch.0.join("anchor.yml")).unwrap();
    let source = r#"import "std:yaml"
import "std:json"
import "std:types"

fn main() -> Int:
    return 0

fn test_workflow_reads_into_json() -> Unit:
    let doc = unwrap(yaml_parse(unwrap(read_text("workflow.yml"))))
    assert(json_as_text(json_get(doc, "name")) == "CI")
    let env = json_get(doc, "env")
    assert(json_as_text(json_get(env, "ANSWER")) == "NO")
    assert(json_as_int(json_get(env, "RUST_BACKTRACE")) == 1)

fn test_anchor_is_rejected_with_a_position() -> Unit:
    match yaml_parse(unwrap(read_text("anchor.yml"))):
        Ok(doc):
            assert(false)
        Err(problem):
            assert(problem.code == "YAML_UNSUPPORTED")
            assert(problem.line == 1)
            assert(problem.column == 11)
"#;
    std::fs::write(scratch.0.join("main.gu"), source).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["test", "main.gu"])
        .current_dir(&scratch.0)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("2 tests passed"));
}

#[test]
fn yaml_parse_is_gated_on_format_72() {
    let scratch = Scratch::new("yaml-format-gate");
    std::fs::write(
        scratch.0.join("main.gu"),
        "import \"std:yaml\"\n\nfn main() -> Int:\n    match yaml_parse(\"a: 1\"):\n        Ok(v):\n            return 0\n        Err(e):\n            return 1\n",
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["build", "main.gu", "--output", "main.gub"])
        .current_dir(&scratch.0)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut bytes = std::fs::read(scratch.0.join("main.gub")).unwrap();
    assert_eq!(&bytes[0..4], b"VRTA");
    let format = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
    assert!(format >= 72);
    bytes[4..8].copy_from_slice(&71u32.to_le_bytes());
    std::fs::write(scratch.0.join("old.gub"), &bytes).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", "old.gub"])
        .current_dir(&scratch.0)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("E_ARTIFACT_VERSION"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
