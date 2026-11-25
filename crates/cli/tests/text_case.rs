//! `text_lower` and `text_upper` (artifact format 90): the Unicode default case mappings.

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
    format!("import \"std:text\"\n\nfn main() -> Int:\n{body}    return 0\n")
}

#[test]
fn ascii_and_empty_text() {
    let out = stdout_of(
        "case-ascii",
        &program(
            "    print(text_lower(\"Hello, WORLD 42!\"))\n    print(text_upper(\"Hello, world 42!\"))\n    print(\"[\" + text_lower(\"\") + \"]\")\n    print(\"[\" + text_upper(\"\") + \"]\")\n",
        ),
    );
    assert_eq!(out, "hello, world 42!\nHELLO, WORLD 42!\n[]\n[]\n");
}

#[test]
fn sharp_s_uppercases_to_two_letters() {
    let out = stdout_of(
        "case-sharp-s",
        &program("    print(text_upper(\"Stra\u{00df}e\"))\n    print(text_lower(\"STRASSE\"))\n"),
    );
    assert_eq!(out, "STRASSE\nstrasse\n");
}

#[test]
fn capital_sigma_ending_a_word_lowercases_to_the_final_form() {
    let out = stdout_of(
        "case-sigma",
        &program("    print(text_lower(\"\u{03a3}\u{0391}\u{03a3}\"))\n    print(text_lower(\"\u{039f}\u{0394}\u{039f}\u{03a3} x\"))\n"),
    );
    assert_eq!(
        out,
        "\u{03c3}\u{03b1}\u{03c2}\n\u{03bf}\u{03b4}\u{03bf}\u{03c2} x\n"
    );
}

#[test]
fn dotted_capital_i_lowers_to_i_and_a_combining_dot() {
    let out = stdout_of(
        "case-dotted-i",
        &program("    let lowered = text_lower(\"\u{0130}\")\n    print(lowered)\n    print(len(lowered))\n"),
    );
    assert_eq!(out, "i\u{0307}\n2\n");
}

#[test]
fn accented_and_non_latin_letters_map() {
    let out = stdout_of(
        "case-accents",
        &program("    print(text_upper(\"\u{00e9}clair \u{0434}\u{043e}\u{043c}\"))\n    print(text_lower(\"\u{00c9}CLAIR \u{0414}\u{041e}\u{041c}\"))\n"),
    );
    assert_eq!(
        out,
        "\u{00c9}CLAIR \u{0414}\u{041e}\u{041c}\n\u{00e9}clair \u{0434}\u{043e}\u{043c}\n"
    );
}

#[test]
fn case_folding_is_still_a_separate_function() {
    let out = stdout_of(
        "case-fold",
        &program("    print(text_casefold(\"Stra\u{00df}e\") == text_casefold(\"STRASSE\"))\n    print(text_lower(\"Stra\u{00df}e\") == text_lower(\"STRASSE\"))\n"),
    );
    assert_eq!(out, "true\nfalse\n");
}

#[test]
fn results_over_the_text_limit_are_refused() {
    // U+0149 is two bytes and uppercases to three, so a text of 12.4 MB (under the 16 MiB limit)
    // would become 18.6 MB.
    let err = failure_of(
        "case-limit",
        "import \"std:text\"\n\nfn main() -> Int:\n    var text = \"\u{0149}\"\n    var doublings = 0\n    while doublings < 22:\n        set text = text + text\n        set doublings += 1\n    set text = text + text_slice(text, 0, 2000000)\n    print(len(text_lower(text)))\n    print(len(text_upper(text)))\n    return 0\n",
    );
    assert!(err.contains("R_TEXT_LIMIT"), "{err}");
}

#[test]
fn old_format_rejects_the_case_instructions() {
    assert_old_format_rejects(
        "case-old-lower",
        "import \"std:text\"\n\nfn main() -> Int:\n    print(text_lower(\"A\"))\n    return 0\n",
        90,
        |op| matches!(op, guion_model::Op::TextLower),
    );
    assert_old_format_rejects(
        "case-old-upper",
        "import \"std:text\"\n\nfn main() -> Int:\n    print(text_upper(\"a\"))\n    return 0\n",
        90,
        |op| matches!(op, guion_model::Op::TextUpper),
    );
}

#[test]
fn a_program_without_the_new_instructions_still_loads_as_format_89() {
    use guion_model::Artifact;
    let scratch = Scratch::new("case-format-89");
    std::fs::write(
        scratch.0.join("main.gu"),
        "fn main() -> Int:\n    return 7\n",
    )
    .unwrap();
    let built = scratch.0.join("main.gub");
    let output = guion(
        &scratch,
        &["build", "main.gu", "--output", built.to_str().unwrap()],
    );
    assert!(output.status.success());
    let mut artifact = Artifact::from_bytes(&std::fs::read(&built).unwrap()).unwrap();
    for format in [14, 88, 89] {
        artifact.format_version = format;
        let bytes = artifact.to_bytes().unwrap();
        assert_eq!(Artifact::from_bytes(&bytes).unwrap().format_version, format);
    }
}

#[test]
fn doc_lists_the_new_functions() {
    let scratch = Scratch::new("case-doc");
    let output = guion(&scratch, &["doc", "std:text"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("text_lower(value: Text) -> Text"), "{text}");
    assert!(text.contains("text_upper(value: Text) -> Text"), "{text}");
}
