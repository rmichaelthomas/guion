use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct Scratch(PathBuf);
impl Scratch {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("guion-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("root")).unwrap();
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
        .output()
        .expect("guion starts")
}

fn run_with_root(scratch: &Scratch, source: &str) -> Output {
    std::fs::write(scratch.0.join("main.gu"), source).unwrap();
    let root = scratch.root();
    guion(
        &scratch.0,
        &["run", "--file-root", root.to_str().unwrap(), "main.gu"],
    )
}

#[test]
fn filesystem_operations_work_inside_the_granted_root() {
    let scratch = Scratch::new("fs-ops");
    let output = run_with_root(
        &scratch,
        r#"import "std:fs"
fn main() -> Int:
    assert(not unwrap(fs_exists("missing.txt")))
    assert(unwrap(fs_kind("missing.txt")) == "missing")
    unwrap(fs_make_dir("a/b/c"))
    unwrap(fs_make_dir("a/b/c"))
    assert(unwrap(fs_kind("a/b")) == "directory")
    unwrap(write_text("a/b/c/f.txt", "hello"))
    assert(unwrap(fs_kind("a/b/c/f.txt")) == "file")
    assert(unwrap(fs_size("a/b/c/f.txt")) == 5)
    assert(unwrap(fs_modified_ms("a/b/c/f.txt")) > 1000000000000)
    assert(unwrap(fs_copy("a/b/c/f.txt", "a/g.txt")) == 5)
    unwrap(fs_rename("a/g.txt", "a/h.txt"))
    assert(unwrap(fs_list_dir("a")) == ["b", "h.txt"])
    assert(unwrap(read_text("a/h.txt")) == "hello")
    assert(not is_ok(fs_remove("a/b")))
    unwrap(fs_remove("a/h.txt"))
    let temp_file = unwrap(fs_temp_file("job"))
    let temp_dir = unwrap(fs_temp_dir("work"))
    assert(temp_file != temp_dir)
    assert(unwrap(fs_kind(temp_file)) == "file")
    assert(unwrap(fs_kind(temp_dir)) == "directory")
    unwrap(fs_remove_all("a"))
    assert(not unwrap(fs_exists("a")))
    assert(not is_ok(fs_temp_file("bad/prefix")))
    assert(not is_ok(fs_temp_file("")))
    assert(not is_ok(fs_remove_all("")))
    assert(not is_ok(fs_remove_all(".")))
    return 0
"#,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn filesystem_operations_cannot_escape_the_granted_root() {
    let scratch = Scratch::new("fs-escape");
    let outside = scratch.0.join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("secret.txt"), "secret").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, scratch.root().join("link")).unwrap();
    let output = run_with_root(
        &scratch,
        r#"import "std:fs"
fn main() -> Int:
    assert(not is_ok(fs_exists("../outside/secret.txt")))
    assert(not is_ok(fs_list_dir("..")))
    assert(not is_ok(fs_make_dir("../made")))
    assert(not is_ok(fs_remove("../outside/secret.txt")))
    assert(not is_ok(fs_copy("../outside/secret.txt", "copy.txt")))
    assert(not is_ok(fs_rename("x", "../moved")))
    assert(not is_ok(fs_exists("/etc/passwd")))
    assert(not is_ok(fs_list_dir("link")))
    assert(not is_ok(fs_remove("link/secret.txt")))
    return 0
"#,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(outside.join("secret.txt").exists());
    assert!(!scratch.0.join("made").exists());
}

#[test]
fn static_inspection_reports_filesystem_read_and_write_for_new_operations() {
    let scratch = Scratch::new("fs-inspect");
    std::fs::write(
        scratch.0.join("reader.gu"),
        "import \"std:fs\"\nfn main() -> Int:\n    print(unwrap(fs_exists(\"x\")))\n    return 0\n",
    )
    .unwrap();
    std::fs::write(
        scratch.0.join("writer.gu"),
        "import \"std:fs\"\nfn main() -> Int:\n    unwrap(fs_make_dir(\"x\"))\n    return 0\n",
    )
    .unwrap();
    std::fs::write(
        scratch.0.join("walker.gu"),
        "import \"std:fs\"\nfn main() -> Int:\n    print(len(unwrap(fs_walk(\".\"))))\n    return 0\n",
    )
    .unwrap();
    let required_effects = |file: &str| -> Vec<String> {
        let output = guion(&scratch.0, &["inspect", "--json", file]);
        assert!(
            output.status.success(),
            "{file}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        report["required_effects"]
            .as_array()
            .unwrap()
            .iter()
            .map(|effect| effect.as_str().unwrap().to_owned())
            .collect()
    };
    let reader = required_effects("reader.gu");
    assert!(reader.contains(&"filesystem.read".to_owned()));
    assert!(!reader.contains(&"filesystem.write".to_owned()));
    assert!(required_effects("writer.gu").contains(&"filesystem.write".to_owned()));
    assert!(required_effects("walker.gu").contains(&"filesystem.read".to_owned()));
}

#[test]
fn path_functions_follow_posix_semantics() {
    let scratch = Scratch::new("path");
    let output = run_with_root(
        &scratch,
        r#"import "std:path"
fn main() -> Int:
    assert(path_normalize("/a/./b/../c//d") == "/a/c/d")
    assert(path_normalize("a/../..") == "..")
    assert(path_normalize("/..") == "/")
    assert(path_normalize("") == ".")
    assert(path_normalize("./x/") == "x")
    assert(path_join("a", "b.txt") == "a/b.txt")
    assert(path_join("a/", "b") == "a/b")
    assert(path_join("a", "/abs") == "/abs")
    assert(path_join("", "b") == "b")
    assert(path_dirname("/x/y/z.tar.gz") == "/x/y")
    assert(path_dirname("z") == "")
    assert(path_dirname("/z") == "/")
    assert(path_basename("/x/y/z.tar.gz") == "z.tar.gz")
    assert(path_basename("/") == "")
    assert(path_extension("/x/y/z.tar.gz") == ".gz")
    assert(path_extension(".bashrc") == "")
    assert(path_extension("noext") == "")
    assert(path_stem("/x/y/z.tar.gz") == "z.tar")
    assert(path_with_extension("d/f.txt", ".md") == "d/f.md")
    assert(path_with_extension("f.txt", ".md") == "f.md")
    assert(path_is_absolute("/x"))
    assert(not path_is_absolute("x"))
    assert(path_components("/a//b/c/") == ["a", "b", "c"])
    return 0
"#,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn filesystem_instructions_are_rejected_in_artifact_format_53() {
    let scratch = Scratch::new("fs-format");
    std::fs::write(
        scratch.0.join("main.gu"),
        "import \"std:fs\"\nfn main() -> Int:\n    print(unwrap(fs_exists(\"x\")))\n    return 0\n",
    )
    .unwrap();
    let built = guion(&scratch.0, &["build", "main.gu", "--output", "main.gub"]);
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
    let mut bytes = std::fs::read(scratch.0.join("main.gub")).unwrap();
    bytes[4..8].copy_from_slice(&53u32.to_le_bytes());
    std::fs::write(scratch.0.join("old.gub"), bytes).unwrap();
    let output = guion(&scratch.0, &["run", "old.gub"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("E_ARTIFACT_VERSION"));
}
