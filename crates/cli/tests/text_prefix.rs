//! `text_starts_with` and `text_ends_with` in `std:text`: scalar-value prefix and suffix tests that
//! never trap on a short value.

use std::path::PathBuf;
use std::process::Command;

struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run(name: &str, body: &str) -> String {
    let dir = std::env::temp_dir().join(format!("guion-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let scratch = Scratch(dir);
    std::fs::write(
        scratch.0.join("main.gu"),
        format!("import \"std:text\"\n\nfn main() -> Int:\n{body}    return 0\n"),
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", "main.gu"])
        .current_dir(&scratch.0)
        .env("GUION_NO_PROMPT", "1")
        .output()
        .expect("guion starts");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn starts_with_matches_a_folder_prefix() {
    let out = run(
        "starts",
        "    print(text_starts_with(\"/Users/x/target/a\", \"/Users/x/target/\"))\n    print(text_starts_with(\"/\", \"/Users/x/target/\"))\n    print(text_starts_with(\"target2/a\", \"target/\"))\n    print(text_starts_with(\"\", \"\"))\n    print(text_starts_with(\"abc\", \"\"))\n    print(text_starts_with(\"\u{e9}/a\", \"\u{e9}\"))\n    print(text_starts_with(\"a\u{1f642}b\", \"a\u{1f642}\"))\n",
    );
    assert_eq!(out, "true\nfalse\nfalse\ntrue\ntrue\ntrue\ntrue\n");
}

#[test]
fn ends_with_matches_a_suffix() {
    let out = run(
        "ends",
        "    print(text_ends_with(\"a.tar.gz\", \".gz\"))\n    print(text_ends_with(\"gz\", \"a.gz\"))\n    print(text_ends_with(\"x\", \"\"))\n    print(text_ends_with(\"\", \"\"))\n    print(text_ends_with(\"a.tar.gz\", \".tar\"))\n    print(text_ends_with(\"caf\u{e9}\", \"\u{e9}\"))\n    print(text_ends_with(\"a\u{1f642}b\", \"\u{1f642}b\"))\n",
    );
    assert_eq!(out, "true\nfalse\ntrue\ntrue\nfalse\ntrue\ntrue\n");
}
