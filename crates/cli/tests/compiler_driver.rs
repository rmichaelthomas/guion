use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

struct TempProject(PathBuf);

impl TempProject {
    fn new() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        loop {
            let path = std::env::temp_dir().join(format!(
                "guion-embedded-compiler-{}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos(),
                NEXT_ID.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("create isolated source root: {error}"),
            }
        }
    }

    fn write(&self, name: &str, source: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, source).expect("write temporary Guion source");
        path
    }
}

impl Drop for TempProject {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn default_cli_compiles_without_external_compiler_tools() {
    let project = TempProject::new();
    let source = project.write(
        "main.gu",
        "fn main() -> Int:\n    print(\"compiled\")\n    return 0\n",
    );
    let empty_path = project.0.join("empty-path");
    fs::create_dir(&empty_path).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["check", source.to_str().unwrap()])
        .current_dir(&project.0)
        .env("PATH", empty_path)
        .env_remove("GUION_STDLIB_ROOT")
        .output()
        .expect("launch Guion with no external compiler tools on PATH");
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn default_cli_resolves_standard_modules_through_their_virtual_grant() {
    let project = TempProject::new();
    let standard = project.0.join("standard-library");
    fs::create_dir_all(&standard).unwrap();
    fs::write(
        standard.join("greeting.gu"),
        "pub fn greeting() -> Text:\n    return \"from std\"\n",
    )
    .unwrap();
    let source = project.write(
        "main.gu",
        "import \"std:greeting\"\n\nfn main() -> Int:\n    print(greeting())\n    return 0\n",
    );
    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", source.to_str().unwrap()])
        .current_dir(&project.0)
        .env("GUION_STDLIB_ROOT", &standard)
        .output()
        .expect("Rust compiler resolves a standard module through its explicit root");
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "from std\n");
}

#[test]
fn default_cli_reports_rust_compiler_import_diagnostics() {
    let project = TempProject::new();
    let source = project.write(
        "main.gu",
        "import \"missing.gu\"\n\nfn main() -> Int:\n    return 0\n",
    );
    let expected = guion_compiler::compile_project(&source, &project.0)
        .expect_err("the Rust compiler must reject the missing import");
    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["check", "--json", source.to_str().unwrap()])
        .current_dir(&project.0)
        .output()
        .expect("run normal CLI compiler on a missing import");
    assert!(!output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout)
        .expect("the normal CLI emits a structured compiler diagnostic");
    assert_eq!(report["kind"], "guion_diagnostic");
    assert_eq!(report["diagnostic"]["code"], expected.code);
    assert_eq!(report["diagnostic"]["message"], expected.message);
    assert_eq!(
        report["diagnostic"]["source"],
        expected
            .source_name
            .unwrap_or_else(|| source.to_string_lossy().into_owned())
    );
}
