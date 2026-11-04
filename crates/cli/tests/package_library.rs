use std::path::Path;
use std::process::{Command, Output};

fn guion(directory: &Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(arguments)
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

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("guion-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn new_lib_creates_a_library_that_checks_tests_and_documents() {
    let dir = scratch("lib-new");
    let created = guion(&dir, &["new", "--lib", "my-lib"]);
    assert!(created.status.success(), "{}", text(&created));
    let library = dir.join("my-lib");
    let manifest = std::fs::read_to_string(library.join("guion.toml")).unwrap();
    assert!(manifest.contains("[package]") && manifest.contains("name = \"my-lib\""));

    let checked = guion(&library, &["check", "tests.gu"]);
    assert!(checked.status.success(), "{}", text(&checked));
    assert!(
        text(&checked).contains("library my-lib 0.1.0"),
        "{}",
        text(&checked)
    );

    let tested = guion(&library, &["test", "tests.gu"]);
    assert!(tested.status.success(), "{}", text(&tested));
    let documented = guion(&library, &["doc", "tests.gu"]);
    assert!(text(&documented).contains("greet"), "{}", text(&documented));

    // A library whose only module exposes nothing public fails the surface check.
    std::fs::write(
        library.join("src/lib.gu"),
        "fn hidden() -> Int:\n    return 1\n",
    )
    .unwrap();
    std::fs::write(
        library.join("tests.gu"),
        "import \"src/lib.gu\"\n\nfn main() -> Int:\n    return 0\n",
    )
    .unwrap();
    let hidden = guion(&library, &["check", "tests.gu"]);
    assert!(!hidden.status.success());
    assert!(
        text(&hidden).contains("no `pub` items"),
        "{}",
        text(&hidden)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn new_lib_enforces_the_name_rules() {
    let dir = scratch("lib-names");
    for (name, expected) in [
        ("Upper", "must be lowercase"),
        ("regex", "reserved"),
        ("guion-tools", "reserved"),
        ("9lives", "start with a letter"),
    ] {
        let output = guion(&dir, &["new", "--lib", name]);
        assert!(!output.status.success(), "{name}");
        assert!(
            text(&output).contains(expected),
            "{name}: {}",
            text(&output)
        );
        assert!(!dir.join(name).exists(), "{name} must not be created");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
