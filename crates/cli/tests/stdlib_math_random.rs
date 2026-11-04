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

#[test]
fn random_sequences_are_unchanged_by_the_format_61_generator() {
    // Expected lines were produced by the format 52 generator, which kept the state as a
    // non-negative big integer. The format 61 generator must reproduce them exactly.
    let source = r#"import "std:random"
fn main() -> Int:
    for seed in [0, -7, 18446744073709551616, 99999999999999999999999]:
        var rng = random_new(seed)
        var i = 0
        while i < 2:
            let a = random_next_u64(rng)
            let b = random_next_float(a.rng)
            let c = random_next_range(b.rng, -3, 10)
            let d = random_next_range(c.rng, 0, 9223372036854775807)
            let e = random_next_range(d.rng, 0, 18446744073709551616)
            set rng = e.rng
            print(int_to_text(a.value) + " " + float_to_text(b.value) + " " + int_to_text(c.value) + " " + int_to_text(d.value) + " " + int_to_text(e.value))
            set i = i + 1
    let signed = random_next_i64(random_new(0))
    let unsigned = random_next_u64(random_new(0))
    assert(signed.value == unsigned.value - 18446744073709551616)
    assert(signed.rng == unsigned.rng)
    return 0
"#;
    let output = run_source("random-sequence", source);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "16294208416658607535 0.43152799704850997 -2 8686239339925766637 1961750202426094747\n\
         6038094601263162090 0.17386786595968284 5 4532161160992623299 17561866513979060390\n\
         7790691224305936752 0.4786370309856862 9 2940488688193949890 10439198631842511153\n\
         14902973265903701061 0.5506260133517186 7 519180555181236417 7396466289114802521\n\
         16294208416658607535 0.43152799704850997 -2 8686239339925766637 1961750202426094747\n\
         6038094601263162090 0.17386786595968284 5 4532161160992623299 17561866513979060390\n\
         15550006742871798632 0.5124836449194183 1 8274034911338554874 15193643809786452083\n\
         7102742841129114092 0.1683482969660829 9 8787191789710485866 4318185699763217277\n"
    );
}

#[test]
fn random_ranges_wider_than_64_bits_fail_instead_of_looping() {
    let source = r#"import "std:random"
fn main() -> Int:
    let draw = random_next_range(random_new(1), 0, 18446744073709551617)
    print(draw.value)
    return 0
"#;
    let output = run_source("random-wide-range", source);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("R_RANDOM_RANGE"));
}

#[test]
fn format_61_random_instructions_are_rejected_in_format_60_artifacts() {
    let dir = std::env::temp_dir().join(format!("guion-random-format-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("main.gu"),
        "import \"std:random\"\nfn main() -> Int:\n    print(random_next_float(random_new(3)).value)\n    return 0\n",
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
    bytes[4..8].copy_from_slice(&60u32.to_le_bytes());
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

#[test]
fn command_line_programs_have_no_instruction_limit_and_a_memory_in_use_limit() {
    // Far more than 2,000,000 instructions and far more than 256 MiB allocated in total, while
    // holding almost nothing: both used to fail under the command line's defaults.
    let churn = r#"fn main() -> Unit:
    var kept = 0
    var i = 0
    while i < 3000000:
        let line = "row " + int_to_text(i) + " with a little padding to make it allocate"
        set kept = kept + len(line)
        set i = i + 1
    print(kept)
"#;
    let output = run_source("live-memory-churn", churn);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Memory actually held still stops at the limit.
    let dir = std::env::temp_dir().join(format!("guion-live-memory-hold-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("guion.toml"),
        "[project]\nname = \"hold\"\nversion = \"0.1.0\"\nentry = \"main.gu\"\n\n[runtime]\nallocation_limit_bytes = 40000000\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("main.gu"),
        "fn main() -> Unit:\n    var items: List[Text] = []\n    var i = 0\n    while i < 2000000:\n        set items = list_push(items, \"held item \" + int_to_text(i) + \" with padding text\")\n        set i = i + 1\n    print(len(items))\n",
    )
    .unwrap();
    let held = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", "main.gu"])
        .current_dir(&dir)
        .output()
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(!held.status.success());
    let stderr = String::from_utf8_lossy(&held.stderr);
    assert!(
        stderr.contains("R_ALLOCATION_LIMIT") && stderr.contains("memory limit reached"),
        "{stderr}"
    );
}
