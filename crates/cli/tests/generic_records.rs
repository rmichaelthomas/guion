//! Generic records (`record Pair[A, B]:`): each set of type arguments becomes an ordinary record.

use std::process::Command;

static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn project(files: &[(&str, &str)]) -> std::path::PathBuf {
    let directory = std::env::temp_dir().join(format!(
        "guion-genrec-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&directory).unwrap();
    for (name, source) in files {
        std::fs::write(directory.join(name), source).unwrap();
    }
    directory
}

fn run_dir(directory: &std::path::Path, command: &str) -> (bool, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .arg(command)
        .arg("main.gu")
        .current_dir(directory)
        .output()
        .expect("guion starts");
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn run(source: &str) -> (bool, String, String) {
    run_dir(&project(&[("main.gu", source)]), "run")
}

#[test]
fn arguments_are_inferred_or_written_and_fields_read_back() {
    let (ok, out, err) = run(r##"record Pair[A, B]:
    first: A
    second: B

record Box[T]:
    items: List[T]

fn describe(p: Pair[Int, Text]) -> Text:
    return f"{p.first}:{p.second}"

fn main() -> Int:
    let p = Pair(1, "one")
    print(describe(p))
    let b = Box[Int]([1, 2, 3])
    print(f"{len(b.items)}")
    let q: Pair[Text, Int] = Pair("a", 2)
    print(f"{q.first}{q.second}")
    let nested = Pair(b, Pair(2, "x"))
    print(f"{nested.second.second}")
    return 0
"##);
    assert!(ok, "{err}");
    assert_eq!(out, "1:one\n3\na2\nx\n");
}

#[test]
fn generic_functions_take_and_return_generic_records() {
    let (ok, out, err) = run(r##"record Pair[A, B]:
    first: A
    second: B

fn swap[A, B](p: Pair[A, B]) -> Pair[B, A]:
    return Pair(p.second, p.first)

fn first_of[T](p: Pair[T, Int]) -> T:
    return p.first

fn main() -> Int:
    let s = swap(Pair(1, "one"))
    print(f"{s.first} {s.second}")
    let k = first_of(Pair[Text, Int]("k", 9))
    print(k)
    return 0
"##);
    assert!(ok, "{err}");
    assert_eq!(out, "one 1\nk\n");
}

#[test]
fn two_instances_are_distinct_types() {
    let (ok, _, err) = run(r##"record Box[T]:
    value: T

fn takes(b: Box[Int]) -> Int:
    return b.value

fn main() -> Int:
    return takes(Box("text"))
"##);
    assert!(!ok);
    assert!(err.contains("E_TYPE_ARGUMENT"), "{err}");
}

#[test]
fn a_generic_record_needs_its_type_arguments() {
    let (ok, _, err) = run(r##"record Box[T]:
    value: T

fn take(b: Box) -> Int:
    return 0

fn main() -> Int:
    return 0
"##);
    assert!(!ok);
    assert!(err.contains("E_GENERIC_ARITY"), "{err}");

    let (ok, _, err) = run(r##"record Box[T]:
    value: T

fn take(b: Box[Int, Int]) -> Int:
    return 0

fn main() -> Int:
    return 0
"##);
    assert!(!ok);
    assert!(err.contains("E_GENERIC_ARITY"), "{err}");
}

#[test]
fn an_argument_the_fields_do_not_fix_must_be_written() {
    let (ok, _, err) = run(r##"record Tag[T]:
    n: Int

fn main() -> Int:
    let t = Tag(1)
    return 0
"##);
    assert!(!ok);
    assert!(err.contains("E_GENERIC_INFER"), "{err}");
}

#[test]
fn a_generic_record_can_be_declared_in_another_module() {
    let directory = project(&[
        (
            "shapes.gu",
            "pub record Pair[A, B]:\n    first: A\n    second: B\n\npub fn make(a: Int, b: Text) -> Pair[Int, Text]:\n    return Pair(a, b)\n",
        ),
        (
            "main.gu",
            "import \"shapes.gu\"\n\nfn main() -> Int:\n    let p = make(7, \"seven\")\n    let q = Pair(1, 2)\n    print(f\"{p.first}{p.second}{q.second}\")\n    return 0\n",
        ),
    ]);
    let (ok, out, err) = run_dir(&directory, "run");
    assert!(ok, "{err}");
    assert_eq!(out, "7seven2\n");
}

#[test]
fn a_generic_record_name_cannot_be_declared_twice() {
    let (ok, _, err) = run(r##"record Box[T]:
    value: T

record Box[T]:
    other: T

fn main() -> Int:
    return 0
"##);
    assert!(!ok);
    assert!(err.contains("E_NAME_DUPLICATE"), "{err}");
}
