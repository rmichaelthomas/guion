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
fn construct_get_set_for_every_tag() {
    let out = stdout_of(
        "every-tag",
        r#"print(array_i8([-128, 127]))
print(array_i16([-32768, 32767]))
print(array_i32([-2147483648, 2147483647]))
print(array_i64([-9223372036854775808, 9223372036854775807]))
print(array_u8([0, 255]))
print(array_u16([65535]))
print(array_u32([4294967295]))
print(array_u64([18446744073709551615]))
print(array_f32([0.1, 2.5]))
print(array_f64([0.1, 2.5]))
print(array_bool([true, false]))
let a = array_f64_full([2, 2], 1.5)
print(a)
print(array_i32_zeros([3]))
print(array_bool_zeros([2]))
print(array_get(array_u64([18446744073709551615]), 0))
print(array_get(array_i64([-9223372036854775808]), 0))
print(array_set(array_i32([1, 2, 3]), 1, 20))
print(array_get_at(a, [1, 0]))
print(array_set_at(array_i32_zeros([2, 2]), [0, 1], 7))
print(array_shape(a))
print(array_ndim(a))
print(array_size(a))
print(array_equal(a, array_f64_full([2, 2], 1.5)))
print(array_f64_full([], 2.0))"#,
    );
    assert_eq!(
        out,
        "[-128, 127]\n[-32768, 32767]\n[-2147483648, 2147483647]\n[-9223372036854775808, 9223372036854775807]\n[0, 255]\n[65535]\n[4294967295]\n[18446744073709551615]\n[0.1, 2.5]\n[0.1, 2.5]\n[true, false]\n[[1.5, 1.5], [1.5, 1.5]]\n[0, 0, 0]\n[false, false]\n18446744073709551615\n-9223372036854775808\n[1, 20, 3]\n1.5\n[[0, 7], [0, 0]]\n[2, 2]\n2\n4\ntrue\n2\n"
    );
}

#[test]
fn out_of_range_values_are_conversion_errors() {
    assert!(error_of("u8-range", "print(array_u8([256]))").contains("R_ARRAY_CONVERSION"));
    assert!(error_of("i8-range", "print(array_i8([-129]))").contains("R_ARRAY_CONVERSION"));
    assert!(error_of("u64-neg", "print(array_u64([-1]))").contains("R_ARRAY_CONVERSION"));
    assert!(
        error_of("set-range", "print(array_set(array_u8([1]), 0, 300))")
            .contains("R_ARRAY_CONVERSION")
    );
    assert!(
        error_of("full-range", "print(array_i16_full([2], 40000))").contains("R_ARRAY_CONVERSION")
    );
}

#[test]
fn indexing_errors() {
    assert!(
        error_of("get-index", "print(array_get(array_i32([1, 2, 3]), 3))")
            .contains("R_ARRAY_INDEX")
    );
    assert!(
        error_of("get-negative", "print(array_get(array_i32([1]), -1))").contains("R_ARRAY_INDEX")
    );
    assert!(
        error_of("get-at-rank", "print(array_get_at(array_i32([1]), [0, 0]))")
            .contains("R_ARRAY_INDEX")
    );
    assert!(error_of("negative-shape", "print(array_i32_zeros([-1]))").contains("R_ARRAY_SHAPE"));
}

#[test]
fn shape_operations() {
    let out = stdout_of(
        "shape-ops",
        r#"let a = array_i32([1, 2, 3, 4, 5, 6])
let m = array_reshape(a, [2, -1])
print(m)
print(array_transpose(m))
print(array_flatten(array_transpose(m)))
print(array_slice(m, 1, 2))
print(array_slice(a, 2, 4))
print(array_concat([m, m]))
print(array_stack([a, a]))
print(array_shape(array_stack([m, m])))
print(array_to_list(m))
print(array_reshape(array_i32_zeros([0]), [3, 0]))"#,
    );
    assert_eq!(
        out,
        "[[1, 2, 3], [4, 5, 6]]\n[[1, 4], [2, 5], [3, 6]]\n[1, 4, 2, 5, 3, 6]\n[[4, 5, 6]]\n[3, 4]\n[[1, 2, 3], [4, 5, 6], [1, 2, 3], [4, 5, 6]]\n[[1, 2, 3, 4, 5, 6], [1, 2, 3, 4, 5, 6]]\n[2, 2, 3]\n[1, 2, 3, 4, 5, 6]\n[[], [], []]\n"
    );
    assert!(error_of(
        "reshape-size",
        "print(array_reshape(array_i32([1, 2, 3]), [2, 2]))"
    )
    .contains("R_ARRAY_SHAPE"));
    assert!(
        error_of("slice-range", "print(array_slice(array_i32([1, 2]), 1, 3))")
            .contains("R_ARRAY_INDEX")
    );
    assert!(error_of(
        "concat-shape",
        "print(array_concat([array_i32_zeros([2, 2]), array_i32_zeros([2, 3])]))"
    )
    .contains("R_ARRAY_SHAPE"));
    assert!(error_of(
        "concat-empty",
        "let parts: List[Array[i32]] = []\nprint(array_concat(parts))"
    )
    .contains("R_ARRAY_SHAPE"));
}

