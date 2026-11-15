//! Generic functions: type parameters bound from the arguments or written at the call.

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

fn run_project(files: &[(&str, &str)]) -> (bool, String, String) {
    let directory = std::env::temp_dir().join(format!(
        "guion-syntax-project-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&directory).unwrap();
    for (name, text) in files {
        std::fs::write(directory.join(name), text).unwrap();
    }
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
fn a_generic_function_is_compiled_for_each_type_it_is_used_with() {
    let (ok, out, err) = run(r##"fn first[T](items: List[T]) -> T:
    return items[0]

fn main() -> Int:
    print(f"{first([4, 5, 6])}")
    print(first(["a", "b"]))
    print(f"{first([2.5, 1.0])}")
    return 0
"##);
    assert!(ok, "{err}");
    assert_eq!(out, "4\na\n2.5\n");
}

#[test]
fn type_arguments_can_be_written_when_they_cannot_be_inferred() {
    let (ok, out, err) = run(r##"fn empty[T]() -> List[T]:
    let items: List[T] = []
    return items

fn main() -> Int:
    let numbers = empty[Int]()
    print(f"{len(numbers)}")
    return 0
"##);
    assert!(ok, "{err}");
    assert_eq!(out, "0\n");
}

#[test]
fn a_generic_that_cannot_be_inferred_says_how_to_write_it() {
    let (ok, _, err) = run(r##"fn empty[T]() -> List[T]:
    let items: List[T] = []
    return items

fn main() -> Int:
    let numbers = empty()
    return 0
"##);
    assert!(!ok);
    assert!(err.contains("E_GENERIC_INFER"), "{err}");
    assert!(err.contains("empty[Int]"), "{err}");
}

#[test]
fn generics_call_generics_and_take_two_type_parameters() {
    let (ok, out, err) = run(r##"record Pair:
    left: Int
    right: Text

fn first[T](items: List[T]) -> T:
    return items[0]

fn pick[A, B](a: List[A], b: List[B]) -> B:
    let ignored = first(a)
    return first(b)

fn main() -> Int:
    print(pick([1, 2], ["x", "y"]))
    let pairs = [Pair(1, "one"), Pair(2, "two")]
    print(first(pairs).right)
    return 0
"##);
    assert!(ok, "{err}");
    assert_eq!(out, "x\none\n");
}

#[test]
fn mismatched_argument_types_for_one_type_parameter_are_an_error() {
    let (ok, _, err) = run(r##"fn same[T](a: T, b: T) -> T:
    return a

fn main() -> Int:
    let x = same(1, "two")
    return 0
"##);
    assert!(!ok);
    assert!(err.contains("E_TYPE_ARGUMENT"), "{err}");
}

#[test]
fn generics_cross_modules() {
    let (ok, out, err) = run_project(&[
        (
            "util.gu",
            "pub fn last[T](items: List[T]) -> T:\n    return items[len(items) - 1]\n",
        ),
        (
            "main.gu",
            "import \"util.gu\"\n\nfn main() -> Int:\n    print(f\"{last([1, 2, 3])}\")\n    return 0\n",
        ),
    ]);
    assert!(ok, "{err}");
    assert_eq!(out, "3\n");
}
