//! `match` arms that are `Int`, `Text` or `Bool` literals, at the top and inside `Some`, `Ok`
//! and `Err`, with `_` or a name as the catch-all.

use std::path::PathBuf;
use std::process::{Command, Output};

struct Scratch(PathBuf);
impl Scratch {
    fn new(name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("guion-matchlit-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn guion(scratch: &Scratch, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(args)
        .current_dir(&scratch.0)
        .env("GUION_NO_PROMPT", "1")
        .output()
        .expect("guion starts")
}

fn run(name: &str, source: &str) -> Output {
    let scratch = Scratch::new(name);
    std::fs::write(scratch.0.join("main.gu"), source).unwrap();
    guion(&scratch, &["run", "main.gu"])
}

fn stdout_of(name: &str, source: &str) -> String {
    let output = run(name, source);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// The diagnostic text of a program that must not compile.
fn rejected(name: &str, source: &str) -> String {
    let output = run(name, source);
    assert!(!output.status.success(), "program compiled");
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn int_literals_match_in_any_base_with_separators_and_negatives() {
    let out = stdout_of(
        "int",
        "fn name(n: Int) -> Text:
    match n:
        0:
            return \"zero\"
        0xFF:
            return \"ff\"
        0o17:
            return \"fifteen\"
        0b101:
            return \"five\"
        1_000:
            return \"thousand\"
        -7:
            return \"minus seven\"
        -0x10:
            return \"minus sixteen\"
        _:
            return \"other\"

fn main() -> Int:
    print(name(0))
    print(name(255))
    print(name(15))
    print(name(5))
    print(name(1000))
    print(name(-7))
    print(name(-16))
    print(name(8))
    return 0
",
    );
    assert_eq!(
        out,
        "zero\nff\nfifteen\nfive\nthousand\nminus seven\nminus sixteen\nother\n"
    );
}

#[test]
fn a_name_arm_binds_the_value_and_text_literals_match() {
    let out = stdout_of(
        "text",
        "fn greet(word: Text) -> Text:
    match word:
        \"hello\":
            return \"hi\"
        \"\":
            return \"nothing\"
        other:
            return \"got \" + other

fn size(n: Int) -> Int:
    match n:
        1:
            return 10
        rest:
            return rest * 2

fn main() -> Int:
    print(greet(\"hello\"))
    print(greet(\"\"))
    print(greet(\"yo\"))
    print(size(1))
    print(size(4))
    return 0
",
    );
    assert_eq!(out, "hi\nnothing\ngot yo\n10\n8\n");
}

#[test]
fn bool_literals_are_exhaustive_with_both_and_take_a_catch_all_otherwise() {
    let out = stdout_of(
        "bool",
        "fn label(flag: Bool) -> Text:
    match flag:
        true:
            return \"yes\"
        false:
            return \"no\"

fn only_true(flag: Bool) -> Int:
    match flag:
        true:
            return 1
        _:
            return 0

fn main() -> Int:
    print(label(true))
    print(label(1 > 2))
    print(only_true(true))
    print(only_true(false))
    return 0
",
    );
    assert_eq!(out, "yes\nno\n1\n0\n");
}

#[test]
fn literals_nest_inside_some_ok_and_err() {
    let out = stdout_of(
        "nested",
        "fn first(values: List[Int], at: Int) -> Text:
    match list_get(values, at):
        Some(0):
            return \"zero\"
        Some(-1):
            return \"minus one\"
        Some(n):
            return \"n=\" + int_to_text(n)
        None:
            return \"none\"

fn parsed(text: Text) -> Text:
    match text_parse_int(text):
        Ok(42):
            return \"answer\"
        Ok(_):
            return \"number\"
        Err(_):
            return \"not a number\"

fn word(w: Option[Text]) -> Int:
    match w:
        Some(\"a\"):
            return 1
        Some(_):
            return 2
        None:
            return 3

fn flag(f: Option[Bool]) -> Int:
    match f:
        Some(true):
            return 1
        Some(false):
            return 0
        None:
            return -1

fn failed(r: Result[Int, Text]) -> Int:
    match r:
        Ok(_):
            return 0
        Err(\"busy\"):
            return 1
        Err(_):
            return 2

fn main() -> Int:
    print(first([0, -1, 5], 0))
    print(first([0, -1, 5], 1))
    print(first([0, -1, 5], 2))
    print(first([0], 3))
    print(parsed(\"42\"))
    print(parsed(\"7\"))
    print(parsed(\"x\"))
    print(word(Some(\"a\")))
    print(word(Some(\"b\")))
    print(word(None))
    print(flag(Some(true)))
    print(flag(Some(false)))
    print(flag(None))
    print(failed(Err(\"busy\")))
    print(failed(Err(\"other\")))
    print(failed(Ok(3)))
    return 0
",
    );
    assert_eq!(
        out,
        "zero\nminus one\nn=5\nnone\nanswer\nnumber\nnot a number\n1\n2\n3\n1\n0\n-1\n1\n2\n0\n"
    );
}

#[test]
fn an_int_or_text_match_without_a_catch_all_names_the_underscore_arm() {
    for (name, source) in [
        (
            "int-missing",
            "fn f(n: Int) -> Int:\n    match n:\n        1:\n            return 1\n        2:\n            return 2\n\nfn main() -> Int:\n    return f(1)\n",
        ),
        (
            "text-missing",
            "fn f(t: Text) -> Int:\n    match t:\n        \"a\":\n            return 1\n\nfn main() -> Int:\n    return f(\"a\")\n",
        ),
        (
            "nested-missing",
            "fn f(v: Option[Int]) -> Int:\n    match v:\n        Some(1):\n            return 1\n        None:\n            return 0\n\nfn main() -> Int:\n    return f(None)\n",
        ),
        (
            "bool-one-side",
            "fn f(b: Bool) -> Int:\n    match b:\n        true:\n            return 1\n\nfn main() -> Int:\n    return f(true)\n",
        ),
    ] {
        let error = rejected(name, source);
        assert!(error.contains("E_MATCH_NONEXHAUSTIVE"), "{name}: {error}");
        assert!(error.contains("`_`"), "{name}: {error}");
    }
}

#[test]
fn repeated_literals_and_arms_after_the_catch_all_are_duplicates() {
    for (name, source) in [
        (
            "dup-int",
            "fn f(n: Int) -> Int:\n    match n:\n        1:\n            return 1\n        0x1:\n            return 2\n        _:\n            return 3\n\nfn main() -> Int:\n    return f(1)\n",
        ),
        (
            "dup-text",
            "fn f(t: Text) -> Int:\n    match t:\n        \"a\":\n            return 1\n        \"a\":\n            return 2\n        _:\n            return 3\n\nfn main() -> Int:\n    return f(\"a\")\n",
        ),
        (
            "dup-bool",
            "fn f(b: Bool) -> Int:\n    match b:\n        true:\n            return 1\n        true:\n            return 2\n        false:\n            return 3\n\nfn main() -> Int:\n    return f(true)\n",
        ),
        (
            "after-catch-all",
            "fn f(n: Int) -> Int:\n    match n:\n        _:\n            return 3\n        1:\n            return 1\n\nfn main() -> Int:\n    return f(1)\n",
        ),
        (
            "dup-nested",
            "fn f(v: Option[Int]) -> Int:\n    match v:\n        Some(1):\n            return 1\n        Some(1):\n            return 2\n        Some(_):\n            return 3\n        None:\n            return 4\n\nfn main() -> Int:\n    return f(None)\n",
        ),
    ] {
        let error = rejected(name, source);
        assert!(error.contains("E_MATCH_DUPLICATE"), "{name}: {error}");
    }
}

#[test]
fn a_literal_of_the_wrong_type_is_a_match_type_error() {
    for (name, source) in [
        (
            "text-on-int",
            "fn f(n: Int) -> Int:\n    match n:\n        \"a\":\n            return 1\n        _:\n            return 0\n\nfn main() -> Int:\n    return f(1)\n",
        ),
        (
            "int-on-text",
            "fn f(t: Text) -> Int:\n    match t:\n        1:\n            return 1\n        _:\n            return 0\n\nfn main() -> Int:\n    return f(\"a\")\n",
        ),
        (
            "bool-on-int",
            "fn f(n: Int) -> Int:\n    match n:\n        true:\n            return 1\n        false:\n            return 0\n\nfn main() -> Int:\n    return f(1)\n",
        ),
        (
            "mixed-literals",
            "fn f(n: Int) -> Int:\n    match n:\n        1:\n            return 1\n        \"a\":\n            return 2\n        _:\n            return 0\n\nfn main() -> Int:\n    return f(1)\n",
        ),
        (
            "nested-wrong",
            "fn f(v: Option[Int]) -> Int:\n    match v:\n        Some(\"a\"):\n            return 1\n        Some(_):\n            return 2\n        None:\n            return 0\n\nfn main() -> Int:\n    return f(None)\n",
        ),
        (
            "float-scrutinee",
            "fn f(x: Float64) -> Int:\n    match x:\n        1:\n            return 1\n        _:\n            return 0\n\nfn main() -> Int:\n    return f(1.0)\n",
        ),
    ] {
        let error = rejected(name, source);
        assert!(error.contains("E_MATCH_TYPE"), "{name}: {error}");
    }
}

#[test]
fn a_literal_cannot_share_a_position_with_a_constructor_and_floats_are_refused() {
    let error = rejected(
        "constructor-and-literal",
        "fn f(v: Option[Int]) -> Int:\n    match v:\n        1:\n            return 1\n        None:\n            return 0\n\nfn main() -> Int:\n    return f(None)\n",
    );
    assert!(error.contains("E_MATCH_PATTERN"), "{error}");
    let error = rejected(
        "float-pattern",
        "fn f(x: Float64) -> Int:\n    match x:\n        1.5:\n            return 1\n        _:\n            return 0\n\nfn main() -> Int:\n    return f(1.0)\n",
    );
    assert!(error.contains("E_MATCH_PATTERN"), "{error}");
}

#[test]
fn a_literal_match_is_machine_code_when_the_same_if_chain_is() {
    let scratch = Scratch::new("speed");
    std::fs::write(
        scratch.0.join("main.gu"),
        "fn by_match(n: Int) -> Int:
    match n:
        0:
            return 10
        1:
            return 20
        -3:
            return 30
        _:
            return n

fn by_if(n: Int) -> Int:
    if n == 0:
        return 10
    if n == 1:
        return 20
    if n == -3:
        return 30
    return n

fn main() -> Int:
    print(by_match(1) + by_if(1))
    return 0
",
    )
    .unwrap();
    let output = guion(&scratch, &["inspect", "--speed", "--json", "main.gu"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let row = |name: &str| {
        report["functions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["function"] == name)
            .unwrap_or_else(|| panic!("{name} is listed"))
            .clone()
    };
    assert_eq!(row("by_if")["runs_as"], "machine code");
    assert_eq!(row("by_match")["runs_as"], row("by_if")["runs_as"]);
}

#[test]
fn a_formatted_literal_match_is_stable() {
    let scratch = Scratch::new("fmt");
    let source = "fn f(n: Int) -> Int:\n    match n:\n        -1:\n            return 1\n        0x10:\n            return 2\n        _:\n            return 3\n\nfn main() -> Int:\n    return f(1)\n";
    std::fs::write(scratch.0.join("main.gu"), source).unwrap();
    let output = guion(&scratch, &["fmt", "--check", "main.gu"]);
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
