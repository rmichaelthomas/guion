//! `fs_disk_usage`, `fs_free_space` and `fs_is_link` in `std:fs` (artifact format 91): on-disk size
//! like `du`, free space like `df`, and a link test that does not follow the link.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct Scratch(PathBuf);
impl Scratch {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("guion-fsdu-{name}-{}", std::process::id()));
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

/// Runs `source` from a working directory outside `file_root`.
fn run_with_root(scratch: &Scratch, file_root: &Path, source: &str) -> Output {
    let program = scratch.0.join("main.gu");
    std::fs::write(&program, source).unwrap();
    guion(
        &scratch.0.join("elsewhere"),
        &[
            "run",
            "--file-root",
            file_root.to_str().unwrap(),
            program.to_str().unwrap(),
        ],
    )
}

fn run_from_elsewhere(scratch: &Scratch, source: &str) -> Output {
    run_with_root(scratch, &scratch.root(), source)
}

fn du_bytes(path: &Path) -> u64 {
    let output = Command::new("du").arg("-sk").arg(path).output().unwrap();
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.split_whitespace()
        .next()
        .unwrap()
        .parse::<u64>()
        .unwrap()
        * 1024
}

fn first_int(output: &Output) -> u64 {
    assert!(output.status.success(), "{}", stderr(output));
    stdout(output).lines().next().unwrap().parse().unwrap()
}

#[test]
fn disk_usage_of_sparse_file_is_below_its_size() {
    let scratch = Scratch::new("sparse");
    let file = std::fs::File::create(scratch.root().join("disk.img")).unwrap();
    file.set_len(100 * 1024 * 1024).unwrap();
    drop(file);
    let output = run_from_elsewhere(
        &scratch,
        r#"import "std:fs"
fn main() -> Int:
    print(unwrap(fs_size("disk.img")))
    print(unwrap(fs_disk_usage("disk.img")))
    return 0
"#,
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let lines: Vec<String> = stdout(&output).lines().map(str::to_owned).collect();
    assert_eq!(lines[0], "104857600");
    assert!(lines[1].parse::<u64>().unwrap() < 1_048_576, "{lines:?}");
}

#[test]
fn disk_usage_of_directory_counts_written_bytes() {
    let scratch = Scratch::new("dir");
    std::fs::create_dir_all(scratch.root().join("d")).unwrap();
    std::fs::write(scratch.root().join("d/a.bin"), vec![0xA5u8; 256 * 1024]).unwrap();
    let output = run_from_elsewhere(
        &scratch,
        r#"import "std:fs"
fn main() -> Int:
    print(unwrap(fs_disk_usage("d")))
    return 0
"#,
    );
    let usage = first_int(&output);
    assert!(usage >= 262_144, "{usage}");
    assert_eq!(usage, du_bytes(&scratch.root().join("d")));
}

#[test]
fn disk_usage_counts_hard_links_once() {
    let scratch = Scratch::new("hard");
    std::fs::create_dir_all(scratch.root().join("d")).unwrap();
    std::fs::write(scratch.root().join("d/a"), vec![0x5Au8; 1_048_576]).unwrap();
    std::fs::hard_link(scratch.root().join("d/a"), scratch.root().join("d/b")).unwrap();
    let output = run_from_elsewhere(
        &scratch,
        r#"import "std:fs"
fn main() -> Int:
    print(unwrap(fs_disk_usage("d")))
    return 0
"#,
    );
    let usage = first_int(&output);
    assert!(usage < 2 * 1_048_576, "{usage}");
    assert_eq!(usage, du_bytes(&scratch.root().join("d")));
}

#[test]
fn disk_usage_does_not_follow_links() {
    let scratch = Scratch::new("nofollow");
    let outside = scratch.0.join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("big.bin"), vec![0x11u8; 4 * 1_048_576]).unwrap();
    std::fs::create_dir_all(scratch.root().join("d")).unwrap();
    std::os::unix::fs::symlink(&outside, scratch.root().join("d/out")).unwrap();
    let output = run_from_elsewhere(
        &scratch,
        r#"import "std:fs"
fn main() -> Int:
    print(unwrap(fs_disk_usage("d")))
    return 0
"#,
    );
    let usage = first_int(&output);
    assert!(usage < 1_048_576, "{usage}");
    assert!(outside.join("big.bin").exists());
}

