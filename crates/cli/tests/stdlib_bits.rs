//! `int_and`, `int_or`, `int_xor`, `int_shift_left` and `int_shift_right` (artifact format 83).

use std::path::PathBuf;
use std::process::{Command, Output};

/// The first artifact format that carries the bit operations.
const BIT_OPS_FIRST_FORMAT: u32 = 83;

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

fn run(scratch: &Scratch, source: &str) -> Output {
    std::fs::write(scratch.0.join("main.gu"), source).unwrap();
    Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", "main.gu"])
        .current_dir(&scratch.0)
        .output()
        .expect("guion starts")
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

#[test]
fn and_or_xor_follow_two_complement_rules() {
    let out = stdout_of(
        "bits-logic",
        "fn main() -> Unit:\n    print(int_and(12, 10))\n    print(int_or(12, 10))\n    print(int_xor(12, 10))\n    print(int_and(-1, 255))\n    print(int_or(-8, 3))\n    print(int_xor(-1, 0))\n    print(int_and(9223372036854775807, -1))\n",
    );
    assert_eq!(out, "8\n14\n6\n255\n-5\n-1\n9223372036854775807\n");
}

#[test]
fn shifts_are_arithmetic_and_left_shift_does_not_wrap() {
    let out = stdout_of(
        "bits-shift",
        "fn main() -> Unit:\n    print(int_shift_left(1, 0))\n    print(int_shift_left(1, 10))\n    print(int_shift_left(5, 62))\n    print(int_shift_right(1024, 3))\n    print(int_shift_right(-1, 5))\n    print(int_shift_right(-9, 1))\n    print(int_shift_right(9223372036854775807, 63))\n    print(int_shift_left(-3, 2))\n",
    );
    assert_eq!(out, "1\n1024\n23058430092136939520\n128\n-1\n-5\n0\n-12\n");
}

#[test]
fn a_negative_or_oversized_shift_count_is_a_typed_error() {
    for (call, label) in [
        ("int_shift_left(1, -1)", "negative left"),
        ("int_shift_left(1, 64)", "left 64"),
        ("int_shift_right(1, -1)", "negative right"),
        ("int_shift_right(1, 64)", "right 64"),
        ("int_shift_right(1, 100000000000000000000)", "huge count"),
    ] {
        let err = failure_of(
            "bits-shift-range",
            &format!("fn main() -> Unit:\n    print({call})\n"),
        );
        assert!(err.contains("R_SHIFT_RANGE"), "{label}: {err}");
        assert!(!err.contains("panicked"), "{label}: {err}");
    }
}

#[test]
fn an_operand_wider_than_64_bits_is_a_typed_error() {
    for call in [
        "int_and(9223372036854775808, 1)",
        "int_or(1, -9223372036854775809)",
        "int_xor(100000000000000000000, 1)",
        "int_shift_right(100000000000000000000, 1)",
        "int_shift_left(100000000000000000000, 1)",
    ] {
        let err = failure_of(
            "bits-width",
            &format!("fn main() -> Unit:\n    print({call})\n"),
        );
        assert!(err.contains("R_INTEGER_RANGE"), "{call}: {err}");
        assert!(!err.contains("panicked"), "{call}: {err}");
    }
}

#[test]
fn bit_operations_check_their_operand_types() {
    let err = failure_of(
        "bits-types",
        "fn main() -> Unit:\n    print(int_and(1, \"a\"))\n",
    );
    assert!(!err.contains("panicked"), "{err}");
}

#[test]
fn old_format_rejects_bit_operations() {
    use guion_model::{Artifact, CodecFunction, Op};
    let scratch = Scratch::new("bits-old-format");
    std::fs::write(
        scratch.0.join("main.gu"),
        "fn main() -> Unit:\n    print(int_xor(6, 3))\n",
    )
    .unwrap();
    let built = scratch.0.join("main.gub");
    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["build", "main.gu", "--output", built.to_str().unwrap()])
        .current_dir(&scratch.0)
        .output()
        .expect("guion starts");
    assert!(output.status.success());
    let current = std::fs::read(&built).unwrap();
    let mut artifact = Artifact::from_bytes(&current).unwrap();
    assert!(artifact.functions.iter().any(|function| function
        .code
        .iter()
        .any(|instruction| matches!(instruction.op, Op::Codec(CodecFunction::IntXor)))));
    let previous = BIT_OPS_FIRST_FORMAT - 1;
    artifact.format_version = previous;
    assert_eq!(artifact.to_bytes().unwrap_err().code, "E_ARTIFACT_VERSION");
    let mut old = current;
    old[4..8].copy_from_slice(&previous.to_le_bytes());
    assert_eq!(
        Artifact::from_bytes(&old).unwrap_err().code,
        "E_ARTIFACT_VERSION"
    );
}

#[test]
fn base32_style_packing_round_trips_with_the_operations() {
    // Five bytes into eight 5-bit groups and back, the shape the beacons package uses.
    let out = stdout_of(
        "bits-pack",
        "fn main() -> Unit:\n    let word = int_or(int_shift_left(int_and(171, 255), 8), int_and(205, 255))\n    print(word)\n    print(int_and(int_shift_right(word, 3), 31))\n    print(int_and(word, 7))\n",
    );
    assert_eq!(out, "43981\n25\n5\n");
}
