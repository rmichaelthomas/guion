use std::process::Command;

fn run(name: &str, source: &str, instruction_limit: u64) -> std::process::Output {
    let dir = std::env::temp_dir().join(format!("guion-native-math-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("src/main.gu"), source).unwrap();
    std::fs::write(
        dir.join("guion.toml"),
        format!(
            "[project]\nname = \"m\"\nversion = \"0.1.0\"\nentry = \"src/main.gu\"\n\n[runtime]\ninstruction_limit = {instruction_limit}\n"
        ),
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .arg("run")
        .arg("src/main.gu")
        .current_dir(&dir)
        .output()
        .expect("guion starts");
    let _ = std::fs::remove_dir_all(&dir);
    output
}

// A hot loop is compiled to machine code; its results and its errors must be the VM's.
#[test]
fn math_in_a_hot_loop_matches_the_vm_results_and_errors() {
    let source = r#"import "std:math"
fn sum(n: Int) -> Float64:
    var i = 0
    var total = 0.0
    while i < n:
        let x = int_to_float(i) * 0.5
        set total = total + math_sin(x) + math_floor(x) + math_pow(x, 2.0) + math_atan2(x, 3.0)
        set i = i + 1
    return total

fn main() -> Int:
    print(sum(5000))
    print(float_to_int(123456.9))
    print(float_to_int(-7.9))
    print(float_to_int(1.0e19))
    return 0
"#;
    let output = run("parity", source, 2_000_000_000);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[1], "123456");
    assert_eq!(lines[2], "-7");
    assert_eq!(lines[3], "10000000000000000000");
}

#[test]
fn native_math_domain_errors_report_the_vm_error() {
    let source = r#"import "std:math"
fn run(n: Int) -> Float64:
    var i = 0
    var total = 0.0
    while i < n:
        set total = total + math_ln(int_to_float(5000 - i))
        set i = i + 1
    return total

fn main() -> Int:
    print(run(5001))
    return 0
"#;
    let output = run("domain", source, 2_000_000_000);
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("R_MATH_DOMAIN"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