#[test]
fn bytes_round_trip_for_every_tag() {
    let out = stdout_of(
        "bytes",
        r#"print(array_u64_from_bytes(array_to_bytes(array_u64([18446744073709551615, 1]))))
print(array_i64_from_bytes(array_to_bytes(array_i64([-9223372036854775808]))))
print(array_i8_from_bytes(array_to_bytes(array_i8([-1, 2]))))
print(array_i16_from_bytes(array_to_bytes(array_i16([-300]))))
print(array_i32_from_bytes(array_to_bytes(array_i32([-70000]))))
print(array_u16_from_bytes(array_to_bytes(array_u16([513]))))
print(array_u32_from_bytes(array_to_bytes(array_u32([70000]))))
print(array_f32_from_bytes(array_to_bytes(array_f32([1.25]))))
print(array_f64_from_bytes(array_to_bytes(array_f64([-0.5]))))
print(array_bool_from_bytes(array_to_bytes(array_bool([true, false]))))
print(array_to_bytes(array_u16([513])) == bytes_from_ints([1, 2]))
print(array_u8_from_bytes(bytes_from_ints([1, 2, 3])))"#,
    );
    assert_eq!(
        out,
        "[18446744073709551615, 1]\n[-9223372036854775808]\n[-1, 2]\n[-300]\n[-70000]\n[513]\n[70000]\n[1.25]\n[-0.5]\n[true, false]\ntrue\n[1, 2, 3]\n"
    );
    assert!(error_of(
        "bytes-width",
        "print(array_u16_from_bytes(bytes_from_ints([1, 2, 3])))"
    )
    .contains("R_ARRAY_SHAPE"));
    assert!(error_of(
        "bool-bytes",
        "print(array_bool_from_bytes(bytes_from_ints([2])))"
    )
    .contains("R_ARRAY_CONVERSION"));
    // 0x7ff0000000000000 is +infinity.
    assert!(error_of(
        "nonfinite-bytes",
        "print(array_f64_from_bytes(bytes_from_ints([0, 0, 0, 0, 0, 0, 240, 127])))"
    )
    .contains("R_FLOAT_NONFINITE"));
}

#[test]
fn checked_element_conversion() {
    let out = stdout_of(
        "convert",
        r#"print(array_to_u8(array_f64([255.9, 0.5])))
print(array_to_i32(array_f64([-1.9, -0.0])))
print(array_to_f64(array_i64([9007199254740993])))
print(array_to_f32(array_f64([0.1])))
print(array_to_bool(array_i32([0, 1])))
print(array_to_i8(array_bool([true, false])))
print(array_to_u64(array_i64([9223372036854775807])))
print(array_to_i64(array_u64([9223372036854775807])))"#,
    );
    assert_eq!(
        out,
        "[255, 0]\n[-1, 0]\n[9007199254740992]\n[0.1]\n[false, true]\n[1, 0]\n[9223372036854775807]\n[9223372036854775807]\n"
    );
    assert!(error_of(
        "f2i-range",
        "print(array_to_i64(array_f64([9223372036854775808.0])))"
    )
    .contains("R_ARRAY_CONVERSION"));
    assert!(
        error_of("f32-overflow", "print(array_to_f32(array_f64([1.0e300])))")
            .contains("R_ARRAY_CONVERSION")
    );
    assert!(
        error_of("to-bool", "print(array_to_bool(array_i32([2])))").contains("R_ARRAY_CONVERSION")
    );
    assert!(error_of(
        "u64-to-i64",
        "print(array_to_i64(array_u64([18446744073709551615])))"
    )
    .contains("R_ARRAY_CONVERSION"));
    assert!(
        error_of("neg-to-u8", "print(array_to_u8(array_i32([-1])))").contains("R_ARRAY_CONVERSION")
    );
}

#[test]
fn copy_on_write_preserves_aliases() {
    let source = r#"fn first(a: Array[i64]) -> Int:
    return array_get(a, 0)

fn main() -> Unit:
    var a = array_i64([1, 2, 3])
    let b = a
    set a = array_set(a, 0, 9)
    print(a)
    print(b)
    var i = 0
    while i < 3:
        set a = array_set(a, i, i * 10)
        set i = i + 1
    print(a)
    print(b)
    let parts = parallel_map([b, a], first, 2)
    print(parts)
"#;
    let (ok, stdout, stderr) = run("cow", source);
    assert!(ok, "{stderr}");
    assert_eq!(
        stdout,
        "[9, 2, 3]\n[1, 2, 3]\n[0, 10, 20]\n[1, 2, 3]\nOk([1, 0])\n"
    );
}

#[test]
fn large_arrays_are_charged_against_the_allocation_limit() {
    assert!(
        error_of("alloc", "print(array_size(array_f64_zeros([100000000])))")
            .contains("R_ALLOCATION_LIMIT")
    );
    let out = stdout_of("big-ok", "print(array_size(array_f64_zeros([2000000])))");
    assert_eq!(out, "2000000\n");
}
