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

// a, b and c hold 240 MB of the 256 MiB budget, so `set a = array_add(a, b)` can only run if the
// result is written into a's own buffer.
#[test]
fn assigning_an_elementwise_result_back_to_its_unshared_local_allocates_nothing() {
    let (ok, stdout, stderr) = run(
        "reuse-local",
        "var a = array_f64_full([10000000], 2.0)\nlet b = array_f64_full([10000000], 3.0)\nlet c = array_f64_full([10000000], 5.0)\nset a = array_add(a, b)\nset a = array_mul(a, b)\nset a = array_neg(a)\nprint(array_get(a, 0))\nprint(array_get(c, 0))\n",
    );
    assert!(ok, "{stderr}");
    assert_eq!(stdout, "-15\n5\n");
}

// Operands near the i64 limits must raise a conversion error (or return the right result)
// instead of overflowing i128 arithmetic, which panics in a debug build.
#[test]
fn i64_range_with_extreme_operands_never_panics() {
    let max = "9223372036854775807";
    let min = "-9223372036854775808";
    let huge = "170141183460469231731687303715884105727";
    let cases = [
        format!("array_i64_range({min}, {max}, 1)"),
        format!("array_i64_range({max}, {min}, -1)"),
        format!("array_i64_range({min}, {max}, {max})"),
        format!("array_i64_range({max}, {min}, -{max})"),
        format!("array_i64_range(-{huge}, {huge}, {huge})"),
        format!("array_i64_range({huge}, -{huge}, -{huge})"),
        format!("array_i64_range(0, {max}, {huge})"),
        format!("array_i64_range(0, 10, -{huge})"),
    ];
    for (index, case) in cases.iter().enumerate() {
        let (ok, stdout, stderr) = run(&format!("range-{index}"), &format!("print({case})"));
        assert!(!stderr.contains("panicked"), "{case} panicked: {stderr}");
        assert!(
            ok || stderr.contains("R_ARRAY_CONVERSION") || stderr.contains("R_ALLOCATION_LIMIT"),
            "{case}: unexpected failure {stdout} {stderr}"
        );
    }
    let (ok, stdout, stderr) = run(
        "range-last",
        &format!(
            "print(array_i64_range({}, {max}, {max}))",
            "-9223372036854775807"
        ),
    );
    assert!(ok, "{stderr}");
    assert!(stdout.contains("9223372036854775807"), "{stdout}");
}

#[test]
fn array_errors_name_the_guest_function_not_the_instruction() {
    let (ok, _, stderr) = run(
        "guest-name",
        "print(array_add(array_u8([200]), array_u8([100])))",
    );
    assert!(!ok);
    assert!(stderr.contains("array_add overflows u8"), "{stderr}");
    assert!(!stderr.contains("ArrayAdd"), "{stderr}");
    let (ok, _, stderr) = run("guest-name-sum", "print(array_min(array_i32_zeros([0])))");
    assert!(!ok);
    assert!(stderr.contains("array_min of an empty array"), "{stderr}");
}

#[test]
fn a_rejected_argument_value_raises_array_argument() {
    for (name, call) in [
        ("clip", "array_clip(array_i32([1, 2]), 5, 1)"),
        ("range", "array_i64_range(0, 10, 0)"),
    ] {
        let (ok, _, stderr) = run(&format!("argument-{name}"), &format!("print({call})"));
        assert!(!ok);
        assert!(stderr.contains("R_ARRAY_ARGUMENT"), "{call}: {stderr}");
    }
}

// Shapes that are not tile multiples, 2-D by 1-D, and an overflowing product.
#[test]
fn matmul_handles_awkward_shapes_and_nonfinite_results() {
    let linspace = |start: f64, stop: f64, len: usize, index: usize| {
        start + (stop - start) * index as f64 / (len - 1) as f64
    };
    for (n, k, m) in [(1, 2, 2), (1, 9, 7), (9, 7, 1), (7, 13, 5), (65, 70, 67)] {
        let source = format!(
            "let a = array_reshape(array_f64_linspace(0.0, 1.0, {}), [{n}, {k}])\nlet b = array_reshape(array_f64_linspace(1.0, 2.0, {}), [{k}, {m}])\nprint(array_shape(array_matmul(a, b)))\nprint(array_sum(array_matmul(a, b)))",
            n * k,
            k * m
        );
        let (ok, stdout, stderr) = run("matmul-shape", &source);
        assert!(ok, "{n}x{k}x{m}: {stderr}");
        let mut want = 0.0f64;
        for i in 0..n {
            for j in 0..m {
                for p in 0..k {
                    want +=
                        linspace(0.0, 1.0, n * k, i * k + p) * linspace(1.0, 2.0, k * m, p * m + j);
                }
            }
        }
        let got: f64 = stdout.lines().last().unwrap().trim().parse().unwrap();
        assert!(
            (got - want).abs() <= 1e-9 * want,
            "{n}x{k}x{m}: {got} vs {want}"
        );
    }
    fails_with(
        "matmul-f64-overflow",
        "let a = array_reshape(array_f64_linspace(1.0e200, 2.0e200, 4096), [64, 64])\nprint(array_matmul(a, a))",
        "R_FLOAT_NONFINITE",
    );
}
