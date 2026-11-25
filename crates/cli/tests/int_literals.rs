//! Integer literals in hexadecimal, octal and binary, and `_` as a digit separator.

use std::path::PathBuf;
use std::process::{Command, Output};

struct Scratch(PathBuf);
impl Scratch {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("guion-intlit-{name}-{}", std::process::id()));
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

fn run(name: &str, source: &str) -> Output {
    let scratch = Scratch::new(name);
    std::fs::write(scratch.0.join("main.gu"), source).unwrap();
    guion(&scratch, &["run", "main.gu"])
}

/// Prints every expression of `expressions`, one per line, and returns stdout.
fn values(name: &str, expressions: &[&str]) -> Vec<String> {
    let body = expressions
        .iter()
        .map(|expression| format!("    print({expression})\n"))
        .collect::<String>();
    let output = run(name, &format!("fn main() -> Int:\n{body}    return 0\n"));
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::to_owned)
        .collect()
}

/// The diagnostic line of a program that must not compile.
fn rejected(name: &str, literal: &str) -> String {
    let output = run(
        name,
        &format!("fn main() -> Int:\n    let value = {literal}\n    return 0\n"),
    );
    assert!(!output.status.success(), "{literal} compiled");
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn prefixed_literals_have_the_value_of_their_base() {
    let printed = values(
        "bases",
        &[
            "0xFF",
            "0Xff",
            "0o755",
            "0O17",
            "0b1010",
            "0B11",
            "0x0",
            "0b0",
            "0o0",
            "-0x10",
            "0xDEADBEEF",
        ],
    );
    assert_eq!(
        printed,
        [
            "255",
            "255",
            "493",
            "15",
            "10",
            "3",
            "0",
            "0",
            "0",
            "-16",
            "3735928559"
        ]
    );
}

#[test]
fn hex_literals_with_an_e_are_integers_not_floats() {
    assert_eq!(
        values("hex-e", &["0xE", "0x1e5", "0xfe"]),
        ["14", "485", "254"]
    );
}

#[test]
fn separators_sit_between_digits_in_every_base() {
    let printed = values(
        "separators",
        &["1_000_000", "0xFF_FF", "0b1111_0000", "0o7_5_5", "1_0"],
    );
    assert_eq!(printed, ["1000000", "65535", "240", "493", "10"]);
}

#[test]
fn big_literals_are_arbitrary_precision() {
    let printed = values(
        "big",
        &[
            "0xFFFFFFFFFFFFFFFFFFFFFFFF",
            "0b1_0000000000000000000000000000000000000000000000000000000000000000",
            "0o7777777777777777777777777",
            "9_223_372_036_854_775_808",
            "1_000_000_000_000_000_000_000",
        ],
    );
    assert_eq!(
        printed,
        [
            "79228162514264337593543950335",
            "18446744073709551616",
            "37778931862957161709567",
            "9223372036854775808",
            "1000000000000000000000",
        ]
    );
}

#[test]
fn decimal_zero_followed_by_digits_is_as_before() {
    assert_eq!(
        values("zeros", &["007", "00", "0", "010"]),
        ["7", "0", "0", "10"]
    );
}

#[test]
fn separators_work_in_floats_too() {
    assert_eq!(
        values("floats", &["1_000.5", "2.5e1_0 > 1.0", "1.0_1"]),
        ["1000.5", "true", "1.01"]
    );
}

#[test]
fn literals_work_in_expressions_and_comparisons() {
    let output = run(
        "patterns",
        "fn main() -> Int:\n    let mask = 0xF0 + 0b1111\n    if mask == 0xFF:\n        print(\"match\")\n    print(0x10 * 1_0)\n    return 0\n",
    );
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(text, "match\n160\n");
}

fn assert_error(name: &str, literal: &str, line: usize, column: usize) {
    let text = rejected(name, literal);
    assert!(text.contains("E_NUMBER_LITERAL"), "{literal}: {text}");
    assert!(
        text.contains(&format!("main.gu:{line}:{column}: error[E_NUMBER_LITERAL]")),
        "{literal} expected column {column}: {text}"
    );
}

#[test]
fn a_separator_that_is_not_between_two_digits_is_an_error() {
    // `    let value = ` is 16 characters, so the literal starts at column 17.
    assert_error("double", "1__0", 2, 18);
    assert_error("trailing", "1_", 2, 18);
    assert_error("hex-leading", "0x_F", 2, 19);
    assert_error("hex-trailing", "0xF_", 2, 20);
    assert_error("bin-double", "0b1__1", 2, 20);
}

#[test]
fn a_prefix_without_digits_is_an_error() {
    assert_error("hex-empty", "0x", 2, 17);
    assert_error("bin-empty", "0B", 2, 17);
    assert_error("oct-empty", "0o", 2, 17);
}

#[test]
fn a_digit_outside_the_base_is_an_error() {
    assert_error("bin-two", "0b2", 2, 19);
    assert_error("bin-late", "0b1012", 2, 22);
    assert_error("oct-eight", "0o8", 2, 19);
    assert_error("hex-g", "0xFG", 2, 20);
}

#[test]
fn the_formatter_keeps_literals_as_written() {
    let scratch = Scratch::new("fmt");
    let source = "fn main() -> Int:\n    let a = 0xFF_ff + 0b1010 - 0o755\n    let b = 1_000_000 * 0XAB\n    let c = 1_000.5\n    return a+b\n";
    std::fs::write(scratch.0.join("main.gu"), source).unwrap();
    let output = guion(&scratch, &["fmt", "main.gu"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let formatted = std::fs::read_to_string(scratch.0.join("main.gu")).unwrap();
    assert_eq!(
        formatted,
        "fn main() -> Int:\n    let a = 0xFF_ff + 0b1010 - 0o755\n    let b = 1_000_000 * 0XAB\n    let c = 1_000.5\n    return a + b\n"
    );
}
