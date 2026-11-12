use std::process::Command;

fn run(name: &str, body: &str) -> (bool, String, String) {
    let dir = std::env::temp_dir().join(format!("guion-arrays-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("main.gu"), body).unwrap();
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

fn main_body(lines: &str) -> String {
    let mut source = String::from("fn main() -> Unit:\n");
    for line in lines.lines() {
        source.push_str("    ");
        source.push_str(line);
        source.push('\n');
    }
    source
}

fn stdout_of(name: &str, lines: &str) -> String {
    let (ok, stdout, stderr) = run(name, &main_body(lines));
    assert!(ok, "{name} failed: {stderr}");
    stdout
}

fn error_of(name: &str, lines: &str) -> String {
    let (ok, stdout, stderr) = run(name, &main_body(lines));
    assert!(!ok, "{name} unexpectedly succeeded: {stdout}");
    stderr
}

#[test]
fn broadcasting_arithmetic() {
    let out = stdout_of(
        "broadcast",
        r#"let m = array_reshape(array_i32([1, 2, 3, 4, 5, 6]), [2, 3])
print(array_add(m, array_i32([10, 20, 30])))
let col = array_reshape(array_i32([1, 2]), [2, 1])
let row = array_reshape(array_i32([10, 20, 30]), [1, 3])
print(array_mul(col, row))
print(array_sub(m, array_i32_full([], 1)))
print(array_add(array_f64_full([], 1.5), array_f64_full([], 2.0)))
print(array_mul_scalar(m, 2))
print(array_sub_scalar(array_f64([1.0, 2.0]), 0.5))
print(array_add_scalar(array_u8([1]), 2))
print(array_div(array_i32([-7, 7]), array_i32([2, 2])))
print(array_div(array_f64([1.0]), array_f64([4.0])))
print(array_div_scalar(array_f32([1.0]), 4.0))
print(array_minimum(array_i64([1, 5]), array_i64([3, 2])))
print(array_maximum(array_i64([1, 5]), array_i64([3, 2])))
print(array_neg(array_i8([-127, 5])))
print(array_abs(array_f64([-2.5, 1.0])))
print(array_clip(array_i32([-5, 3, 9]), 0, 5))
print(array_add(array_f64_zeros([0]), array_f64_zeros([0])))"#,
    );
    assert_eq!(
        out,
        "[[11, 22, 33], [14, 25, 36]]\n[[10, 20, 30], [20, 40, 60]]\n[[0, 1, 2], [3, 4, 5]]\n3.5\n[[2, 4, 6], [8, 10, 12]]\n[0.5, 1.5]\n[3]\n[-4, 3]\n[0.25]\n[0.25]\n[1, 2]\n[3, 5]\n[127, -5]\n[2.5, 1]\n[0, 3, 5]\n[]\n"
    );
}

#[test]
fn arithmetic_errors() {
    assert!(error_of(
        "bad-broadcast",
        "print(array_add(array_i32_zeros([2, 3]), array_i32_zeros([2])))"
    )
    .contains("R_ARRAY_SHAPE"));
    assert!(error_of(
        "i8-overflow",
        "print(array_add(array_i8([127]), array_i8([1])))"
    )
    .contains("R_ARRAY_OVERFLOW"));
    assert!(
        error_of("u8-underflow", "print(array_sub_scalar(array_u8([0]), 1))")
            .contains("R_ARRAY_OVERFLOW")
    );
    assert!(error_of("neg-min", "print(array_neg(array_i8([-128])))").contains("R_ARRAY_OVERFLOW"));
    assert!(error_of(
        "int-div-zero",
        "print(array_div(array_i32([1]), array_i32([0])))"
    )
    .contains("R_DIV_ZERO"));
    assert!(error_of(
        "float-div-zero",
        "print(array_div_scalar(array_f64([1.0]), 0.0))"
    )
    .contains("R_DIV_ZERO"));
    assert!(error_of(
        "scalar-range",
        "print(array_add_scalar(array_u8([1]), 300))"
    )
    .contains("R_ARRAY_CONVERSION"));
    assert!(error_of(
        "float-overflow",
        "print(array_mul_scalar(array_f64([1.0e300]), 1.0e300))"
    )
    .contains("R_FLOAT_NONFINITE"));
}

#[test]
fn float_math() {
    let out = stdout_of(
        "math",
        r#"print(array_sqrt(array_f64([4.0, 2.25])))
print(array_exp(array_f64([0.0])))
print(array_log(array_f64([1.0])))
print(array_log10(array_f64([1000.0])))
print(array_log2(array_f32([8.0])))
print(array_floor(array_f64([-1.5, 1.5])))
print(array_ceil(array_f64([-1.5, 1.5])))
print(array_round(array_f64([-2.5, 2.5])))
print(array_pow(array_f64([2.0, 3.0]), 2.0))
print(array_sin(array_f64([0.0])))
print(array_tanh(array_f64([0.0])))"#,
    );
    assert_eq!(
        out,
        "[2, 1.5]\n[1]\n[0]\n[3]\n[3]\n[-2, 1]\n[-1, 2]\n[-3, 3]\n[4, 9]\n[0]\n[0]\n"
    );
    assert!(
        error_of("sqrt-domain", "print(array_sqrt(array_f64([-1.0])))").contains("R_MATH_DOMAIN")
    );
    assert!(error_of("log-domain", "print(array_log(array_f64([0.0])))").contains("R_MATH_DOMAIN"));
    assert!(
        error_of("exp-overflow", "print(array_exp(array_f64([1000.0])))")
            .contains("R_FLOAT_NONFINITE")
    );
}

#[test]
fn comparison_and_logic() {
    let out = stdout_of(
        "compare",
        r#"let a = array_i32([1, 5, 3])
let b = array_i32([2, 5, 1])
print(array_lt(a, b))
print(array_le(a, b))
print(array_gt(a, b))
print(array_ge(a, b))
print(array_eq(a, b))
print(array_ne(a, b))
print(array_gt_scalar(a, 2))
print(array_eq_scalar(array_bool([true, false]), true))
let mask = array_gt_scalar(a, 2)
print(array_not(mask))
print(array_and(mask, array_lt_scalar(a, 4)))
print(array_or(mask, array_eq_scalar(a, 1)))
print(array_any(mask))
print(array_all(mask))
print(array_count(mask))
print(array_where(mask, a, array_i32_zeros([3])))
print(array_filter(a, mask))
print(array_filter(array_reshape(a, [3, 1]), array_reshape(mask, [3, 1])))
print(array_any(array_bool_zeros([0])))
print(array_all(array_bool_zeros([0])))"#,
    );
    assert_eq!(
        out,
        "[true, false, false]\n[true, true, false]\n[false, false, true]\n[false, true, true]\n[false, true, false]\n[true, false, true]\n[false, true, true]\n[true, false]\n[true, false, false]\n[false, false, true]\n[true, true, true]\ntrue\nfalse\n2\n[0, 5, 3]\n[5, 3]\n[5, 3]\nfalse\ntrue\n"
    );
    assert!(error_of(
        "filter-shape",
        "print(array_filter(array_i32([1, 2]), array_bool([true])))"
    )
    .contains("R_ARRAY_SHAPE"));
}

#[test]
fn array_filter_edges_keep_order_and_every_kind() {
    let out = stdout_of(
        "filter-edges",
        r#"let a = array_f64([1.5, 2.5, 3.5, 4.5])
print(array_filter(a, array_bool([false, false, false, false])))
print(array_filter(a, array_bool([true, true, true, true])))
print(array_filter(a, array_bool([false, true, false, true])))
print(array_filter(a, array_bool([true, false, false, false])))
print(array_filter(array_i64([7, 8, 9]), array_bool([false, true, true])))
print(array_filter(array_bool([true, false, true]), array_bool([true, true, false])))
print(array_size(array_filter(array_f64_zeros([0]), array_bool_zeros([0]))))"#,
    );
    assert_eq!(
        out,
        "[]\n[1.5, 2.5, 3.5, 4.5]\n[2.5, 4.5]\n[1.5]\n[8, 9]\n[true, false]\n0\n"
    );
}

#[test]
fn reused_buffers_never_change_a_value_another_variable_holds() {
    let out = stdout_of(
        "reuse-alias",
        r#"let a = array_i32([1, 2, 3])
let b = a
let c = array_add(a, array_i32([10, 10, 10]))
print(a)
print(b)
print(c)
var d = array_f64([1.0, 2.0])
let e = d
set d = array_mul(d, array_f64([3.0, 3.0]))
print(d)
print(e)
let f = array_neg(array_add(e, e))
print(f)
print(e)
var g = array_i64([1, 2, 3])
set g = array_add(g, g)
set g = array_mul(g, g)
print(g)"#,
    );
    assert_eq!(
        out,
        "[1, 2, 3]\n[1, 2, 3]\n[11, 12, 13]\n[3, 6]\n[1, 2]\n[-2, -4]\n[1, 2]\n[4, 16, 36]\n"
    );
}

#[test]
fn a_nonfinite_result_in_a_reused_buffer_still_raises() {
    let err = error_of(
        "reuse-nonfinite",
        "let big = array_f64_full([2], 1.0e308)\nprint(array_add(array_add(big, big), big))",
    );
    assert!(err.contains("R_FLOAT_NONFINITE"), "{err}");
}

#[test]
fn a_nonfinite_result_assigned_back_to_its_local_still_raises() {
    let err = error_of(
        "reuse-local-nonfinite",
        "var x = array_f64([1.0, 1.0e308, 1.0e308])\nset x = array_add(x, x)\nprint(x)",
    );
    assert!(err.contains("R_FLOAT_NONFINITE"), "{err}");
    assert!(err.contains("element 1"), "{err}");
}
