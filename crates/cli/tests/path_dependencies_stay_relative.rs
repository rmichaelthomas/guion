//! Known issue 28: `guion add --path` and `guion update` keep a relative path dependency
//! relative, so a manifest with one is portable between machines and checkouts.

use std::process::Command;

fn guion(directory: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(args)
        .current_dir(directory)
        .output()
        .unwrap()
}

#[test]
fn add_and_update_write_the_path_relative_to_the_project() {
    let root = std::env::temp_dir().join(format!("guion-relpath-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("app/src")).unwrap();
    std::fs::create_dir_all(root.join("lib/src")).unwrap();
    std::fs::write(
        root.join("lib/guion.toml"),
        "[package]\nname = \"mylib\"\nversion = \"0.1.0\"\nentry = \"src/lib.gu\"\nexports = [\"src/lib.gu\"]\n",
    )
    .unwrap();
    std::fs::write(root.join("lib/src/lib.gu"), "pub fn five() -> Int:\n    return 5\n").unwrap();
    std::fs::write(
        root.join("app/guion.toml"),
        "[project]\nname = \"app\"\nversion = \"0.1.0\"\nentry = \"src/main.gu\"\n",
    )
    .unwrap();
    std::fs::write(root.join("app/src/main.gu"), "fn main() -> Int:\n    return 0\n").unwrap();
    let app = root.join("app");

    let added = guion(&app, &["add", "mylib", "--path", "../lib"]);
    assert!(added.status.success(), "{}", String::from_utf8_lossy(&added.stderr));
    let updated = guion(&app, &["update"]);
    assert!(updated.status.success(), "{}", String::from_utf8_lossy(&updated.stderr));

    let manifest = std::fs::read_to_string(app.join("guion.toml")).unwrap();
    let lock = std::fs::read_to_string(app.join("guion.lock")).unwrap();
    assert!(manifest.contains("path = \"../lib\""), "{manifest}");
    assert!(lock.contains("source = \"../lib\""), "{lock}");
    let absolute = root.to_string_lossy().into_owned();
    assert!(!manifest.contains(&absolute) && !lock.contains(&absolute));
}
