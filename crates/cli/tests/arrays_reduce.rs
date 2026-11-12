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
fn whole_array_reductions() {
    let out = stdout_of(
        "reduce",
        r#"print(array_sum(array_i64([9223372036854775807, 1])))
print(array_sum(array_u8([200, 200])))
print(array_sum(array_i32_zeros([0])))
print(array_prod(array_i64([4294967296, 4294967296])))
print(array_prod(array_f64_zeros([0])))
print(array_min(array_i32([3, -1, 2])))
print(array_max(array_f32([0.5, 2.5])))
print(array_argmin(array_i32([3, -1, -1])))
print(array_argmax(array_i32([1, 7, 7])))
let x = array_f64([1.0, 2.0, 3.0, 4.0])
print(array_mean(x))
print(array_var(x))
print(array_std(x))
print(array_mean(array_i32([1, 2])))
let tenth = array_f64_full([1000000], 0.1)
print(array_sum(tenth) > 99999.9999999 and array_sum(tenth) < 100000.0000001)"#,
    );
    assert_eq!(
        out,
        "9223372036854775808\n400\n0\n18446744073709551616\n1\n-1\n2.5\n1\n1\n2.5\n1.25\n1.118033988749895\n1.5\ntrue\n"
    );
    assert!(
        error_of("min-empty", "print(array_min(array_i32_zeros([0])))").contains("R_ARRAY_EMPTY")
    );
    assert!(
        error_of("mean-empty", "print(array_mean(array_f64_zeros([0])))").contains("R_ARRAY_EMPTY")
    );
    assert!(
        error_of("argmax-empty", "print(array_argmax(array_f64_zeros([0])))")
            .contains("R_ARRAY_EMPTY")
    );
}

#[test]
fn axis_reductions() {
    let out = stdout_of(
        "axis",
        r#"let m = array_reshape(array_i32([1, 2, 3, 4, 5, 6]), [2, 3])
print(array_sum_axis(m, 0))
print(array_sum_axis(m, 1))
print(array_min_axis(m, 0))
print(array_max_axis(m, 1))
print(array_mean_axis(m, 0))
print(array_sum_axis(array_i32_zeros([0, 3]), 0))"#,
    );
    assert_eq!(
        out,
        "[5, 7, 9]\n[6, 15]\n[1, 2, 3]\n[3, 6]\n[2.5, 3.5, 4.5]\n[0, 0, 0]\n"
    );
    assert!(error_of(
        "axis-range",
        "print(array_sum_axis(array_i32_zeros([2, 3]), 2))"
    )
    .contains("R_ARRAY_SHAPE"));
    assert!(error_of(
        "axis-overflow",
        "print(array_sum_axis(array_i8([100, 100]), 0))"
    )
    .contains("R_ARRAY_OVERFLOW"));
    assert!(error_of(
        "min-axis-empty",
        "print(array_min_axis(array_i32_zeros([0, 3]), 0))"
    )
    .contains("R_ARRAY_EMPTY"));
}

#[test]
fn ordering_and_accumulation() {
    let out = stdout_of(
        "order",
        r#"print(array_sort(array_f64([3.0, -1.0, 2.0])))
print(array_argsort(array_i32([2, 1, 2, 1])))
print(array_cumsum(array_i32([1, 2, 3])))
print(array_cumsum(array_reshape(array_i32([1, 2, 3, 4]), [2, 2])))
print(array_sort(array_u8_zeros([0])))"#,
    );
    assert_eq!(
        out,
        "[-1, 2, 3]\n[1, 3, 0, 2]\n[1, 3, 6]\n[1, 3, 6, 10]\n[]\n"
    );
    assert!(
        error_of("sort-rank", "print(array_sort(array_i32_zeros([2, 2])))")
            .contains("R_ARRAY_SHAPE")
    );
    assert!(error_of(
        "cumsum-overflow",
        "print(array_cumsum(array_u8([200, 100])))"
    )
    .contains("R_ARRAY_OVERFLOW"));
}

#[test]
fn linear_algebra() {
    let out = stdout_of(
        "linalg",
        r#"print(array_dot(array_i64([4294967296, 1]), array_i64([4294967296, 2])))
print(array_dot(array_f64([1.0, 2.0]), array_f64([3.0, 4.0])))
let a = array_reshape(array_i32([1, 2, 3, 4, 5, 6]), [2, 3])
let b = array_reshape(array_i32([7, 8, 9, 10, 11, 12]), [3, 2])
print(array_matmul(a, b))
print(array_matmul(a, array_i32([1, 0, 1])))
print(array_matmul(array_f64_zeros([2, 0]), array_f64_zeros([0, 2])))"#,
    );
    assert_eq!(
        out,
        "18446744073709551618\n11\n[[58, 64], [139, 154]]\n[4, 10]\n[[0, 0], [0, 0]]\n"
    );
    assert!(error_of(
        "dot-len",
        "print(array_dot(array_i32([1]), array_i32([1, 2])))"
    )
    .contains("R_ARRAY_SHAPE"));
    assert!(error_of(
        "matmul-shape",
        "print(array_matmul(array_i32_zeros([2, 3]), array_i32_zeros([2, 3])))"
    )
    .contains("R_ARRAY_SHAPE"));
    assert!(error_of(
        "matmul-overflow",
        "print(array_matmul(array_i8_full([1, 2], 100), array_i8_full([2, 1], 100)))"
    )
    .contains("R_ARRAY_OVERFLOW"));
}

#[test]
fn ranges_and_random() {
    let source = r#"import "std:random"
fn main() -> Unit:
    print(array_i64_range(0, 10, 3))
    print(array_i64_range(5, 0, -2))
    print(array_i64_range(0, 0, 1))
    print(array_f64_linspace(0.0, 1.0, 5))
    print(array_f64_linspace(2.0, 2.0, 1))
    let r = array_f64_random(42, [3])
    let first = random_next_float(random_new(42))
    let second = random_next_float(first.rng)
    let third = random_next_float(second.rng)
    print(array_get(r, 0) == first.value and array_get(r, 1) == second.value and array_get(r, 2) == third.value)
    print(array_shape(array_f64_random(1, [2, 3])))
    let n = array_f64_normal(7, [20000])
    let mean = array_mean(n)
    let std = array_std(n)
    print(mean > -0.05 and mean < 0.05 and std > 0.95 and std < 1.05)
    print(array_equal(array_f64_normal(7, [5]), array_f64_normal(7, [5])))
    # Cross-checked against an independent Python SplitMix64 + Box-Muller.
    print(array_get(array_f64_normal(1, [3]), 2))
"#;
    let (ok, stdout, stderr) = run("random", source);
    assert!(ok, "{stderr}");
    assert_eq!(
        stdout,
        "[0, 3, 6, 9]\n[5, 3, 1]\n[]\n[0, 0.25, 0.5, 0.75, 1]\n[2]\ntrue\n[2, 3]\ntrue\ntrue\n-2.5000674933698677\n"
    );
    assert!(error_of("range-step", "print(array_i64_range(0, 10, 0))").contains("R_ARRAY_ARGUMENT"));
    assert!(
        error_of("linspace-count", "print(array_f64_linspace(0.0, 1.0, -1))")
            .contains("R_ARRAY_SHAPE")
    );
}
