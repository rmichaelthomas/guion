//! `fs_set_mode` and `fs_mode` in `std:fs` (artifact format 90): permission bits as octal text,
//! resolved against the file root like every other file operation.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct Scratch(PathBuf);
impl Scratch {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("guion-fsmode-{name}-{}", std::process::id()));
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

/// Runs `source` with the process working directory at `elsewhere`, outside the file root, so a
/// path that resolved against the working directory would miss the root.
fn run_from_elsewhere(scratch: &Scratch, source: &str) -> Output {
    let program = scratch.0.join("main.gu");
    std::fs::write(&program, source).unwrap();
    let root = scratch.root();
    guion(
        &scratch.0.join("elsewhere"),
        &[
            "run",
            "--file-root",
            root.to_str().unwrap(),
            program.to_str().unwrap(),
        ],
    )
}

#[cfg(unix)]
fn mode_of(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).unwrap().permissions().mode() & 0o7777
}

#[cfg(unix)]
#[test]
fn setting_600_on_a_file_reads_back_600() {
    let scratch = Scratch::new("file");
    std::fs::write(scratch.root().join("secret.txt"), "x").unwrap();
    let output = run_from_elsewhere(
        &scratch,
        r#"import "std:fs"
fn main() -> Int:
    unwrap(fs_set_mode("secret.txt", "600"))
    print(unwrap(fs_mode("secret.txt")))
    unwrap(fs_set_mode("secret.txt", "0644"))
    print(unwrap(fs_mode("secret.txt")))
    unwrap(fs_set_mode("secret.txt", "4755"))
    print(unwrap(fs_mode("secret.txt")))
    return 0
"#,
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "600\n644\n4755\n");
    assert_eq!(mode_of(&scratch.root().join("secret.txt")), 0o4755);
}

#[cfg(unix)]
#[test]
fn setting_700_on_a_directory_works() {
    let scratch = Scratch::new("dir");
    std::fs::create_dir_all(scratch.root().join("private")).unwrap();
    let output = run_from_elsewhere(
        &scratch,
        r#"import "std:fs"
fn main() -> Int:
    unwrap(fs_set_mode("private", "700"))
    print(unwrap(fs_mode("private")))
    return 0
"#,
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "700\n");
    assert_eq!(mode_of(&scratch.root().join("private")), 0o700);
}

