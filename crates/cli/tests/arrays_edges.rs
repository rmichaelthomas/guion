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

/// Runs `lines` in a project whose `guion.toml` caps the allocation budget at `limit` bytes.
fn run_limited(name: &str, limit: usize, lines: &str) -> (bool, String, String) {
    let dir =
        std::env::temp_dir().join(format!("guion-arrays-limit-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("guion.toml"),
        format!(
            "[package]\nname = \"limited\"\nversion = \"0.1.0\"\n\n[runtime]\nallocation_limit_bytes = {limit}\n"
        ),
    )
    .unwrap();
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

/// A kernel that holds a temporary beside its operands and its output. `tight` holds the
/// operands and the output but not the temporary; `roomy` holds all three.
fn kernel_temporary_is_charged(
    name: &str,
    lines: &str,
    expected: &str,
    tight: usize,
    roomy: usize,
) {
    let (ok, stdout, stderr) = run_limited(&format!("{name}-tight"), tight, lines);
    assert!(!ok, "{name} fit in {tight} bytes: {stdout}");
    assert!(stderr.contains("R_ALLOCATION_LIMIT"), "{name}: {stderr}");
    let (ok, stdout, stderr) = run_limited(&format!("{name}-roomy"), roomy, lines);
    assert!(ok, "{name} failed in {roomy} bytes: {stderr}");
    assert_eq!(stdout.trim(), expected, "{name}");
}

// Each operand and output here is about 0.4 to 0.8 MB; the limits hold those and not the
// kernel's own temporary (a sort's index vector, a hash table, packing buffers).
#[test]
fn kernel_temporaries_are_charged_to_the_allocation_budget() {
    kernel_temporary_is_charged(
        "sort",
        "let a = array_i64_range(0, 100000, 1)\nprint(array_size(array_sort(a)))",
        "100000",
        2_300_000,
        3_000_000,
    );
    kernel_temporary_is_charged(
        "argsort",
        "let a = array_i64_range(0, 100000, 1)\nprint(array_size(array_argsort(a)))",
        "100000",
        2_300_000,
        3_000_000,
    );
    kernel_temporary_is_charged(
        "factorize",
        "let a = array_i64_range(0, 100000, 1)\nprint(array_size(array_factorize(a)))",
        "100000",
        2_500_000,
        8_000_000,
    );
    kernel_temporary_is_charged(
        "matmul",
        "let a = array_f64_zeros([300, 300])\nlet b = array_f64_zeros([300, 300])\nprint(array_size(array_matmul(a, b)))",
        "90000",
        2_900_000,
        6_000_000,
    );
    kernel_temporary_is_charged(
        "join-index",
        "let a = array_i64_range(0, 50000, 1)\nlet b = array_i64_range(0, 50000, 1)\nprint(array_size(array_join_index(a, b, false)))",
        "100000",
        2_500_000,
        6_000_000,
    );
}

// A loop of sorts under a budget that holds one sort at a time completes. The command line
// also refills the budget from live memory, so this guards the loop as a user sees it; the
// virtual machine's own release of each kernel's temporary is tested in the runtime crate.
#[test]
fn a_loop_of_argsorts_under_a_small_budget_completes() {
    let (ok, stdout, stderr) = run_limited(
        "argsort-loop",
        3_000_000,
        "let a = array_i64_range(0, 100000, 1)\nvar total = 0\nvar i = 0\nwhile i < 200:\n    set total = total + array_size(array_argsort(a))\n    set i = i + 1\nprint(total)",
    );
    assert!(ok, "{stderr}");
    assert_eq!(stdout.trim(), "20000000");
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
    // Exactly two elements: the start, and the start plus the step (zero).
    assert_eq!(stdout, "[-9223372036854775807, 0]\n");
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

// The matrix-by-vector form (rank-1 right operand) gives a rank-1 result.
#[test]
fn matmul_by_a_rank_one_vector_gives_a_vector() {
    let (n, k) = (300, 257);
    let source = format!(
        "let a = array_reshape(array_f64_linspace(0.0, 1.0, {}), [{n}, {k}])\nlet v = array_f64_linspace(1.0, 2.0, {k})\nlet product = array_matmul(a, v)\nprint(array_shape(product))\nprint(array_sum(product))",
        n * k
    );
    let (ok, stdout, stderr) = run("matmul-vector", &source);
    assert!(ok, "{stderr}");
    let mut lines = stdout.lines();
    assert_eq!(lines.next(), Some("[300]"));
    let mut want = 0.0f64;
    for i in 0..n {
        for p in 0..k {
            let a = (i * k + p) as f64 / (n * k - 1) as f64;
            let v = 1.0 + p as f64 / (k - 1) as f64;
            want += a * v;
        }
    }
    let got: f64 = lines.next().unwrap().trim().parse().unwrap();
    assert!((got - want).abs() <= 1e-9 * want, "{got} vs {want}");
}

// Float32 is computed in f64 and rounded; the result stays within 1e-5 of the exact product.
#[test]
fn matmul_of_float32_arrays_agrees_with_the_exact_product() {
    let (n, k, m) = (40, 50, 30);
    let source = format!(
        "let a = array_to_f32(array_reshape(array_f64_linspace(0.0, 1.0, {}), [{n}, {k}]))\nlet b = array_to_f32(array_reshape(array_f64_linspace(1.0, 2.0, {}), [{k}, {m}]))\nlet product = array_matmul(a, b)\nprint(array_shape(product))\nprint(array_get(product, 0))\nprint(array_get(product, {}))",
        n * k,
        k * m,
        n * m - 1
    );
    let (ok, stdout, stderr) = run("matmul-f32", &source);
    assert!(ok, "{stderr}");
    let operand = |start: f64, stop: f64, len: usize, index: usize| {
        let exact = start + (stop - start) * index as f64 / (len - 1) as f64;
        f64::from(exact as f32)
    };
    let element = |i: usize, j: usize| {
        (0..k)
            .map(|p| operand(0.0, 1.0, n * k, i * k + p) * operand(1.0, 2.0, k * m, p * m + j))
            .sum::<f64>()
    };
    let mut lines = stdout.lines();
    assert_eq!(lines.next(), Some("[40, 30]"));
    for (line, want) in [
        (lines.next(), element(0, 0)),
        (lines.next(), element(n - 1, m - 1)),
    ] {
        let got: f64 = line.unwrap().trim().parse().unwrap();
        assert!((got - want).abs() <= 1e-5 * want, "{got} vs {want}");
    }
}

// About ten million multiply-adds, enough to be split across threads; the answer must not
// depend on how it is split.
#[test]
fn a_matmul_large_enough_to_use_several_threads_agrees_with_the_triple_loop() {
    let (n, k, m) = (256, 200, 200);
    let source = format!(
        "let a = array_reshape(array_f64_linspace(0.0, 1.0, {}), [{n}, {k}])\nlet b = array_reshape(array_f64_linspace(1.0, 2.0, {}), [{k}, {m}])\nlet product = array_matmul(a, b)\nprint(array_shape(product))\nprint(array_sum(product))\nprint(array_get(product, {}))\nprint(array_equal(product, array_matmul(a, b)))",
        n * k,
        k * m,
        n * m - 1
    );
    let (ok, stdout, stderr) = run("matmul-threaded", &source);
    assert!(ok, "{stderr}");
    let a = |i: usize, p: usize| (i * k + p) as f64 / (n * k - 1) as f64;
    let b = |p: usize, j: usize| 1.0 + (p * m + j) as f64 / (k * m - 1) as f64;
    let mut sum = 0.0f64;
    let mut last = 0.0f64;
    for i in 0..n {
        for j in 0..m {
            let element: f64 = (0..k).map(|p| a(i, p) * b(p, j)).sum();
            sum += element;
            if i == n - 1 && j == m - 1 {
                last = element;
            }
        }
    }
    let mut lines = stdout.lines();
    assert_eq!(lines.next(), Some("[256, 200]"));
    let got: f64 = lines.next().unwrap().trim().parse().unwrap();
    assert!((got - sum).abs() <= 1e-9 * sum, "{got} vs {sum}");
    let got: f64 = lines.next().unwrap().trim().parse().unwrap();
    assert!((got - last).abs() <= 1e-9 * last, "{got} vs {last}");
    assert_eq!(lines.next(), Some("true"));
}
