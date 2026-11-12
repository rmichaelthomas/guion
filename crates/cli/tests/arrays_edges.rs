//! Edge cases found in the whole-branch review: memory accounting for reshape and flatten,
//! non-finite axis means, unsigned matmul overflow, empty float sums and huge zero-size extents.

use std::process::Command;

fn run(name: &str, lines: &str) -> (bool, String, String) {
    let dir =
        std::env::temp_dir().join(format!("guion-arrays-edges-{name}-{}", std::process::id()));
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

fn fails_with(name: &str, lines: &str, code: &str) {
    let (ok, stdout, stderr) = run(name, lines);
    assert!(!ok, "{name} unexpectedly succeeded: {stdout}");
    assert!(
        stderr.contains(code),
        "{name}: expected {code}, got {stderr}"
    );
    assert!(!stderr.contains("panicked"), "{name} panicked: {stderr}");
}

// Five 64 MB copies held at once exceed the 256 MiB default budget.
#[test]
fn reshape_copies_are_charged_to_the_allocation_budget() {
    fails_with(
        "reshape",
        "let a = array_f64_zeros([8000000])\nvar kept: List[Array[f64]] = []\nvar i = 0\nwhile i < 5:\n    set kept = list_push(kept, array_reshape(a, [-1]))\n    set i = i + 1\nprint(len(kept))",
        "R_ALLOCATION_LIMIT",
    );
}

#[test]
fn flatten_copies_are_charged_to_the_allocation_budget() {
    fails_with(
        "flatten",
        "let a = array_f64_zeros([4000, 2000])\nvar kept: List[Array[f64]] = []\nvar i = 0\nwhile i < 5:\n    set kept = list_push(kept, array_flatten(a))\n    set i = i + 1\nprint(len(kept))",
        "R_ALLOCATION_LIMIT",
    );
}

#[test]
fn an_axis_mean_that_overflows_raises_instead_of_storing_infinity() {
    fails_with(
        "mean-axis",
        "let m = array_mean_axis(array_reshape(array_f64([1.0e308, 1.0e308]), [2, 1]), 0)\nprint(array_get(m, 0))",
        "R_FLOAT_NONFINITE",
    );
}

#[test]
fn unsigned_matmul_overflow_raises_instead_of_wrapping() {
    fails_with(
        "matmul-u64",
        "let a = array_reshape(array_u64([18446744073709551615, 8589934592]), [1, 2])\nlet b = array_u64([18446744073709551615, 4294967296])\nprint(array_matmul(a, b))",
        "R_ARRAY_OVERFLOW",
    );
    fails_with(
        "matmul-i64-sum",
        "let a = array_reshape(array_i64([9223372036854775807, 9223372036854775807]), [1, 2])\nlet b = array_i64([9223372036854775807, 9223372036854775807])\nprint(array_matmul(a, b))",
        "R_ARRAY_OVERFLOW",
    );
}

#[test]
fn empty_float_sums_are_positive_zero() {
    let (ok, stdout, stderr) = run(
        "empty-sum",
        "print(array_sum(array_f64_zeros([0])))\nprint(array_sum(array_f32_zeros([0])))\nprint(array_dot(array_f64_zeros([0]), array_f64_zeros([0])))",
    );
    assert!(ok, "{stderr}");
    assert_eq!(stdout, "0\n0\n0\n");
}

#[test]
fn concatenating_huge_zero_size_extents_raises_a_shape_error() {
    fails_with(
        "concat",
        "let z = array_i64_zeros([4611686018427387904, 0])\nprint(array_shape(array_concat([z, z, z, z])))",
        "R_ARRAY_SHAPE",
    );
}

// a and b hold 160 MB of the 256 MiB budget and `a * b` another 80 MB. Adding to that
// unshared temporary reuses its buffer; allocating a fourth 80 MB array would exceed the limit.
#[test]
fn elementwise_results_reuse_an_unshared_operand_buffer() {
    let (ok, stdout, stderr) = run(
        "reuse-budget",
        "let a = array_f64_full([10000000], 2.0)\nlet b = array_f64_full([10000000], 3.0)\nprint(array_get(array_add(array_mul(a, b), b), 9999999))\nprint(array_get(array_neg(array_mul(a, b)), 0))\nprint(array_get(array_neg(array_add(array_mul(a, b), b)), 0))\n",
    );
    assert!(ok, "{stderr}");
    assert_eq!(stdout, "9\n-6\n-9\n");
}
