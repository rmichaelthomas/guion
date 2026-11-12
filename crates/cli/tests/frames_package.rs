//! The `frames` package checks and passes its own tests, and an application can depend on it
//! by path.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn package() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packages/frames")
}

fn guion(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(args)
        .current_dir(dir)
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

#[test]
fn package_checks_cleanly() {
    let checked = guion(&package(), &["check", "tests.gu"]);
    assert!(checked.status.success(), "{}", text(&checked));
    assert!(!text(&checked).contains("warning"), "{}", text(&checked));
}

#[test]
fn package_tests_pass() {
    let tested = guion(&package(), &["test", "tests.gu"]);
    assert!(tested.status.success(), "{}", text(&tested));
}

#[test]
fn an_application_uses_the_package_by_path() {
    let dir = std::env::temp_dir().join(format!("guion-frames-app-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("guion.toml"),
        "[project]\nname = \"app\"\nversion = \"0.1.0\"\nentry = \"src/main.gu\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.gu"),
        "import \"pkg:frames/src/lib.gu\"\n\nfn main() -> Unit:\n    let frame = unwrap(frame_new([column_i64(\"a\", [1, 2, 3])]))\n    print(frame_rows(frame))\n",
    )
    .unwrap();
    let added = guion(
        &dir,
        &["add", "frames", "--path", package().to_str().unwrap()],
    );
    assert!(added.status.success(), "{}", text(&added));
    let ran = guion(&dir, &["run", "src/main.gu"]);
    assert!(ran.status.success(), "{}", text(&ran));
    assert_eq!(String::from_utf8_lossy(&ran.stdout), "3\n");
    let _ = std::fs::remove_dir_all(&dir);
}
