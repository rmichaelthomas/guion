use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_RUN: AtomicUsize = AtomicUsize::new(0);

struct Run {
    stdout: String,
    stderr: String,
    success: bool,
}

fn scratch() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "guion-native-scalar-args-{}-{}",
        std::process::id(),
        NEXT_RUN.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn guion(dir: &std::path::Path, args: &[&str]) -> Run {
    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(args)
        .current_dir(dir)
        .output()
        .expect("guion starts");
    Run {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        success: output.status.success(),
    }
}

/// Runs the source from text and again as a built artifact, and returns both results.
fn run_both_ways(source: &str) -> [Run; 2] {
    let dir = scratch();
    std::fs::write(dir.join("main.gu"), source).unwrap();
    let from_source = guion(&dir, &["run", "main.gu"]);
    let build = guion(&dir, &["build", "main.gu", "-o", "main.gub"]);
    assert!(build.success, "{}{}", build.stdout, build.stderr);
    let from_artifact = guion(&dir, &["run", "main.gub"]);
    let _ = std::fs::remove_dir_all(&dir);
    [from_source, from_artifact]
}

fn assert_prints(source: &str, expected: &str) {
    for (label, run) in ["source", "artifact"].iter().zip(run_both_ways(source)) {
        assert!(run.success, "{label}: {}{}", run.stdout, run.stderr);
        assert_eq!(run.stdout, expected, "{label}: {}", run.stderr);
    }
}

// A Bool argument reaches native code as a raw 0 or 1. When the caller stays in the VM (here
// because it prints), the entry into the native callee used to pass an arena handle instead,
// which is nonzero, so `false` arrived as true.
#[test]
fn bool_arguments_from_a_printing_caller_keep_their_value() {
    let source = r#"fn a(flag: Bool) -> Int:
    if flag:
        return 1
    return 0

fn main() -> Int:
    print("x")
    assert(a(false) == 0)
    assert(a(true) == 1)
    return 0
"#;
    assert_prints(source, "x\n");
}

#[test]
fn bool_arguments_without_a_print_keep_their_value() {
    let source = r#"fn a(flag: Bool) -> Int:
    if flag:
        return 1
    return 0

fn main() -> Int:
    assert(a(false) == 0)
    assert(a(true) == 1)
    return 0
"#;
    assert_prints(source, "");
}

#[test]
fn bool_variable_and_expression_arguments_keep_their_value() {
    let source = r#"fn a(flag: Bool) -> Int:
    if flag:
        return 1
    return 0

fn main() -> Int:
    print("x")
    let off = false
    let on = true
    let n = 3
    print(a(off))
    print(a(on))
    print(a(1 > 2))
    print(a(2 > 1))
    print(a(n == 3))
    print(a(not on))
    return 0
"#;
    assert_prints(source, "x\n0\n1\n0\n1\n1\n0\n");
}

#[test]
fn several_bool_arguments_are_each_independent() {
    let source = r#"fn pick(a: Bool, b: Bool, c: Bool) -> Int:
    var total = 0
    if a:
        set total = total + 1
    if b:
        set total = total + 10
    if c:
        set total = total + 100
    return total

fn main() -> Int:
    print("x")
    print(pick(false, false, false))
    print(pick(true, false, false))
    print(pick(false, true, false))
    print(pick(false, false, true))
    print(pick(true, false, true))
    return 0
"#;
    assert_prints(source, "x\n0\n1\n10\n100\n101\n");
}

#[test]
fn bool_results_and_mixed_scalar_arguments_survive_the_entry() {
    let source = r#"fn mix(n: Int, flag: Bool, x: Float64) -> Bool:
    if flag:
        return n > 0
    return x > 1.0

fn main() -> Int:
    print("x")
    print(mix(0, false, 0.5))
    print(mix(0, false, 2.5))
    print(mix(1, true, 0.5))
    print(mix(0, true, 0.5))
    return 0
"#;
    assert_prints(source, "x\nfalse\ntrue\ntrue\nfalse\n");
}

#[test]
fn int_zero_arguments_from_a_printing_caller_stay_zero() {
    let source = r#"fn sign(n: Int) -> Int:
    if n == 0:
        return 100
    return n

fn main() -> Int:
    print("x")
    assert(sign(0) == 100)
    assert(sign(7) == 7)
    return 0
"#;
    assert_prints(source, "x\n");
}

#[test]
fn unit_results_from_a_printing_caller_are_unit() {
    let source = r#"fn quiet(flag: Bool) -> Unit:
    let kept = flag

fn main() -> Int:
    print("x")
    quiet(false)
    quiet(true)
    return 0
"#;
    assert_prints(source, "x\n");
}
