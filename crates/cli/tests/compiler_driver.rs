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
                "veritas-embedded-compiler-{}-{}-{}",
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
        fs::write(&path, source).expect("write temporary Veritas source");
        path
    }
}

impl Drop for TempProject {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn default_cli_uses_embedded_compiler_without_stage_zero_files_or_path() {
    let project = TempProject::new();
    let source = project.write(
        "main.vr",
        "fn main() -> Int:\n    print(\"embedded\")\n    return 0\n",
    );
    let empty_path = project.0.join("empty-path");
    fs::create_dir(&empty_path).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_veritas"))
        .args(["check", source.to_str().unwrap()])
        .current_dir(&project.0)
        .env("PATH", empty_path)
        .env_remove("VERITAS_STDLIB_ROOT")
        .output()
        .expect("launch installed CLI without stage-zero tools on PATH");
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
        standard.join("greeting.vr"),
        "pub fn greeting() -> Text:\n    return \"from std\"\n",
    )
    .unwrap();
    let source = project.write(
        "main.vr",
        "import \"std:greeting\"\n\nfn main() -> Int:\n    print(greeting())\n    return 0\n",
    );
    let output = Command::new(env!("CARGO_BIN_EXE_veritas"))
        .args(["run", source.to_str().unwrap()])
        .current_dir(&project.0)
        .env("VERITAS_STDLIB_ROOT", &standard)
        .output()
        .expect("compile and run a project with an independently granted standard module");
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "from std\n");
}

#[test]
fn default_cli_cannot_fallback_to_rust_source_compiler() {
    let project = TempProject::new();
    let source = project.write(
        "main.vr",
        "import \"missing.vr\"\n\nfn main() -> Int:\n    return 0\n",
    );
    let output = Command::new(env!("CARGO_BIN_EXE_veritas"))
        .args(["check", source.to_str().unwrap(), "--json"])
        .current_dir(&project.0)
        .output()
        .expect("launch default CLI with an unsupported unresolved import");
    assert!(
        !output.status.success(),
        "missing import must fail compilation"
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout)
        .expect("self-hosted compiler returns the structured diagnostic schema");
    assert_eq!(report["kind"], "veritas_diagnostic");
    assert_eq!(report["diagnostic"]["code"], "E_IMPORT_READ");
    assert!(
        !String::from_utf8_lossy(&output.stderr).contains("stage zero"),
        "the default CLI must not attempt a stage-zero recovery path"
    );
}