#[test]
fn disk_usage_of_a_link_path_is_the_link_itself() {
    let scratch = Scratch::new("linkpath");
    std::fs::create_dir_all(scratch.root().join("d")).unwrap();
    std::fs::write(scratch.root().join("d/a.bin"), vec![0x22u8; 1_048_576]).unwrap();
    std::os::unix::fs::symlink("d", scratch.root().join("dlink")).unwrap();
    let output = run_from_elsewhere(
        &scratch,
        r#"import "std:fs"
fn main() -> Int:
    print(unwrap(fs_disk_usage("dlink")))
    print(unwrap(fs_is_link("dlink")))
    return 0
"#,
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);
    let lines: Vec<&str> = text.lines().collect();
    assert!(lines[0].parse::<u64>().unwrap() < 65_536, "{lines:?}");
    assert_eq!(lines[1], "true");
}

#[test]
fn is_link_false_for_file_directory_and_missing() {
    let scratch = Scratch::new("islink");
    std::fs::write(scratch.root().join("f.txt"), "x").unwrap();
    std::fs::create_dir_all(scratch.root().join("d")).unwrap();
    std::os::unix::fs::symlink("f.txt", scratch.root().join("l")).unwrap();
    let output = run_from_elsewhere(
        &scratch,
        r#"import "std:fs"
fn main() -> Int:
    print(unwrap(fs_is_link("f.txt")))
    print(unwrap(fs_is_link("d")))
    print(unwrap(fs_is_link("nope")))
    print(unwrap(fs_is_link("l")))
    return 0
"#,
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "false\nfalse\nfalse\ntrue\n");
}

#[test]
fn free_space_matches_statvfs() {
    let scratch = Scratch::new("free");
    let output = run_from_elsewhere(
        &scratch,
        r#"import "std:fs"
fn main() -> Int:
    print(unwrap(fs_free_space(".")))
    return 0
"#,
    );
    let reported = first_int(&output) as f64;
    let c_path = std::ffi::CString::new(scratch.root().to_str().unwrap()).unwrap();
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    assert_eq!(unsafe { libc::statvfs(c_path.as_ptr(), &mut stat) }, 0);
    let expected = stat.f_bavail as f64 * stat.f_frsize as f64;
    assert!(
        (reported - expected).abs() <= expected * 0.01,
        "reported {reported}, statvfs {expected}"
    );
}

#[test]
fn missing_path_is_not_found() {
    let scratch = Scratch::new("missing");
    let output = run_from_elsewhere(
        &scratch,
        r#"import "std:fs"
import "std:text"
fn main() -> Int:
    match fs_disk_usage("nope"):
        Ok(_):
            print("ok")
        Err(problem):
            print(file_error_code(problem))
    return 0
"#,
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "R_FILE_NOT_FOUND\n");
}

#[test]
fn parent_traversal_is_refused() {
    let scratch = Scratch::new("traversal");
    std::fs::write(scratch.0.join("x"), "x").unwrap();
    let output = run_from_elsewhere(
        &scratch,
        r#"import "std:fs"
import "std:text"
fn main() -> Int:
    match fs_disk_usage("../x"):
        Ok(_):
            print("ok")
        Err(problem):
            print(file_error_code(problem))
    match fs_free_space("../x"):
        Ok(_):
            print("ok")
        Err(problem):
            print(file_error_code(problem))
    match fs_is_link("../x"):
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
        "R_HOST_DENIED\nR_HOST_DENIED\nR_HOST_DENIED\n"
    );
}

#[test]
fn root_reached_through_symlink_works() {
    let scratch = Scratch::new("rootlink");
    std::fs::write(scratch.root().join("f.bin"), vec![1u8; 8192]).unwrap();
    let alias = scratch.0.join("rootlink");
    std::os::unix::fs::symlink(scratch.root(), &alias).unwrap();
    let output = run_with_root(
        &scratch,
        &alias,
        r#"import "std:fs"
fn main() -> Int:
    print(unwrap(fs_disk_usage(".")) >= 8192)
    print(unwrap(fs_disk_usage("f.bin")) >= 8192)
    print(unwrap(fs_free_space(".")) > 0)
    print(unwrap(fs_is_link(".")))
    return 0
"#,
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "true\ntrue\ntrue\nfalse\n");
}

