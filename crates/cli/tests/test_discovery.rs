//! `guion test` with no file finds the package's `tests.gu`.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn guion(directory: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(args)
        .current_dir(directory)
        .output()
        .expect("guion starts")
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn package(name: &str, tests: &str) -> PathBuf {
    let directory =
        std::env::temp_dir().join(format!("guion-discovery-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(directory.join("src")).unwrap();
    std::fs::write(
        directory.join("guion.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nentry = \"src/lib.gu\"\n",
    )
    .unwrap();
    std::fs::write(
        directory.join("src/lib.gu"),
        "pub fn one() -> Int:\n    return 1\n",
    )
    .unwrap();
    if !tests.is_empty() {
        std::fs::write(directory.join("tests.gu"), tests).unwrap();
    }
    directory
}

const PASSING: &str = "import \"src/lib.gu\"\n\nfn main() -> Int:\n    return 0\n\nfn test_one() -> Unit:\n    assert(one() == 1)\n";

#[test]
fn a_bare_guion_test_in_a_package_runs_its_tests_file() {
    let directory = package("bare", PASSING);
    let ran = guion(&directory, &["test"]);
    assert!(ran.status.success(), "{}", text(&ran));
    assert!(text(&ran).contains("1 tests passed"), "{}", text(&ran));
}

#[test]
fn a_directory_argument_runs_that_directorys_tests_file() {
    let directory = package("dir", PASSING);
    let ran = guion(
        directory.parent().unwrap(),
        &["test", directory.file_name().unwrap().to_str().unwrap()],
    );
    assert!(ran.status.success(), "{}", text(&ran));
    assert!(text(&ran).contains("1 tests passed"), "{}", text(&ran));
}

#[test]
fn flags_still_work_with_discovery() {
    let directory = package("json", PASSING);
    let ran = guion(&directory, &["test", "--json"]);
    assert!(ran.status.success(), "{}", text(&ran));
    assert!(text(&ran).contains("guion_test_report"), "{}", text(&ran));
}

#[test]
fn a_failing_test_in_a_discovered_file_exits_nonzero() {
    let directory = package(
        "fail",
        "import \"src/lib.gu\"\n\nfn main() -> Int:\n    return 0\n\nfn test_one() -> Unit:\n    assert(one() == 2)\n",
    );
    let ran = guion(&directory, &["test"]);
    assert!(!ran.status.success());
}

#[test]
fn an_explicit_file_still_wins() {
    let directory = package("explicit", PASSING);
    std::fs::write(
        directory.join("other.gu"),
        "fn main() -> Int:\n    return 0\n\nfn test_a() -> Unit:\n    assert(true)\n\nfn test_b() -> Unit:\n    assert(true)\n",
    )
    .unwrap();
    let ran = guion(&directory, &["test", "other.gu"]);
    assert!(ran.status.success(), "{}", text(&ran));
    assert!(text(&ran).contains("2 tests passed"), "{}", text(&ran));
}

#[test]
fn nothing_to_discover_says_where_tests_are_looked_for() {
    let directory = package("none", "");
    std::fs::remove_file(directory.join("guion.toml")).unwrap();
    let ran = guion(&directory, &["test"]);
    assert!(!ran.status.success());
    assert!(text(&ran).contains("tests.gu"), "{}", text(&ran));
}
