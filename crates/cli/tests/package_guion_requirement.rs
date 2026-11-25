//! A package can declare which Guion it needs (`[package] guion = ">=0.9.0"`), and the toolchain
//! refuses a package whose requirement the running Guion does not meet. These tests need no
//! registry: they cover the manifest field, path dependencies and locked dependencies.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

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

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("guion-req-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn library(root: &Path, requirement: Option<&str>) {
    std::fs::create_dir_all(root.join("src")).unwrap();
    let guion_line = requirement
        .map(|text| format!("guion = \"{text}\"\n"))
        .unwrap_or_default();
    std::fs::write(
        root.join("guion.toml"),
        format!("[package]\nname = \"needy\"\nversion = \"1.0.0\"\nentry = \"src/lib.gu\"\nexports = [\"src/lib.gu\"]\n{guion_line}"),
    )
    .unwrap();
    std::fs::write(
        root.join("src/lib.gu"),
        "pub fn answer() -> Int:\n    return 42\n",
    )
    .unwrap();
    std::fs::write(
        root.join("tests.gu"),
        "import \"src/lib.gu\"\n\nfn main() -> Int:\n    return 0\n",
    )
    .unwrap();
}

fn project(root: &Path) {
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("guion.toml"),
        "[project]\nname = \"app\"\nversion = \"0.1.0\"\nentry = \"src/main.gu\"\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/main.gu"),
        "import \"pkg:needy/src/lib.gu\"\n\nfn main() -> Int:\n    print(answer())\n    return 0\n",
    )
    .unwrap();
}

#[test]
fn a_satisfied_or_absent_requirement_changes_nothing() {
    for requirement in [None, Some(">=0.9.0"), Some("*"), Some(">=0.1, <1.0")] {
        let work = scratch("ok");
        library(&work.join("needy"), requirement);
        project(&work.join("app"));
        let added = guion(
            &work.join("app"),
            &[
                "add",
                "needy",
                "--path",
                work.join("needy").to_str().unwrap(),
            ],
        );
        assert!(added.status.success(), "{requirement:?}: {}", text(&added));
        let ran = guion(&work.join("app"), &["run", "--no-cache", "src/main.gu"]);
        assert!(ran.status.success(), "{requirement:?}: {}", text(&ran));
        assert_eq!(String::from_utf8_lossy(&ran.stdout), "42\n");
        let _ = std::fs::remove_dir_all(&work);
    }
}

#[test]
fn an_invalid_requirement_is_rejected_by_check_naming_the_field() {
    let work = scratch("invalid");
    library(&work.join("needy"), Some("newer than 0.9"));
    let checked = guion(&work.join("needy"), &["check", "tests.gu"]);
    assert!(!checked.status.success(), "{}", text(&checked));
    let message = text(&checked);
    assert!(
        message.contains("[package].guion") && message.contains("newer than 0.9"),
        "{message}"
    );
    let _ = std::fs::remove_dir_all(&work);
}

#[test]
fn an_invalid_requirement_is_rejected_by_publish_before_any_network_use() {
    let work = scratch("invalid-publish");
    library(&work.join("needy"), Some(">="));
    let published = guion(&work.join("needy"), &["publish"]);
    assert!(!published.status.success());
    assert!(
        text(&published).contains("[package].guion"),
        "{}",
        text(&published)
    );
    let _ = std::fs::remove_dir_all(&work);
}

#[test]
fn a_path_dependency_whose_requirement_is_unmet_is_refused() {
    let work = scratch("path");
    library(&work.join("needy"), Some(">=99.0.0"));
    project(&work.join("app"));
    let added = guion(
        &work.join("app"),
        &[
            "add",
            "needy",
            "--path",
            work.join("needy").to_str().unwrap(),
        ],
    );
    assert!(!added.status.success(), "{}", text(&added));
    let message = text(&added);
    assert!(message.contains("E_PACKAGE_GUION_VERSION"), "{message}");
    assert!(
        message.contains("`needy`") && message.contains("1.0.0"),
        "{message}"
    );
    assert!(message.contains(">=99.0.0"), "{message}");
    assert!(message.contains(env!("CARGO_PKG_VERSION")), "{message}");
    assert!(message.contains("upgrade Guion"), "{message}");
    let _ = std::fs::remove_dir_all(&work);
}

#[test]
fn a_locked_dependency_whose_requirement_is_unmet_fails_check_and_run() {
    // The lock was made by a newer Guion that met the requirement; this one does not. The
    // installed copy is edited to say so, as it would read had the package always said it.
    let work = scratch("locked");
    library(&work.join("needy"), None);
    project(&work.join("app"));
    let app = work.join("app");
    let added = guion(
        &app,
        &[
            "add",
            "needy",
            "--path",
            work.join("needy").to_str().unwrap(),
        ],
    );
    assert!(added.status.success(), "{}", text(&added));
    let installed = app.join(".guion/packages/needy/1.0.0/guion.toml");
    let manifest = std::fs::read_to_string(&installed).unwrap();
    std::fs::write(
        &installed,
        manifest.replace("[package]\n", "[package]\nguion = \">=99.0.0\"\n"),
    )
    .unwrap();
    for arguments in [
        vec!["check", "src/main.gu"],
        vec!["run", "--no-cache", "src/main.gu"],
    ] {
        let failed = guion(&app, &arguments);
        assert!(!failed.status.success(), "{arguments:?}: {}", text(&failed));
        assert!(
            text(&failed).contains("E_PACKAGE_GUION_VERSION"),
            "{arguments:?}: {}",
            text(&failed)
        );
    }
    let _ = std::fs::remove_dir_all(&work);
}
