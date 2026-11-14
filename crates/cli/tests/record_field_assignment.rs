//! `set record.field = value`: assigning a field of a record variable, with the aliasing and error
//! behaviour the language promises, and the in-place append a list in a record field needs.

use std::process::Command;

fn run(source: &str) -> (bool, String, String) {
    let directory = std::env::temp_dir().join(format!(
        "guion-field-set-{}-{}",
        std::process::id(),
        source.len()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let file = directory.join("main.gu");
    std::fs::write(&file, source).unwrap();
    std::fs::write(
        directory.join("guion.toml"),
        "[package]\nname = \"field-set\"\nversion = \"0.1.0\"\n[runtime]\ninstruction_limit = 2000000000\nallocation_limit_bytes = 8589934592\n",
    )
    .unwrap();
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

const BAG: &str = "record Bag:\n    items: List[Int]\n    count: Int\n\n";

#[test]
fn a_field_can_be_assigned_and_read_back() {
    let (ok, out, err) = run(&format!(
        "{BAG}fn main() -> Unit:\n    var b = Bag([], 0)\n    set b.count = 5\n    set b.items = [1, 2]\n    print(b.count)\n    print(len(b.items))\n    print(b)\n"
    ));
    assert!(ok, "{err}");
    assert_eq!(out, "5\n2\nBag([1, 2], 5)\n");
}

#[test]
fn a_copy_taken_before_an_assignment_keeps_the_old_value() {
    let (ok, out, err) = run(&format!(
        "{BAG}fn main() -> Unit:\n    var b = Bag([1], 1)\n    let before = b\n    set b.items = list_push(b.items, 2)\n    set b.count = 2\n    print(before)\n    print(b)\n"
    ));
    assert!(ok, "{err}");
    assert_eq!(out, "Bag([1], 1)\nBag([1, 2], 2)\n");
}

#[test]
fn a_record_passed_to_a_function_is_the_current_value() {
    let (ok, out, err) = run(&format!(
        "{BAG}fn total(bag: Bag) -> Int:\n    return len(bag.items) * 10 + bag.count\n\nfn main() -> Unit:\n    var b = Bag([], 0)\n    set b.items = list_push(b.items, 7)\n    set b.count = 3\n    print(total(b))\n    set b = Bag([1, 2, 3], 4)\n    print(total(b))\n"
    ));
    assert!(ok, "{err}");
    assert_eq!(out, "13\n34\n");
}

#[test]
fn nested_fields_assign_through_the_path() {
    let (ok, out, err) = run(
        "record Inner:\n    n: Int\n    tags: List[Text]\n\nrecord Outer:\n    name: Text\n    inner: Inner\n\nfn main() -> Unit:\n    var o = Outer(\"x\", Inner(1, []))\n    set o.inner.n = 10\n    set o.inner.tags = list_push(o.inner.tags, \"a\")\n    set o.name = \"y\"\n    print(o)\n    set o = Outer(\"z\", Inner(5, [\"q\"]))\n    print(o.inner.tags)\n",
    );
    assert!(ok, "{err}");
    assert_eq!(out, "Outer(y, Inner(10, [a]))\n[q]\n");
}

#[test]
fn the_constructor_spelling_of_an_append_also_works() {
    let (ok, out, err) = run(&format!(
        "{BAG}fn main() -> Unit:\n    var b = Bag([], 0)\n    var i = 0\n    while i < 3:\n        set b = Bag(list_push(b.items, i), b.count + 1)\n        set i = i + 1\n    print(b)\n"
    ));
    assert!(ok, "{err}");
    assert_eq!(out, "Bag([0, 1, 2], 3)\n");
}

#[test]
fn a_constructor_argument_that_reads_the_appended_field_sees_the_old_list() {
    let (ok, out, err) = run(&format!(
        "{BAG}fn main() -> Unit:\n    var b = Bag([1, 2], 0)\n    set b = Bag(list_push(b.items, 3), len(b.items))\n    print(b)\n"
    ));
    assert!(ok, "{err}");
    assert_eq!(out, "Bag([1, 2, 3], 2)\n");
}

#[test]
fn assigning_a_field_of_an_immutable_record_is_an_error() {
    let (ok, _, err) = run(&format!(
        "{BAG}fn main() -> Unit:\n    let b = Bag([], 0)\n    set b.count = 1\n"
    ));
    assert!(!ok);
    assert!(err.contains("E_ASSIGN_IMMUTABLE"), "{err}");
}

#[test]
fn assigning_a_field_that_does_not_exist_is_an_error() {
    let (ok, _, err) = run(&format!(
        "{BAG}fn main() -> Unit:\n    var b = Bag([], 0)\n    set b.missing = 1\n"
    ));
    assert!(!ok);
    assert!(err.contains("E_FIELD_UNKNOWN"), "{err}");
}

#[test]
fn a_field_value_of_the_wrong_type_is_an_error() {
    let (ok, _, err) = run(&format!(
        "{BAG}fn main() -> Unit:\n    var b = Bag([], 0)\n    set b.count = \"one\"\n    set b.count = 2\n"
    ));
    assert!(!ok);
    assert!(err.contains("E_TYPE"), "{err}");
}

#[test]
fn two_hundred_thousand_appends_to_a_record_field_run_in_linear_time() {
    let started = std::time::Instant::now();
    let (ok, out, err) = run(&format!(
        "{BAG}fn main() -> Unit:\n    var b = Bag([], 0)\n    var i = 0\n    while i < 200000:\n        set b.items = list_push(b.items, i)\n        set b.count = b.count + 1\n        set i = i + 1\n    print(len(b.items))\n    print(b.count)\n"
    ));
    assert!(ok, "{err}");
    assert_eq!(out, "200000\n200000\n");
    assert!(
        started.elapsed().as_secs() < 20,
        "appends took {:?}",
        started.elapsed()
    );
}