#[test]
fn denied_without_file_grant() {
    let scratch = Scratch::new("denied");
    let program = scratch.0.join("main.gu");
    std::fs::write(
        &program,
        r#"import "std:fs"
import "std:text"
fn main() -> Int:
    match fs_disk_usage("a"):
        Ok(_):
            print("ok")
        Err(problem):
            print(file_error_code(problem))
    match fs_free_space("a"):
        Ok(_):
            print("ok")
        Err(problem):
            print(file_error_code(problem))
    match fs_is_link("a"):
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
    assert_eq!(
        stdout(&output),
        "R_HOST_DENIED\nR_HOST_DENIED\nR_HOST_DENIED\n"
    );
}

#[test]
fn inspect_reports_filesystem_read() {
    let scratch = Scratch::new("inspect");
    std::fs::write(
        scratch.0.join("all.gu"),
        "import \"std:fs\"\nfn main() -> Int:\n    print(unwrap(fs_disk_usage(\"x\")))\n    print(unwrap(fs_free_space(\"x\")))\n    print(unwrap(fs_is_link(\"x\")))\n    return 0\n",
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
        effects.contains(&"filesystem.read".to_owned()),
        "{effects:?}"
    );
    assert!(
        !effects.contains(&"filesystem.write".to_owned()),
        "{effects:?}"
    );
}

#[test]
fn format_90_artifact_using_disk_usage_is_rejected() {
    let scratch = Scratch::new("format");
    let program = scratch.0.join("main.gu");
    std::fs::write(
        &program,
        "import \"std:fs\"\nfn main() -> Int:\n    print(is_ok(fs_disk_usage(\"a\")))\n    return 0\n",
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
    assert!(text.contains("require artifact format 91"), "{text}");
}

#[test]
fn unreadable_subdirectory_is_a_permission_error_naming_it() {
    use std::os::unix::fs::PermissionsExt;
    let scratch = Scratch::new("unreadable");
    let sub = scratch.root().join("d/sub");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(sub.join("f"), "x").unwrap();
    std::fs::set_permissions(&sub, std::fs::Permissions::from_mode(0o000)).unwrap();
    let output = run_from_elsewhere(
        &scratch,
        r#"import "std:fs"
import "std:text"
fn main() -> Int:
    match fs_disk_usage("d"):
        Ok(_):
            print("ok")
        Err(problem):
            print(file_error_code(problem))
            print(file_error_message(problem))
    return 0
"#,
    );
    std::fs::set_permissions(&sub, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);
    let mut lines = text.lines();
    // Running as root can read anything; the denial only exists for an ordinary user.
    if lines.next() == Some("ok") {
        return;
    }
    assert!(text.starts_with("R_FILE_PERMISSION\n"), "{text}");
    assert!(text.contains("d/sub"), "{text}");
}

#[cfg(unix)]
#[test]
fn a_recording_replays_without_touching_the_files() {
    let scratch = Scratch::new("tape");
    let file = scratch.root().join("t.txt");
    std::fs::write(&file, vec![7u8; 20_000]).unwrap();
    std::os::unix::fs::symlink("t.txt", scratch.root().join("l")).unwrap();
    let program = scratch.0.join("main.gu");
    std::fs::write(
        &program,
        "import \"std:fs\"\nfn main() -> Int:\n    print(unwrap(fs_disk_usage(\"t.txt\")))\n    print(unwrap(fs_free_space(\".\")))\n    print(unwrap(fs_is_link(\"l\")))\n    print(unwrap(fs_is_link(\"t.txt\")))\n    return 0\n",
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
    let lines: Vec<String> = stdout(&recorded).lines().map(str::to_owned).collect();
    assert_eq!(lines.len(), 4, "{lines:?}");
    assert_eq!(&lines[2..], ["true", "false"]);
    std::fs::remove_file(&file).unwrap();
    std::fs::remove_file(scratch.root().join("l")).unwrap();
    let replayed = run(args("--replay"));
    assert!(replayed.status.success(), "{}", stderr(&replayed));
    assert_eq!(stdout(&replayed), stdout(&recorded));
}
