use std::process::Command;

fn run_source(name: &str, source: &str) -> std::process::Output {
    let dir = std::env::temp_dir().join(format!("guion-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("main.gu"), source).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", "main.gu"])
        .current_dir(&dir)
        .output()
        .expect("guion starts");
    let _ = std::fs::remove_dir_all(&dir);
    output
}

#[test]
fn math_conversions_and_random_produce_exact_known_answers() {
    let source = r#"import "std:math"
import "std:random"
fn main() -> Int:
    assert(math_sin(0.0) == 0.0)
    assert(math_cos(0.0) == 1.0)
    assert(math_pow(2.0, 10.0) == 1024.0)
    assert(math_floor_int(-2.5) == -3)
    assert(math_ceil_int(-2.5) == -2)
    assert(math_round_int(2.5) == 3)
    assert(math_abs(-3.5) == 3.5)
    assert(math_hypot(3.0, 4.0) == 5.0)
    assert(math_atan2(1.0, 1.0) * 4.0 == math_pi())
    assert(int_to_float(7) / 2.0 == 3.5)
    assert(float_to_int(-3.99) == -3)
    assert(float_to_int(123456789012345678901234567890.0) > 1000000000000000000000)
    assert(int_to_text(-42) == "-42")
    assert(float_to_text(1.5) == "1.5")
    assert(int_to_float(float_to_int(0.1 + 0.2) + 1) == 1.0)
    let first = random_next_u64(random_new(0))
    assert(first.value == 16294208416658607535)
    let again = random_next_u64(random_new(0))
    assert(again.value == first.value)
    let next = random_next_u64(first.rng)
    assert(next.value != first.value)
    var rng = random_new(7)
    var i = 0
    while i < 200:
        let draw = random_next_range(rng, -3, 4)
        assert(draw.value >= -3)
        assert(draw.value < 4)
        let unit = random_next_float(draw.rng)
        assert(unit.value >= 0.0)
        assert(unit.value < 1.0)
        set rng = unit.rng
        set i = i + 1
    return 0
"#;
    let output = run_source("math-random", source);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn math_domain_and_overflow_errors_carry_stable_codes() {
    let cases = [
        ("math_asin(2.0)", "R_MATH_DOMAIN"),
        ("math_acos(-1.5)", "R_MATH_DOMAIN"),
        ("math_ln(0.0)", "R_MATH_DOMAIN"),
        ("math_log10(-1.0)", "R_MATH_DOMAIN"),
        ("math_pow(-8.0, 0.5)", "R_MATH_DOMAIN"),
        ("math_pow(0.0, -1.0)", "R_DIV_ZERO"),
        ("math_exp(1000.0)", "R_FLOAT_NONFINITE"),
    ];
    for (expression, code) in cases {
        let source = format!(
            "import \"std:math\"\nfn main() -> Int:\n    print({expression})\n    return 0\n"
        );
        let output = run_source("math-errors", &source);
        assert!(!output.status.success(), "{expression} should fail");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains(code), "{expression}: {stderr}");
    }
}

#[test]
fn new_math_instructions_are_rejected_in_older_artifact_formats() {
    let dir = std::env::temp_dir().join(format!("guion-math-format-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("main.gu"),
        "import \"std:math\"\nfn main() -> Int:\n    print(math_sin(1.0))\n    return 0\n",
    )
    .unwrap();
    let built = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["build", "main.gu", "--output", "main.gub"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
    let mut bytes = std::fs::read(dir.join("main.gub")).unwrap();
    bytes[4..8].copy_from_slice(&51u32.to_le_bytes());
    std::fs::write(dir.join("old.gub"), bytes).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", "old.gub"])
        .current_dir(&dir)
        .output()
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("E_ARTIFACT_VERSION"));
}
