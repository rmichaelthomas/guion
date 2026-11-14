//! The deprecation marker of VERSIONING.md section 7: a doc comment on a public function,
//! `# deprecated(since 0.3.0): use NAME`, reported by `guion doc` and warned about by `guion lint`.
use std::path::PathBuf;
use std::process::{Command, Output};

const LIBRARY: &str = r#"# A fixture library.

# Adds one.
# deprecated(since 0.3.0): use add_one
pub fn increment(value: Int) -> Int:
    return value + 1

pub fn add_one(value: Int) -> Int:
    return value + 1

# Not marked: an ordinary comment that mentions deprecated(since 1.0.0) in passing.
pub fn plain(value: Int) -> Int:
    return value
"#;

const MAIN: &str = r#"import "lib.gu"

fn main() -> Int:
    let a = increment(1)
    let b = add_one(a)
    return plain(b) - 3
"#;

struct Fixture(PathBuf);

impl Fixture {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("guion-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("lib.gu"), LIBRARY).unwrap();
        std::fs::write(dir.join("main.gu"), MAIN).unwrap();
        Fixture(dir)
    }

    fn guion(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_guion"))
            .current_dir(&self.0)
            .args(args)
            .output()
            .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn the_marker_shows_in_doc_text() {
    let fixture = Fixture::new("deprecation-text");
    let output = fixture.guion(&["doc", "main.gu"]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    let out = text(&output.stdout);
    let line = |name: &str| {
        out.lines()
            .find(|line| line.contains(&format!("fn {name}(")))
            .unwrap_or_else(|| panic!("no line for {name} in:\n{out}"))
            .to_owned()
    };
    assert!(line("increment").contains("deprecated since 0.3.0: use add_one"));
    assert!(!line("add_one").contains("deprecated"));
    assert!(!line("plain").contains("deprecated"));
}

#[test]
fn the_marker_shows_in_doc_json_and_unmarked_functions_are_unchanged() {
    let fixture = Fixture::new("deprecation-json");
    let output = fixture.guion(&["doc", "--json", "main.gu"]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let functions = report["functions"].as_array().unwrap();
    let find = |name: &str| {
        functions
            .iter()
            .find(|function| function["name"].as_str().is_some_and(|n| n.ends_with(name)))
            .unwrap_or_else(|| panic!("no function {name}"))
    };
    assert_eq!(
        find("increment")["deprecated"],
        serde_json::json!({"since": "0.3.0", "use": "add_one"})
    );
    for name in ["add_one", "plain"] {
        assert!(
            find(name).get("deprecated").is_none(),
            "{name} gained a deprecated field"
        );
    }
}

#[test]
fn a_call_site_gets_the_lint_warning_and_nothing_else_does() {
    let fixture = Fixture::new("deprecation-lint");
    let output = fixture.guion(&["lint", "--json", "main.gu"]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let diagnostics = report["diagnostics"].as_array().unwrap();
    let deprecated = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic["code"] == "W_DEPRECATED_CALL")
        .collect::<Vec<_>>();
    assert_eq!(deprecated.len(), 1, "{diagnostics:?}");
    assert_eq!(deprecated[0]["line"], 4);
    assert_eq!(deprecated[0]["column"], 13);
    assert_eq!(
        deprecated[0]["message"],
        "`increment` is deprecated since 0.3.0: use `add_one`"
    );
    assert_eq!(deprecated[0]["severity"], "warning");
    // The deprecated function still runs exactly as before.
    let run = fixture.guion(&["run", "main.gu"]);
    assert!(run.status.success(), "{}", text(&run.stderr));
}

#[test]
fn a_program_that_calls_only_unmarked_functions_lints_clean() {
    let fixture = Fixture::new("deprecation-clean");
    std::fs::write(
        fixture.0.join("main.gu"),
        "import \"lib.gu\"\n\nfn main() -> Int:\n    return plain(add_one(1))\n",
    )
    .unwrap();
    let output = fixture.guion(&["lint", "main.gu"]);
    assert!(output.status.success());
    assert!(
        text(&output.stdout).contains("no warnings"),
        "{}",
        text(&output.stderr)
    );
}
