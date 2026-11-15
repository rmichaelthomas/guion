//! Closures: function values that capture the variables they use.

use std::process::Command;

static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn run(source: &str) -> (bool, String, String) {
    let directory = std::env::temp_dir().join(format!(
        "guion-syntax-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("main.gu"), source).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .arg("run")
        .arg("main.gu")
        .current_dir(&directory)
        .output()
        .expect("guion starts");
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

#[test]
fn a_lambda_captures_the_variables_it_uses() {
    let (ok, out, err) = run(r##"fn main() -> Int:
    let offset = 10
    let add = fn(x: Int) -> Int: x + offset
    print(f"{add(5)}")
    return 0
"##);
    assert!(ok, "{err}");
    assert_eq!(out, "15\n");
}

#[test]
fn a_closure_can_be_returned_and_keeps_its_values() {
    let (ok, out, err) = run(r##"fn adder(n: Int) -> fn(Int) -> Int:
    return fn(x: Int) -> Int: x + n

fn main() -> Int:
    let add2 = adder(2)
    let add40 = adder(40)
    print(f"{add2(1)} {add40(1)} {add2(add40(0))}")
    return 0
"##);
    assert!(ok, "{err}");
    assert_eq!(out, "3 41 42\n");
}

#[test]
fn closures_work_with_the_list_builtins() {
    let (ok, out, err) = run(r##"fn main() -> Int:
    let limit = 3
    let scale = 10
    let numbers = [1, 2, 3, 4, 5]
    let big = filter_list(numbers, fn(x: Int) -> Bool: x > limit)
    let scaled = map_list(big, fn(x: Int) -> Int: x * scale)
    let total = fold_list(scaled, 0, fn(acc: Int, x: Int) -> Int: acc + x + limit)
    print(f"{len(big)} {total}")
    if any_list(numbers, fn(x: Int) -> Bool: x == limit):
        print("found")
    return 0
"##);
    assert!(ok, "{err}");
    assert_eq!(out, "2 96\nfound\n");
}

#[test]
fn a_nested_fn_is_a_local_function_that_captures() {
    let (ok, out, err) = run(r##"fn main() -> Int:
    let base = 100
    fn bump(x: Int) -> Int:
        let doubled = x * 2
        return doubled + base
    print(f"{bump(1)} {bump(2)}")
    return 0
"##);
    assert!(ok, "{err}");
    assert_eq!(out, "102 104\n");
}

#[test]
fn a_closure_captures_the_value_at_creation_not_a_later_change() {
    let (ok, out, err) = run(r##"fn main() -> Int:
    var n = 1
    let show = fn() -> Int: n
    set n = 99
    print(f"{show()} {n}")
    return 0
"##);
    assert!(ok, "{err}");
    assert_eq!(out, "1 99\n");
}

#[test]
fn a_closure_cannot_change_what_it_captured() {
    let (ok, _, err) = run(r##"fn main() -> Int:
    var n = 1
    fn bump() -> Int:
        set n = n + 1
        return n
    print(f"{bump()}")
    return 0
"##);
    assert!(!ok);
    assert!(!err.is_empty());
}

#[test]
fn closures_capture_closures_and_other_values() {
    let (ok, out, err) = run(r##"record Config:
    prefix: Text

fn main() -> Int:
    let config = Config("id-")
    let label = fn(n: Int) -> Text: config.prefix + int_to_text(n)
    let twice = fn(n: Int) -> Text: label(n) + label(n + 1)
    print(twice(7))
    return 0
"##);
    assert!(ok, "{err}");
    assert_eq!(out, "id-7id-8\n");
}

#[test]
fn sorting_with_a_capturing_literal_is_refused_at_check_time() {
    let (ok, _, err) = run(r##"fn main() -> Int:
    let k = 2
    let sorted = sort_by_int_key([3, 1, 2], fn(x: Int) -> Int: x * k)
    print(f"{len(sorted)}")
    return 0
"##);
    assert!(!ok);
    assert!(err.contains("E_FN_CLOSURE"), "{err}");
    assert!(!err.contains("runtime error"), "{err}");
}

#[test]
fn sorting_with_a_closure_held_in_a_variable_still_says_it_needs_a_named_function() {
    let (ok, _, err) = run(r##"fn main() -> Int:
    let k = 2
    let key = fn(x: Int) -> Int: x * k
    let sorted = sort_by_int_key([3, 1, 2], key)
    print(f"{len(sorted)}")
    return 0
"##);
    assert!(!ok);
    assert!(err.contains("R_FN_CLOSURE"), "{err}");
}

#[test]
fn sort_by_takes_a_function_literal_that_captures_nothing() {
    let (ok, out, err) = run(r##"fn main() -> Int:
    let sorted = sort_by([3, 1, 2], fn(x: Int) -> Int: 0 - x)
    print(f"{sorted[0]}")
    return 0
"##);
    assert!(ok, "{err}");
    assert_eq!(out, "3\n");
}

#[test]
fn an_unimported_std_function_names_its_module_instead_of_a_lexical_neighbour() {
    let (ok, _, err) = run(r##"fn main() -> Int:
    print(text_join(["a", "b"], ","))
    return 0
"##);
    assert!(!ok);
    assert!(err.contains("E_NAME_UNKNOWN"), "{err}");
    assert!(err.contains("import \"std:text\""), "{err}");
    assert!(!err.contains("text_find"), "{err}");
}
