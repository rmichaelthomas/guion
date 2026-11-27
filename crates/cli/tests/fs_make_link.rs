//! `fs_make_link` in `std:fs` (artifact format 91): a symbolic link whose target text is stored as
//! given, created inside the file root.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct Scratch(PathBuf);
impl Scratch {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("guion-fsml-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("root")).unwrap();
        std::fs::create_dir_all(dir.join("elsewhere")).unwrap();
        Scratch(dir)
    }
    fn root(&self) -> PathBuf {
        self.0.join("root")
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn guion(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(args)
        .current_dir(dir)
        .env("GUION_NO_PROMPT", "1")
        .output()
        .expect("guion starts")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// Runs `source` from a working directory outside the file root.
fn run_from_elsewhere(scratch: &Scratch, source: &str) -> Output {
    let program = scratch.0.join("main.gu");
    std::fs::write(&program, source).unwrap();
    guion(
        &scratch.0.join("elsewhere"),
        &[
            "run",
            "--file-root",
            scratch.root().to_str().unwrap(),
            program.to_str().unwrap(),
        ],
    )
}

#[test]
fn make_link_creates_relative_link() {
    let scratch = Scratch::new("relative");
    std::fs::write(scratch.root().join("real.txt"), "hello").unwrap();
    let output = run_from_elsewhere(
        &scratch,
        r#"import "std:fs"
fn main() -> Int:
    unwrap(fs_make_link("real.txt", "alias.txt"))
    print(unwrap(fs_is_link("alias.txt")))
    print(unwrap(read_text("alias.txt")))
    return 0
"#,
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "true\nhello\n");
    assert_eq!(
        std::fs::read_link(scratch.root().join("alias.txt")).unwrap(),
        Path::new("real.txt")
    );
}

#[test]
fn make_link_dangling_allowed() {
    let scratch = Scratch::new("dangling");
    let output = run_from_elsewhere(
        &scratch,
        r#"import "std:fs"
fn main() -> Int:
    unwrap(fs_make_link("nothing/here", "dead"))
    print(unwrap(fs_is_link("dead")))
    print(unwrap(fs_exists("dead")))
    return 0
"#,
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "true\nfalse\n");
}

#[test]
fn make_link_outside_target_is_stored_but_reads_stay_confined() {
    let scratch = Scratch::new("outside");
    let secret = scratch.0.join("elsewhere").join("secret.txt");
    std::fs::write(&secret, "secret").unwrap();
    let output = run_from_elsewhere(
        &scratch,
        &format!(
            r#"import "std:fs"
import "std:text"
fn main() -> Int:
    match fs_make_link("{}", "out"):
        Ok(_):
            print("created")
        Err(problem):
            print(file_error_code(problem))
    match fs_size("out"):
        Ok(_):
            print("read")
        Err(problem):
            print(file_error_code(problem))
    return 0
"#,
            secret.display()
        ),
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "created\nR_HOST_DENIED\n");
    assert_eq!(
        std::fs::read_link(scratch.root().join("out")).unwrap(),
        secret
    );
}

#[test]
fn make_link_existing_entry_refused() {
    let scratch = Scratch::new("exists");
    std::fs::write(scratch.root().join("taken"), "x").unwrap();
    let output = run_from_elsewhere(
        &scratch,
        r#"import "std:fs"
import "std:text"
fn main() -> Int:
    match fs_make_link("a", "taken"):
        Ok(_):
            print("created")
        Err(problem):
            print(file_error_code(problem))
    unwrap(fs_make_link("a", "twice"))
    match fs_make_link("b", "twice"):
        Ok(_):
            print("created")
        Err(problem):
            print(file_error_code(problem))
    return 0
"#,
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "R_FILE_EXISTS\nR_FILE_EXISTS\n");
    assert_eq!(
        std::fs::read_to_string(scratch.root().join("taken")).unwrap(),
        "x"
    );
}

#[test]
fn make_link_parent_outside_root_refused() {
    let scratch = Scratch::new("parent");
    let output = run_from_elsewhere(
        &scratch,
        r#"import "std:fs"
import "std:text"
fn main() -> Int:
    match fs_make_link("a", "../x"):
        Ok(_):
            print("created")
        Err(problem):
            print(file_error_code(problem))
    return 0
"#,
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "R_HOST_DENIED\n");
    assert!(std::fs::symlink_metadata(scratch.0.join("x")).is_err());
}

#[test]
fn make_link_needs_write_grant() {
    let scratch = Scratch::new("denied");
    let program = scratch.0.join("main.gu");
    std::fs::write(
        &program,
        r#"import "std:fs"
import "std:text"
fn main() -> Int:
    match fs_make_link("a", "l"):
        Ok(_):
            print("created")
        Err(problem):
            print(file_error_code(problem))
    return 0
"#,
    )
    .unwrap();
    let output = guion(
        &scratch.0.join("elsewhere"),
        &["run", "--sealed", program.to_str().unwrap()],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "R_HOST_DENIED\n");
}

#[test]
fn inspect_reports_filesystem_write() {
    let scratch = Scratch::new("inspect");
    std::fs::write(
        scratch.0.join("all.gu"),
        "import \"std:fs\"\nfn main() -> Int:\n    unwrap(fs_make_link(\"a\", \"b\"))\n    return 0\n",
    )
    .unwrap();
    let output = guion(&scratch.0, &["inspect", "--json", "all.gu"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let effects: Vec<String> = report["required_effects"]
        .as_array()
        .unwrap()
        .iter()
        .map(|effect| effect.as_str().unwrap().to_owned())
        .collect();
    assert!(
        effects.contains(&"filesystem.write".to_owned()),
        "{effects:?}"
    );
}

#[test]
fn format_90_artifact_using_make_link_is_rejected() {
    let scratch = Scratch::new("format");
    let program = scratch.0.join("main.gu");
    std::fs::write(
        &program,
        "import \"std:fs\"\nfn main() -> Int:\n    print(is_ok(fs_make_link(\"a\", \"b\")))\n    return 0\n",
    )
    .unwrap();
    let artifact = scratch.0.join("main.gub");
    let build = guion(
        &scratch.0,
        &[
            "build",
            program.to_str().unwrap(),
            "--output",
            artifact.to_str().unwrap(),
        ],
    );
    assert!(build.status.success(), "{}", stderr(&build));
    let mut bytes = std::fs::read(&artifact).unwrap();
    bytes[4..8].copy_from_slice(&90u32.to_le_bytes());
    std::fs::write(&artifact, bytes).unwrap();
    let output = guion(&scratch.0, &["run", artifact.to_str().unwrap()]);
    assert!(!output.status.success());
    let text = format!("{}{}", stdout(&output), stderr(&output));
    assert!(text.contains("E_ARTIFACT_VERSION"), "{text}");
    // A program that imports `std:fs` carries every wrapper, so the first gated function the
    // reader meets names the format; the exact `fs_make_link` message is pinned in the model.
    assert!(text.contains("artifact format 91"), "{text}");
}

#[test]
fn a_recording_replays_without_touching_the_files() {
    let scratch = Scratch::new("tape");
    let program = scratch.0.join("main.gu");
    std::fs::write(
        &program,
        "import \"std:fs\"\nfn main() -> Int:\n    print(is_ok(fs_make_link(\"t.txt\", \"l\")))\n    print(unwrap(fs_is_link(\"l\")))\n    return 0\n",
    )
    .unwrap();
    let root = scratch.root();
    let tape = scratch.0.join("run.tape");
    let args = |mode: &'static str| {
        vec![
            "run".to_owned(),
            mode.to_owned(),
            tape.to_str().unwrap().to_owned(),
            "--file-root".to_owned(),
            root.to_str().unwrap().to_owned(),
            program.to_str().unwrap().to_owned(),
        ]
    };
    let run = |arguments: Vec<String>| {
        let borrowed: Vec<&str> = arguments.iter().map(String::as_str).collect();
        guion(&scratch.0.join("elsewhere"), &borrowed)
    };
    let recorded = run(args("--record"));
    assert!(recorded.status.success(), "{}", stderr(&recorded));
    assert_eq!(stdout(&recorded), "true\ntrue\n");
    std::fs::remove_file(scratch.root().join("l")).unwrap();
    let replayed = run(args("--replay"));
    assert!(replayed.status.success(), "{}", stderr(&replayed));
    assert_eq!(stdout(&replayed), stdout(&recorded));
    assert!(std::fs::symlink_metadata(scratch.root().join("l")).is_err());
}