#[cfg(unix)]
#[test]
fn a_relative_path_resolves_under_the_file_root_not_the_working_directory() {
    let scratch = Scratch::new("cwd");
    std::fs::write(scratch.root().join("target.txt"), "x").unwrap();
    std::fs::write(scratch.0.join("elsewhere").join("target.txt"), "x").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(
        scratch.root().join("target.txt"),
        std::fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    std::fs::set_permissions(
        scratch.0.join("elsewhere").join("target.txt"),
        std::fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    let output = run_from_elsewhere(
        &scratch,
        r#"import "std:fs"
fn main() -> Int:
    unwrap(fs_set_mode("target.txt", "600"))
    return 0
"#,
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(mode_of(&scratch.root().join("target.txt")), 0o600);
    assert_eq!(
        mode_of(&scratch.0.join("elsewhere").join("target.txt")),
        0o644
    );
}

#[cfg(unix)]
#[test]
fn a_symlink_is_followed_like_chmod() {
    let scratch = Scratch::new("link");
    std::fs::write(scratch.root().join("real.txt"), "x").unwrap();
    std::os::unix::fs::symlink("real.txt", scratch.root().join("alias.txt")).unwrap();
    let output = run_from_elsewhere(
        &scratch,
        r#"import "std:fs"
fn main() -> Int:
    unwrap(fs_set_mode("alias.txt", "640"))
    print(unwrap(fs_mode("real.txt")))
    return 0
"#,
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "640\n");
}

#[cfg(unix)]
#[test]
fn paths_outside_the_root_and_missing_paths_are_refused() {
    let scratch = Scratch::new("escape");
    std::fs::write(scratch.0.join("outside.txt"), "x").unwrap();
    let before = mode_of(&scratch.0.join("outside.txt"));
    let output = run_from_elsewhere(
        &scratch,
        r#"import "std:fs"
fn code_of(result: Result[Unit, FileError]) -> Text:
    if is_ok(result):
        return "ok"
    return file_error_code(error(result))

fn main() -> Int:
    print(code_of(fs_set_mode("../outside.txt", "600")))
    print(code_of(fs_set_mode("missing.txt", "600")))
    assert(not is_ok(fs_mode("../outside.txt")))
    match fs_mode("missing.txt"):
        Ok(_):
            print("ok")
        Err(problem):
            print(file_error_code(problem))
    return 0
"#,
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        stdout(&output),
        "R_HOST_DENIED\nR_FILE_NOT_FOUND\nR_FILE_NOT_FOUND\n"
    );
    assert_eq!(mode_of(&scratch.0.join("outside.txt")), before);
}

#[cfg(unix)]
#[test]
fn invalid_modes_are_r_file_mode_invalid() {
    let scratch = Scratch::new("invalid");
    std::fs::write(scratch.root().join("f.txt"), "x").unwrap();
    let before = mode_of(&scratch.root().join("f.txt"));
    let output = run_from_elsewhere(
        &scratch,
        r#"import "std:fs"
fn main() -> Int:
    for mode in ["", "8", "12345", "abc", "-1", "10000", " 600", "7777x"]:
        match fs_set_mode("f.txt", mode):
            Ok(_):
                print("ok")
            Err(problem):
                print(file_error_code(problem))
    return 0
"#,
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "R_FILE_MODE_INVALID\n".repeat(8));
    assert_eq!(mode_of(&scratch.root().join("f.txt")), before);
}

#[test]
fn without_a_file_root_both_functions_are_denied() {
    let scratch = Scratch::new("denied");
    let program = scratch.0.join("main.gu");
    std::fs::write(
        &program,
        r#"import "std:fs"
fn main() -> Int:
    match fs_set_mode("a.txt", "600"):
        Ok(_):
            print("ok")
        Err(problem):
            print(file_error_code(problem))
    match fs_mode("a.txt"):
        Ok(_):
            print("ok")
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
    assert_eq!(stdout(&output), "R_HOST_DENIED\nR_HOST_DENIED\n");
}

#[test]
fn an_absolute_path_is_denied_with_the_same_fix_as_the_other_file_operations() {
    let scratch = Scratch::new("absolute");
    std::fs::write(scratch.0.join("outside.txt"), "x").unwrap();
    let outside = scratch.0.join("outside.txt");
    let folder = scratch.0.clone();
    let source = format!(
        "import \"std:fs\"\nfn main() -> Int:\n    match fs_set_mode(\"{0}\", \"600\"):\n        Ok(_):\n            print(\"ok\")\n        Err(problem):\n            print(file_error_code(problem))\n    return 0\n",
        outside.display()
    );
    let program = scratch.0.join("main.gu");
    std::fs::write(&program, source).unwrap();
    let root = scratch.root();
    let output = guion(
        &scratch.0.join("elsewhere"),
        &[
            "run",
            "--json",
            "--file-root",
            root.to_str().unwrap(),
            program.to_str().unwrap(),
        ],
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["program_stdout"], "R_HOST_DENIED\n", "{report}");
    let denial = &report["denials"][0];
    assert_eq!(denial["code"], "R_HOST_DENIED", "{report}");
    assert_eq!(
        denial["grant"]["flag"],
        format!("--file-root {}", folder.display()),
        "{report}"
    );
}

#[test]
fn an_artifact_below_format_90_that_uses_them_is_refused() {
    let scratch = Scratch::new("format");
    let program = scratch.0.join("main.gu");
    std::fs::write(
        &program,
        "import \"std:fs\"\nfn main() -> Int:\n    print(is_ok(fs_mode(\"a\")))\n    return 0\n",
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
    bytes[4..8].copy_from_slice(&89u32.to_le_bytes());
    std::fs::write(&artifact, bytes).unwrap();
    let output = guion(&scratch.0, &["run", artifact.to_str().unwrap()]);
    assert!(!output.status.success());
    let text = format!("{}{}", stdout(&output), stderr(&output));
    assert!(text.contains("E_ARTIFACT_VERSION"), "{text}");
}

#[test]
fn inspect_reports_filesystem_write_and_read() {
    let scratch = Scratch::new("inspect");
    std::fs::write(
        scratch.0.join("writer.gu"),
        "import \"std:fs\"\nfn main() -> Int:\n    unwrap(fs_set_mode(\"x\", \"600\"))\n    return 0\n",
    )
    .unwrap();
    std::fs::write(
        scratch.0.join("reader.gu"),
        "import \"std:fs\"\nfn main() -> Int:\n    print(unwrap(fs_mode(\"x\")))\n    return 0\n",
    )
    .unwrap();
    let effects = |file: &str| -> Vec<String> {
        let output = guion(&scratch.0, &["inspect", "--json", file]);
        assert!(output.status.success(), "{file}: {}", stderr(&output));
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        report["required_effects"]
            .as_array()
            .unwrap()
            .iter()
            .map(|effect| effect.as_str().unwrap().to_owned())
            .collect()
    };
    assert_eq!(effects("writer.gu"), vec!["filesystem.write".to_owned()]);
    let reader = effects("reader.gu");
    assert!(reader.contains(&"filesystem.read".to_owned()), "{reader:?}");
    assert!(
        !reader.contains(&"filesystem.write".to_owned()),
        "{reader:?}"
    );
}

#[test]
fn guion_doc_lists_both_functions() {
    let scratch = Scratch::new("doc");
    let output = guion(&scratch.0, &["doc", "std:fs"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);
    assert!(
        text.contains("fs_set_mode(path: Text, mode: Text) -> Result[Unit, FileError]"),
        "{text}"
    );
    assert!(
        text.contains("fs_mode(path: Text) -> Result[Text, FileError]"),
        "{text}"
    );
}

#[cfg(unix)]
#[test]
fn a_recording_replays_without_touching_the_file() {
    let scratch = Scratch::new("tape");
    let file = scratch.root().join("t.txt");
    std::fs::write(&file, "x").unwrap();
    let program = scratch.0.join("main.gu");
    std::fs::write(
        &program,
        "import \"std:fs\"\nfn main() -> Int:\n    unwrap(fs_set_mode(\"t.txt\", \"600\"))\n    print(unwrap(fs_mode(\"t.txt\")))\n    return 0\n",
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
    assert_eq!(stdout(&recorded), "600\n");
    assert_eq!(mode_of(&file), 0o600);
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
    let replayed = run(args("--replay"));
    assert!(replayed.status.success(), "{}", stderr(&replayed));
    assert_eq!(stdout(&replayed), "600\n");
    assert_eq!(mode_of(&file), 0o644);
}
