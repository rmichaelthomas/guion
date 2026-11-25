//! `int_to_text_radix` and `text_parse_int_radix` (artifact format 90): integers as text in radix 2 to 36.

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
fn int_to_text_radix_writes_lowercase_digits_without_prefix() {
    let out = stdout_of(
        "radix-write",
        &program(
            "    print(int_to_text_radix(255, 16))\n    print(int_to_text_radix(255, 2))\n    print(int_to_text_radix(493, 8))\n    print(int_to_text_radix(35, 36))\n    print(int_to_text_radix(0, 2))\n    print(int_to_text_radix(0, 36))\n    print(int_to_text_radix(1234567, 10))\n    print(int_to_text_radix(-255, 16))\n    print(int_to_text_radix(-1, 2))\n    print(int_to_text_radix(0xDEADBEEF, 16))\n",
        ),
    );
    assert_eq!(
        out,
        "ff\n11111111\n755\nz\n0\n0\n1234567\n-ff\n-1\ndeadbeef\n"
    );
}

#[test]
fn int_to_text_radix_handles_the_ends_of_the_machine_range_and_beyond() {
    let out = stdout_of(
        "radix-big",
        &program(
            "    let top = 9223372036854775807\n    print(int_to_text_radix(top, 16))\n    print(int_to_text_radix(-top - 1, 16))\n    print(int_to_text_radix(top + 1, 16))\n    print(int_to_text_radix(0xFFFF_FFFF_FFFF_FFFF_FFFF_FFFF, 16))\n    print(int_to_text_radix(-0xFFFF_FFFF_FFFF_FFFF_FFFF_FFFF, 2))\n    print(int_to_text_radix(123456789012345678901234567890, 36))\n",
        ),
    );
    assert_eq!(
        out,
        format!("7fffffffffffffff\n-8000000000000000\n8000000000000000\nffffffffffffffffffffffff\n-{}\n{EXPECTED_36}\n", "1".repeat(96))
    );
}

/// 123456789012345678901234567890 in base 36, computed independently with Python's arithmetic.
const EXPECTED_36: &str = "byw97um9s91dlz68tsi";

#[test]
fn text_parse_int_radix_reads_either_case_and_a_leading_minus() {
    let out = stdout_of(
        "radix-parse",
        &program(
            "    print(unwrap(text_parse_int_radix(\"ff\", 16)))\n    print(unwrap(text_parse_int_radix(\"FF\", 16)))\n    print(unwrap(text_parse_int_radix(\"-ff\", 16)))\n    print(unwrap(text_parse_int_radix(\"755\", 8)))\n    print(unwrap(text_parse_int_radix(\"1010\", 2)))\n    print(unwrap(text_parse_int_radix(\"Zz\", 36)))\n    print(unwrap(text_parse_int_radix(\"000\", 2)))\n    print(unwrap(text_parse_int_radix(\"-0\", 10)))\n    print(unwrap(text_parse_int_radix(\"ffffffffffffffffffffffff\", 16)))\n    print(unwrap(text_parse_int_radix(\"-8000000000000000\", 16)))\n    print(unwrap(text_parse_int_radix(\"8000000000000000\", 16)))\n",
        ),
    );
    assert_eq!(
        out,
        "255\n255\n-255\n493\n10\n1295\n0\n0\n79228162514264337593543950335\n-9223372036854775808\n9223372036854775808\n"
    );
}

#[test]
fn text_parse_int_radix_rejects_anything_but_digits_and_one_minus() {
    let out = stdout_of(
        "radix-parse-bad",
        &program(
            "    for text in [\"\", \"-\", \"0x1f\", \"1_0\", \"+5\", \" 5\", \"5 \", \"g\", \"--1\", \"1.5\", \"\\u{661}\"]:\n        match text_parse_int_radix(text, 16):\n            Ok(_):\n                print(\"ok\")\n            Err(problem):\n                print(file_error_code(problem))\n    match text_parse_int_radix(\"12\", 2):\n        Ok(_):\n            print(\"ok\")\n        Err(problem):\n            print(file_error_code(problem))\n",
        ),
    );
    assert_eq!(out, "E_INT_PARSE\n".repeat(12));
}

#[test]
fn a_radix_outside_2_to_36_is_a_runtime_error() {
    for (name, call) in [
        ("w0", "int_to_text_radix(1, 0)"),
        ("w1", "int_to_text_radix(1, 1)"),
        ("w37", "int_to_text_radix(1, 37)"),
        ("wneg", "int_to_text_radix(1, -2)"),
        ("p0", "text_parse_int_radix(\"1\", 0)"),
        ("p1", "text_parse_int_radix(\"1\", 1)"),
        ("p37", "text_parse_int_radix(\"1\", 37)"),
    ] {
        let err = failure_of(
            &format!("radix-range-{name}"),
            &format!("import \"std:text\"\n\nfn main() -> Int:\n    print({call})\n    return 0\n"),
        );
        assert!(err.contains("R_RADIX_RANGE"), "{call}: {err}");
    }
}

#[test]
fn the_two_functions_round_trip_in_every_radix() {
    let out = stdout_of(
        "radix-round-trip",
        &program(
            "    let values = [0, 1, -1, 35, 36, 255, -4096, 9223372036854775807, -9223372036854775807 - 1, 9223372036854775808, 123456789012345678901234567890, -98765432109876543210987654321098765432109876543210]\n    var bad = 0\n    for radix in range(2, 37):\n        for value in values:\n            if unwrap(text_parse_int_radix(int_to_text_radix(value, radix), radix)) != value:\n                set bad += 1\n    print(bad)\n",
        ),
    );
    assert_eq!(out, "0\n");
}

#[test]
fn radix_ten_agrees_with_int_to_text_and_text_parse_int() {
    let out = stdout_of(
        "radix-ten",
        &program(
            "    for value in [0, 7, -7, 1000000, 123456789012345678901234567890]:\n        print(int_to_text_radix(value, 10) == int_to_text(value) and unwrap(text_parse_int_radix(int_to_text(value), 10)) == value)\n",
        ),
    );
    assert_eq!(out, "true\n".repeat(5));
}

#[test]
fn a_parsed_integer_over_the_digit_limit_is_refused() {
    let digits = "f".repeat(5000);
    let err = failure_of(
        "radix-digit-limit",
        &format!("import \"std:text\"\n\nfn main() -> Int:\n    print(text_parse_int_radix(\"{digits}\", 16))\n    return 0\n"),
    );
    assert!(err.contains("R_INTEGER_LIMIT"), "{err}");
}

#[test]
fn old_format_rejects_the_radix_instructions() {
    assert_old_format_rejects(
        "radix-old-write",
        "import \"std:text\"\n\nfn main() -> Int:\n    print(int_to_text_radix(255, 16))\n    return 0\n",
        90,
        |op| matches!(op, guion_model::Op::IntToTextRadix),
    );
    assert_old_format_rejects(
        "radix-old-parse",
        "import \"std:text\"\n\nfn main() -> Int:\n    print(text_parse_int_radix(\"ff\", 16))\n    return 0\n",
        90,
        |op| matches!(op, guion_model::Op::TextParseIntRadix),
    );
}

#[test]
fn doc_lists_the_new_functions() {
    let scratch = Scratch::new("radix-doc");
    let output = guion(&scratch, &["doc", "std:text"]);
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        text.contains("int_to_text_radix(value: Int, radix: Int) -> Text"),
        "{text}"
    );
    assert!(
        text.contains("text_parse_int_radix(text: Text, radix: Int) -> Result[Int, FileError]"),
        "{text}"
    );
}
