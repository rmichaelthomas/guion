//! Format-63 array kernels used by the `frames` package: gather, factorize, segmented
//! reductions by group code, and hash joins on code arrays.

use std::process::Command;

fn run(name: &str, lines: &str) -> (bool, String, String) {
    let dir = std::env::temp_dir().join(format!(
        "guion-frames-kernels-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut source = String::from("fn main() -> Unit:\n");
    for line in lines.lines() {
        source.push_str("    ");
        source.push_str(line);
        source.push('\n');
    }
    std::fs::write(dir.join("main.gu"), source).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", "main.gu"])
        .current_dir(&dir)
        .output()
        .expect("guion starts");
    let _ = std::fs::remove_dir_all(&dir);
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn prints(name: &str, lines: &str, expected: &str) {
    let (ok, stdout, stderr) = run(name, lines);
    assert!(ok, "{name} failed: {stderr}");
    assert_eq!(stdout, expected, "{name}");
}

fn fails_with(name: &str, lines: &str, code: &str) {
    let (ok, stdout, stderr) = run(name, lines);
    assert!(!ok, "{name} unexpectedly succeeded: {stdout}");
    assert!(
        stderr.contains(code),
        "{name}: expected {code}, got {stderr}"
    );
    assert!(!stderr.contains("panicked"), "{name} panicked: {stderr}");
}

#[test]
fn take_gathers_by_index() {
    prints(
        "take",
        "print(array_take(array_f64([1.5, 2.5, 3.5]), array_i64([2, 0, 2])))\nprint(array_take(array_bool([true, false]), array_i64([])))",
        "[3.5, 1.5, 3.5]\n[]\n",
    );
    fails_with(
        "take-range",
        "print(array_take(array_i64([1]), array_i64([1])))",
        "R_ARRAY_INDEX",
    );
    fails_with(
        "take-neg",
        "print(array_take(array_i64([1]), array_i64([-1])))",
        "R_ARRAY_INDEX",
    );
}

#[test]
fn factorize_gives_first_seen_codes() {
    prints(
        "factorize",
        "print(array_factorize(array_i64([7, 3, 7, 9, 3])))\nprint(array_factorize(array_f64([0.0, -0.0, 1.0])))\nprint(array_factorize(array_bool([false, true, false])))",
        "[0, 1, 0, 2, 1]\n[0, 0, 1]\n[0, 1, 0]\n",
    );
}

#[test]
fn segmented_reductions_by_code() {
    prints(
        "by",
        "let codes = array_i64([0, 1, 0, 2, 1])\nprint(array_first_index(codes, 3))\nprint(array_count_by(codes, 3))\nprint(array_sum_by(array_i64([1, 2, 3, 4, 5]), codes, 3))\nprint(array_min_by(array_i64([1, 2, 3, 4, 5]), codes, 3))\nprint(array_max_by(array_f64([1.0, 2.0, 3.0, 4.0, 5.0]), codes, 3))\nprint(array_mean_by(array_f64([1.0, 2.0, 3.0, 4.0, 5.0]), codes, 3))\nprint(array_sum_by(array_i64([1]), array_i64([0]), 2))\nprint(array_count_by(array_i64([]), 2))\nprint(array_first_index(array_i64([1]), 2))",
        "[0, 1, 3]\n[2, 2, 1]\n[4, 7, 4]\n[1, 2, 4]\n[3, 5, 4]\n[2, 3.5, 4]\n[1, 0]\n[0, 0]\n[-1, 0]\n",
    );
}

#[test]
fn group_kernels_reject_out_of_range_codes() {
    fails_with(
        "count-high",
        "print(array_count_by(array_i64([0, 3]), 3))",
        "R_ARRAY_INDEX",
    );
    fails_with(
        "sum-neg",
        "print(array_sum_by(array_i64([1]), array_i64([-1]), 1))",
        "R_ARRAY_INDEX",
    );
    fails_with(
        "groups-neg",
        "print(array_count_by(array_i64([]), -1))",
        "R_ARRAY_SHAPE",
    );
    fails_with(
        "len",
        "print(array_sum_by(array_i64([1, 2]), array_i64([0]), 1))",
        "R_ARRAY_SHAPE",
    );
    fails_with(
        "min-empty",
        "print(array_min_by(array_i64([1]), array_i64([0]), 2))",
        "R_ARRAY_EMPTY",
    );
    fails_with(
        "mean-empty",
        "print(array_mean_by(array_f64([1.0]), array_i64([0]), 2))",
        "R_ARRAY_EMPTY",
    );
}

#[test]
fn sum_by_checks_integer_overflow() {
    fails_with(
        "overflow",
        "print(array_sum_by(array_i8([100, 100]), array_i64([0, 0]), 1))",
        "R_ARRAY_OVERFLOW",
    );
}

#[test]
fn join_index_pairs_rows_and_never_matches_negative_codes() {
    prints(
        "join",
        "print(array_join_index(array_i64([0, 1, 2, -1]), array_i64([1, 0, 1, -1]), false))\nprint(array_join_index(array_i64([0, 1, 2, -1]), array_i64([1, 0, 1, -1]), true))",
        "[0, 1, 1, 1, 0, 2]\n[0, 1, 1, 2, 3, 1, 0, 2, -1, -1]\n",
    );
}
