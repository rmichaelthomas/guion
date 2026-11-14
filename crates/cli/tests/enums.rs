//! Enums with variants that carry values, and exhaustive `match` on them.

use std::process::Command;

fn run(source: &str) -> (bool, String, String) {
    let directory = std::env::temp_dir().join(format!(
        "guion-syntax-{}-{}",
        std::process::id(),
        source.len()
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
        files.iter().map(|(_, text)| text.len()).sum::<usize>()
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


const SHAPES: &str = r##"enum Shape:
    Circle(radius: Float64)
    Rect(width: Float64, height: Float64)
    Dot

fn area(shape: Shape) -> Float64:
    match shape:
        Circle(r):
            return 3.0 * r * r
        Rect(w, h):
            return w * h
        Dot:
            return 0.0

fn main() -> Int:
    print(f"{area(Circle(2.0))}")
    print(f"{area(Rect(2.0, 5.0))}")
    print(f"{area(Dot)}")
    return 0
"##;

#[test]
fn enum_variants_carry_values_and_match_picks_the_right_arm() {
    let (ok, out, err) = run(SHAPES);
    assert!(ok, "{err}");
    assert_eq!(out, "12\n10\n0\n");
}

#[test]
fn a_match_that_misses_a_variant_names_it() {
    let source = SHAPES.replace("        Dot:\n            return 0.0\n", "");
    let (ok, _, err) = run(&source);
    assert!(!ok);
    assert!(err.contains("E_MATCH_MISSING_VARIANT"), "{err}");
    assert!(err.contains("`Dot`"), "{err}");
}

#[test]
fn a_variant_matched_twice_is_an_error() {
    let source = SHAPES.replace(
        "        Dot:\n            return 0.0\n",
        "        Dot:\n            return 0.0\n        Dot:\n            return 1.0\n",
    );
    let (ok, _, err) = run(&source);
    assert!(!ok);
    assert!(err.contains("E_MATCH_DUPLICATE"), "{err}");
}

#[test]
fn a_variant_with_the_wrong_number_of_values_is_an_error() {
    let source = SHAPES.replace("Rect(2.0, 5.0)", "Rect(2.0)");
    let (ok, _, err) = run(&source);
    assert!(!ok);
    assert!(err.contains("E_ENUM_ARITY"), "{err}");
}

#[test]
fn enums_live_in_lists_and_work_with_text_and_list_fields() {
    let (ok, out, err) = run(
        r##"enum Token:
    Word(text: Text)
    Number(value: Int)
    Group(items: List[Int])
    End

fn describe(token: Token) -> Text:
    match token:
        Word(t):
            return t
        Number(n):
            return f"#{n}"
        Group(items):
            return f"group of {len(items)}"
        End:
            return "end"

fn main() -> Int:
    let tokens = [Word("hi"), Number(7), Group([1, 2, 3]), End]
    for token in tokens:
        print(describe(token))
    return 0
"##,
    );
    assert!(ok, "{err}");
    assert_eq!(out, "hi\n#7\ngroup of 3\nend\n");
}

#[test]
fn enums_cross_modules() {
    let (ok, out, err) = run_project(&[
        (
            "shapes.gu",
            "pub enum Mode:\n    On\n    Off(level: Int)\n\npub fn level(mode: Mode) -> Int:\n    match mode:\n        On:\n            return 100\n        Off(l):\n            return l\n",
        ),
        (
            "main.gu",
            "import \"shapes.gu\"\n\nfn main() -> Int:\n    print(f\"{level(On)} {level(Off(3))}\")\n    return 0\n",
        ),
    ]);
    assert!(ok, "{err}");
    assert_eq!(out, "100 3\n");
}

#[test]
fn a_variant_field_of_an_unsupported_type_is_an_error() {
    let (ok, _, err) = run("enum Bad:\n    Wrap(x: Option[Int])\n\nfn main() -> Int:\n    return 0\n");
    assert!(!ok);
    assert!(err.contains("E_ENUM_FIELD_TYPE"), "{err}");
}
