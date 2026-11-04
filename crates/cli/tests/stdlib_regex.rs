use std::process::Command;

fn run_source(name: &str, source: &str) -> std::process::Output {
    let dir = std::env::temp_dir().join(format!("guion-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("main.gu"), source).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", "main.gu"])
        .current_dir(&dir)
        .output()
        .expect("guion starts");
    let _ = std::fs::remove_dir_all(&dir);
    output
}

// Expected values below were produced with CPython's re module for the syntax both engines share.
#[test]
fn regex_matches_finds_replaces_and_splits_like_cpython() {
    let source = r##"import "std:regex"
fn main() -> Int:
    assert(unwrap(regex_is_match("^[a-z]+[0-9]{2}$", "abc42")))
    assert(not unwrap(regex_is_match("^[a-z]+[0-9]{2}$", "abc4")))
    assert(unwrap(regex_find_all("[0-9]+", "a1 b22 c333")) == ["1", "22", "333"])
    assert(len(unwrap(regex_find_all("x", "abc"))) == 0)
    let groups = unwrap(unwrap(regex_find("(\\w+)@(\\w+)\\.com", "mail bob@example.com now")))
    assert(groups == ["bob@example.com", "bob", "example"])
    assert(not is_some(unwrap(regex_find("zzz", "abc"))))
    let optional = unwrap(unwrap(regex_find("a(b)?c", "ac")))
    assert(optional == ["ac", ""])
    assert(unwrap(regex_replace("[0-9]+", "a1 b22", "#")) == "a# b#")
    assert(unwrap(regex_replace_first("[0-9]+", "a1 b22", "#")) == "a# b22")
    assert(unwrap(regex_replace("(\\w+) (\\w+)", "hello world", "$2 $1")) == "world hello")
    assert(unwrap(regex_split(",\\s*", "a, b,c,   d")) == ["a", "b", "c", "d"])
    assert(unwrap(regex_find_all("é+", "caféé é")) == ["éé", "é"])
    assert(unwrap(regex_is_match(regex_escape("a.b*c"), "a.b*c")))
    assert(not unwrap(regex_is_match(regex_escape("a.b"), "axb")))
    return 0
"##;
    let output = run_source("regex-basic", source);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn invalid_patterns_and_unsupported_features_return_errors_and_matching_stays_linear() {
    let source = r##"import "std:regex"
import "std:types"
fn main() -> Int:
    assert(not is_ok(regex_is_match("(", "x")))
    assert(not is_ok(regex_find_all("[a-", "x")))
    assert(not is_ok(regex_is_match("(a)\\1", "aa")))
    assert(not is_ok(regex_is_match("(?=a)a", "a")))
    match regex_is_match("(", "x"):
        Ok(found):
            return 1
        Err(problem):
            assert(problem.code == "E_REGEX_INVALID")
    var long = ""
    var count = 0
    while count < 30000:
        set long = long + "a"
        set count = count + 1
    assert(unwrap(regex_is_match("(a+)+$", long)))
    assert(not unwrap(regex_is_match("(a+)+b", long)))
    return 0
"##;
    let output = run_source("regex-errors", source);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
