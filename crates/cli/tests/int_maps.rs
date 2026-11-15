//! `Map[Int, T]` (artifact format 75): keys are `Int`, iteration is numeric and ascending.

use std::process::Command;

static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn run(source: &str) -> (bool, String, String) {
    let directory = std::env::temp_dir().join(format!(
        "guion-intmap-{}-{}",
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
fn int_keys_get_set_has_and_len() {
    let (ok, out, err) = run(r##"fn main() -> Int:
    let m: Map[Int, Text] = {}
    let a = map_set(map_set(m, 10, "ten"), -3, "minus three")
    print(f"{len(a)} {a[10]} {a[-3]}")
    if is_some(map_get(a, 10)):
        print("some")
    if not map_has(a, 11):
        print("none")
    return 0
"##);
    assert!(ok, "{err}");
    assert_eq!(out, "2 ten minus three\nsome\nnone\n");
}

#[test]
fn iteration_is_numeric_not_lexical() {
    let (ok, out, err) = run(r##"fn main() -> Int:
    let m = {10: "j", 9: "i", -3: "c", 200: "x"}
    for k in m:
        print(f"{k}{m[k]}")
    return 0
"##);
    assert!(ok, "{err}");
    assert_eq!(out, "-3c\n9i\n10j\n200x\n");
}

#[test]
fn an_int_keyed_map_is_not_a_text_keyed_map() {
    let (ok, _, err) = run(r##"fn main() -> Int:
    let m: Map[Int, Int] = {}
    let n = map_set(m, "a", 1)
    return 0
"##);
    assert!(!ok);
    assert!(err.contains("E_TYPE_ARGUMENT"), "{err}");
    let (ok, _, err) = run(r##"fn takes(m: Map[Text, Int]) -> Int:
    return len(m)

fn main() -> Int:
    let m: Map[Int, Int] = {}
    return takes(m)
"##);
    assert!(!ok, "an Int map must not pass for a Text map");
    assert!(!err.is_empty());
}

#[test]
fn a_map_key_must_be_text_or_int() {
    let (ok, _, err) = run("fn main() -> Int:\n    let m: Map[Bool, Int] = {}\n    return 0\n");
    assert!(!ok);
    assert!(err.contains("E_TYPE_MAP_KEY"), "{err}");
}

#[test]
fn duplicate_int_literal_keys_are_refused() {
    let (ok, _, err) = run("fn main() -> Int:\n    let m = {1: 1, 1: 2}\n    return 0\n");
    assert!(!ok);
    assert!(err.contains("E_MAP_DUPLICATE_KEY"), "{err}");
}

#[test]
fn an_int_map_travels_through_functions_and_records() {
    let (ok, out, err) = run(r##"record Index:
    entries: Map[Int, Int]

fn add(index: Index, k: Int, v: Int) -> Index:
    return Index(map_set(index.entries, k, v))

fn main() -> Int:
    let empty: Map[Int, Int] = {}
    let index = add(add(Index(empty), 2, 20), 1, 10)
    var total = 0
    for k in index.entries:
        set total = total * 100 + index.entries[k]
    print(f"{total}")
    return 0
"##);
    assert!(ok, "{err}");
    assert_eq!(out, "1020\n");
}
