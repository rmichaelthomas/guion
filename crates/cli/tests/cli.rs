use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    process::Command,
    thread,
};

fn python3_path() -> String {
    let output = Command::new("python3")
        .args(["-c", "import sys; print(sys.executable)"])
        .output()
        .unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

#[test]
fn new_creates_a_guion_project_using_guion_names() {
    let base = std::env::temp_dir().join(format!("guion-new-smoke-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    let project = base.join("hello-guion");

    let created = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["new", project.to_str().unwrap()])
        .output()
        .unwrap();

    assert!(
        created.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&created.stdout),
        String::from_utf8_lossy(&created.stderr)
    );
    assert!(project.join("src/main.gu").is_file());
    assert!(project.join("guion.toml").is_file());
    assert!(!project.join("src/main.vr").exists());
    assert!(!project.join("veritas.toml").exists());
    let source = fs::read_to_string(project.join("src/main.gu")).unwrap();
    assert!(source.contains("Hello, Guion!"));
    let guide = fs::read_to_string(project.join("README.md")).unwrap();
    assert!(guide.contains("guion check"));
    assert!(guide.contains("src/main.gu"));
    let _ = fs::remove_dir_all(base);
}

#[test]
fn legacy_veritas_project_names_remain_readable_during_the_transition() {
    let base = std::env::temp_dir().join(format!("guion-legacy-project-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(base.join("src")).unwrap();
    fs::write(
        base.join("veritas.toml"),
        "[project]\nname = \"legacy-project\"\nversion = \"0.1.0\"\nentry = \"src/main.vr\"\n",
    )
    .unwrap();
    fs::write(
        base.join("src/main.vr"),
        "fn main() -> Int:\n    print(\"legacy input works\")\n    return 0\n",
    )
    .unwrap();

    let run = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", base.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "legacy input works\n");
    let _ = fs::remove_dir_all(base);
}

fn copy_project_without_managed_environment(
    source: &std::path::Path,
    destination: &std::path::Path,
) {
    fs::create_dir_all(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        if entry.file_name() == ".guion" || entry.file_name() == ".veritas" {
            continue;
        }
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let kind = entry.file_type().unwrap();
        if kind.is_dir() {
            copy_project_without_managed_environment(&source_path, &destination_path);
        } else if kind.is_file() {
            fs::copy(source_path, destination_path).unwrap();
        } else {
            panic!(
                "unsupported project fixture entry: {}",
                source_path.display()
            );
        }
    }
}

struct RemoveDirOnDrop(std::path::PathBuf);

impl Drop for RemoveDirOnDrop {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn sync_json_reports_arguments_and_missing_python_configuration() {
    let base = std::env::temp_dir().join(format!("guion-json-sync-{}", std::process::id()));
    let project = base.join("plain project");
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(&project).unwrap();
    fs::write(
        project.join("guion.toml"),
        "[project]\nname = \"sync-json-test\"\nversion = \"0.1.0\"\nentry = \"main.gu\"\n",
    )
    .unwrap();
    fs::write(project.join("main.gu"), "fn main() -> Int:\n    return 0\n").unwrap();

    let no_python = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["sync", "--json", project.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(no_python.status.code(), Some(1));
    assert!(no_python.stderr.is_empty());
    let report: serde_json::Value = serde_json::from_slice(&no_python.stdout).unwrap();
    assert_eq!(report["kind"], "guion_sync_report");
    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["status"], "failed");
    assert_eq!(report["diagnostic"]["code"], "E_SYNC_CONFIGURATION");

    let missing = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["sync", "--json", base.join("missing").to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(missing.status.code(), Some(1));
    let report: serde_json::Value = serde_json::from_slice(&missing.stdout).unwrap();
    assert_eq!(report["kind"], "guion_sync_report");
    assert_eq!(report["diagnostic"]["code"], "E_PROJECT_NOT_FOUND");

    let args = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["sync", "--json"])
        .output()
        .unwrap();
    assert_eq!(args.status.code(), Some(1));
    let report: serde_json::Value = serde_json::from_slice(&args.stdout).unwrap();
    assert_eq!(report["diagnostic"]["code"], "E_SYNC_ARGUMENT");
    let _ = fs::remove_dir_all(base);
}

#[cfg(unix)]
#[test]
fn failed_dependency_update_preserves_the_previous_locked_environment() {
    let base = std::env::temp_dir().join(format!(
        "guion-sync-update-recovery-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let bridge = base.join(".guion/python-bridge");
    fs::create_dir_all(bridge.join(".venv/bin")).unwrap();
    fs::write(
        base.join("guion.toml"),
        "[project]\nname = \"sync-recovery\"\nversion = \"0.1.0\"\nentry = \"main.gu\"\n\n[python]\nversion = \"3.14.8\"\ndependencies = [\"requests==2.32.5\"]\n",
    )
    .unwrap();
    fs::write(base.join("main.gu"), "fn main() -> Int:\n    return 0\n").unwrap();
    let old_uv_lock = "version = 1\npackage = \"previous working dependency graph\"\n";
    let old_guion_lock = "format_version = 1\nmanifest_sha256 = \"previous manifest\"\n";
    fs::write(bridge.join("uv.lock"), old_uv_lock).unwrap();
    fs::write(base.join("guion.lock"), old_guion_lock).unwrap();
    fs::write(bridge.join(".venv/bin/keep-me"), "previous environment\n").unwrap();
    let fake_uv = base.join("fake-uv");
    fs::write(
        &fake_uv,
        "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then printf 'uv 0.13.4\\n'; exit 0; fi\nif [ \"$1\" = \"lock\" ]; then shift; while [ \"$#\" -gt 0 ]; do if [ \"$1\" = \"--project\" ]; then shift; printf 'version = 1\\npackage = \\\"failed update candidate\\\"\\n' > \"$1/uv.lock\"; exit 0; fi; shift; done; fi\nif [ \"$1\" = \"sync\" ]; then printf 'simulated dependency installation failure\\n' >&2; exit 17; fi\nexit 99\n",
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = fs::metadata(&fake_uv).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&fake_uv, permissions).unwrap();

    let report = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["sync", "--update", "--json"])
        .arg(&base)
        .env("GUION_UV", &fake_uv)
        .output()
        .unwrap();
    assert_eq!(report.status.code(), Some(1));
    let report: serde_json::Value = serde_json::from_slice(&report.stdout).unwrap();
    assert_eq!(report["diagnostic"]["code"], "E_DEPENDENCY_INSTALL_FAILED");
    assert_eq!(
        fs::read_to_string(bridge.join("uv.lock")).unwrap(),
        old_uv_lock
    );
    assert_eq!(
        fs::read_to_string(base.join("guion.lock")).unwrap(),
        old_guion_lock
    );
    assert_eq!(
        fs::read_to_string(bridge.join(".venv/bin/keep-me")).unwrap(),
        "previous environment\n"
    );
    let _ = fs::remove_dir_all(base);
}

#[cfg(unix)]
#[test]
fn successful_dependency_update_commits_new_environment_and_matching_locks() {
    let base = std::env::temp_dir().join(format!(
        "guion-sync-update-commit-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let bridge = base.join(".guion/python-bridge");
    fs::create_dir_all(bridge.join(".venv/bin")).unwrap();
    fs::write(
        base.join("guion.toml"),
        "[project]\nname = \"sync-commit\"\nversion = \"0.1.0\"\nentry = \"main.gu\"\n\n[python]\nversion = \"3.14.8\"\ndependencies = [\"requests==2.32.5\"]\n",
    )
    .unwrap();
    fs::write(base.join("main.gu"), "fn main() -> Int:\n    return 0\n").unwrap();
    fs::write(bridge.join("uv.lock"), "package = \"old\"\n").unwrap();
    fs::write(base.join("guion.lock"), "manifest_sha256 = \"old\"\n").unwrap();
    fs::write(bridge.join(".venv/bin/old-python"), "old environment\n").unwrap();
    let fake_uv = base.join("fake-uv");
    fs::write(
        &fake_uv,
        r##"#!/bin/sh
if [ "$1" = "--version" ]; then printf 'uv 0.13.4\n'; exit 0; fi
project=''
for arg in "$@"; do if [ "$previous" = "--project" ]; then project="$arg"; fi; previous="$arg"; done
if [ "$1" = "lock" ]; then printf 'version = 1\npackage = \"updated dependency graph\"\n' > "$project/uv.lock"; exit 0; fi
if [ "$1" = "sync" ]; then
  mkdir -p "$project/.venv/bin"
  cat > "$project/.venv/bin/python3" <<'PYTHON'
#!/bin/sh
printf '%s\n' 'Python 3.14.8'
PYTHON
  chmod 755 "$project/.venv/bin/python3"
  exit 0
fi
exit 99
"##,
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = fs::metadata(&fake_uv).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&fake_uv, permissions).unwrap();

    let report = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["sync", "--update", "--json"])
        .arg(&base)
        .env("GUION_UV", &fake_uv)
        .output()
        .unwrap();
    assert!(
        report.status.success(),
        "{}",
        String::from_utf8_lossy(&report.stdout)
    );
    let report: serde_json::Value = serde_json::from_slice(&report.stdout).unwrap();
    assert_eq!(report["status"], "succeeded");
    assert_eq!(report["updated"], true);
    let committed_uv_lock = fs::read_to_string(bridge.join("uv.lock")).unwrap();
    assert!(committed_uv_lock.contains("updated dependency graph"));
    assert!(report["lock_sha256"]
        .as_str()
        .is_some_and(|digest| digest.len() == 64));
    assert!(report["bridge_lock_sha256"]
        .as_str()
        .is_some_and(|digest| digest.len() == 64));
    assert!(bridge.join(".venv/bin/python3").is_file());
    assert!(!bridge.join(".venv/bin/old-python").exists());
    let launched = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", "--json"])
        .arg(&base)
        .env("GUION_UV", &fake_uv)
        .output()
        .unwrap();
    assert!(
        launched.status.success(),
        "{}",
        String::from_utf8_lossy(&launched.stdout)
    );
    let run_report: serde_json::Value = serde_json::from_slice(&launched.stdout).unwrap();
    assert_eq!(run_report["status"], "passed");
    let _ = fs::remove_dir_all(base);
}

#[test]
fn doctor_json_reports_native_and_python_interop_readiness() {
    let missing_uv =
        std::env::temp_dir().join(format!("guion-doctor-missing-uv-{}", std::process::id()));
    let report = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["doctor", "--json"])
        .env("GUION_UV", &missing_uv)
        .env_remove("GUION_PYTHON")
        .output()
        .unwrap();
    assert!(
        report.status.success(),
        "{}",
        String::from_utf8_lossy(&report.stderr)
    );
    assert!(report.stderr.is_empty());
    let report: serde_json::Value = serde_json::from_slice(&report.stdout).unwrap();
    assert_eq!(report["kind"], "guion_doctor_report");
    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["runtime"]["status"], "available");
    assert_eq!(report["capabilities"]["pure_guion"], true);
    assert_eq!(report["capabilities"]["python_calls"], false);
    assert_eq!(report["python"]["sidecar_mode"], "not_configured");
    assert_eq!(report["managed_toolchain"]["status"], "unavailable");
    assert_eq!(
        report["managed_toolchain"]["diagnostic"]["code"],
        "E_UV_NOT_FOUND"
    );

    let explicit_python = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["doctor", "--json"])
        .env("GUION_UV", &missing_uv)
        .env("GUION_PYTHON", "/opt/custom/python3")
        .output()
        .unwrap();
    assert!(explicit_python.status.success());
    let report: serde_json::Value = serde_json::from_slice(&explicit_python.stdout).unwrap();
    assert_eq!(report["capabilities"]["python_calls"], true);
    assert_eq!(report["python"]["sidecar_mode"], "explicit");
    assert_eq!(report["python"]["interpreter"], "/opt/custom/python3");
}

#[cfg(target_os = "macos")]
#[test]
fn run_json_reports_python_native_images_loaded_dynamically() {
    let base = std::env::temp_dir().join(format!(
        "guion-run-native-images-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&base).unwrap();
    let source_c = base.join("probe.c");
    let library = base.join("libguion_cli_probe.dylib");
    fs::write(&source_c, "int guion_probe(void) { return 7; }\n").unwrap();
    let status = Command::new("cc")
        .args(["-dynamiclib", "-o"])
        .arg(&library)
        .arg(&source_c)
        .status()
        .unwrap();
    assert!(status.success());
    let python_module = base.join("native_loader.py");
    fs::write(
        &python_module,
        "import ctypes\ndef load(path):\n    return ctypes.CDLL(path).guion_probe()\n",
    )
    .unwrap();
    let source = base.join("main.gu");
    fs::write(
        &source,
        "extern python \"native_loader:load\" fn load_native(path: Text) -> Int\n\nfn main() -> Int:\n    return load_native(args()[0])\n",
    )
    .unwrap();
    let report = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", "--json"])
        .arg(&source)
        .arg(&library)
        .env("GUION_PYTHON", python3_path())
        .current_dir(&base)
        .output()
        .unwrap();
    assert_eq!(
        report.status.code(),
        Some(7),
        "{}",
        String::from_utf8_lossy(&report.stdout)
    );
    assert!(
        report.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&report.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&report.stdout).unwrap();
    assert_eq!(report["kind"], "guion_run_report");
    assert_eq!(
        report["python_native_image_observation"],
        "macos_dyld_snapshot"
    );
    let library = fs::canonicalize(library).unwrap();
    assert!(report["python_native_images"].as_array().unwrap().contains(
        &serde_json::Value::String(library.to_string_lossy().into_owned())
    ));
    let _ = fs::remove_dir_all(base);
}

#[cfg(target_os = "macos")]
#[test]
fn run_json_keeps_python_native_images_observed_before_a_call_failure() {
    let base = std::env::temp_dir().join(format!(
        "guion-run-native-image-failure-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&base).unwrap();
    let source_c = base.join("probe.c");
    let library = base.join("libguion_cli_failure_probe.dylib");
    fs::write(&source_c, "int guion_probe(void) { return 7; }\n").unwrap();
    let status = Command::new("cc")
        .args(["-dynamiclib", "-o"])
        .arg(&library)
        .arg(&source_c)
        .status()
        .unwrap();
    assert!(status.success());
    fs::write(
        base.join("native_loader.py"),
        "import ctypes\ndef load_then_fail(path):\n    ctypes.CDLL(path)\n    raise RuntimeError('failure after load')\n",
    )
    .unwrap();
    let source = base.join("main.gu");
    fs::write(
        &source,
        "extern python \"native_loader:load_then_fail\" fn load_native(path: Text) -> Int\n\nfn main() -> Int:\n    return load_native(args()[0])\n",
    )
    .unwrap();
    let report = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", "--json"])
        .arg(&source)
        .arg(&library)
        .env("GUION_PYTHON", python3_path())
        .current_dir(&base)
        .output()
        .unwrap();
    assert_eq!(report.status.code(), Some(1));
    assert!(
        report.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&report.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&report.stdout).unwrap();
    assert_eq!(report["status"], "failed");
    assert_eq!(report["diagnostic"]["code"], "R_PYTHON_CALL");
    assert_eq!(
        report["python_native_image_observation"],
        "macos_dyld_snapshot"
    );
    let library = fs::canonicalize(library).unwrap();
    assert!(report["python_native_images"].as_array().unwrap().contains(
        &serde_json::Value::String(library.to_string_lossy().into_owned())
    ));
    let _ = fs::remove_dir_all(base);
}

#[test]
fn package_json_reports_package_identity_verification_and_argument_failures() {
    let base = std::env::temp_dir().join(format!("guion-json-package-{}", std::process::id()));
    let project = base.join("project with spaces");
    let package = base.join("output app");
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(&project).unwrap();
    fs::write(
        project.join("guion.toml"),
        "[project]\nname = \"json-package-test\"\nversion = \"0.1.0\"\nentry = \"main.gu\"\n",
    )
    .unwrap();
    fs::write(
        project.join("main.gu"),
        "fn main() -> Int:\n    print(\"packaged\")\n    return 0\n",
    )
    .unwrap();

    let packaged = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args([
            "package",
            "--json",
            project.to_str().unwrap(),
            "--output",
            package.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        packaged.status.success(),
        "{}",
        String::from_utf8_lossy(&packaged.stderr)
    );
    assert!(packaged.stderr.is_empty());
    let report: serde_json::Value = serde_json::from_slice(&packaged.stdout).unwrap();
    assert_eq!(report["kind"], "guion_package_report");
    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["directory"], package.to_string_lossy().as_ref());
    assert_eq!(report["package_schema_version"], 2);
    assert_eq!(
        report["artifact_sha256"],
        serde_json::from_slice::<serde_json::Value>(
            &fs::read(package.join("PACKAGE.json")).unwrap()
        )
        .unwrap()["artifact_sha256"]
    );
    assert_eq!(
        report["package_metadata_sha256"],
        format!(
            "{:x}",
            Sha256::digest(fs::read(package.join("PACKAGE.json")).unwrap())
        )
    );

    let verified = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["verify-package", "--json", package.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        verified.status.success(),
        "{}",
        String::from_utf8_lossy(&verified.stderr)
    );
    assert!(verified.stderr.is_empty());
    let verification: serde_json::Value = serde_json::from_slice(&verified.stdout).unwrap();
    assert_eq!(verification["kind"], "guion_package_verification_report");
    assert_eq!(verification["status"], "verified");

    fs::write(package.join("app.gub"), b"tampered artifact").unwrap();
    let rejected = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["verify-package", "--json", package.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(rejected.status.code(), Some(1));
    assert!(rejected.stderr.is_empty());
    let diagnostic: serde_json::Value = serde_json::from_slice(&rejected.stdout).unwrap();
    assert_eq!(diagnostic["diagnostic"]["code"], "E_PACKAGE_VERIFY_FAILED");

    let existing_output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args([
            "package",
            "--json",
            project.to_str().unwrap(),
            "--output",
            package.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(existing_output.status.code(), Some(1));
    assert!(existing_output.stderr.is_empty());
    let diagnostic: serde_json::Value = serde_json::from_slice(&existing_output.stdout).unwrap();
    assert_eq!(diagnostic["diagnostic"]["code"], "E_PACKAGE_OUTPUT_EXISTS");

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;

        let dangling = base.join("dangling package link");
        symlink(base.join("not created"), &dangling).unwrap();
        let rejected = Command::new(env!("CARGO_BIN_EXE_guion"))
            .args([
                "package",
                "--json",
                project.to_str().unwrap(),
                "--output",
                dangling.to_str().unwrap(),
            ])
            .output()
            .unwrap();
        assert_eq!(rejected.status.code(), Some(1));
        assert!(rejected.stderr.is_empty());
        let diagnostic: serde_json::Value = serde_json::from_slice(&rejected.stdout).unwrap();
        assert_eq!(diagnostic["diagnostic"]["code"], "E_PACKAGE_OUTPUT_EXISTS");
        assert!(fs::symlink_metadata(&dangling)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(fs::read_link(&dangling).unwrap(), base.join("not created"));
    }

    let unicode_package = base.join("deliveries Ω / Guion café");
    let unicode_build = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args([
            "package",
            "--json",
            project.to_str().unwrap(),
            "--output",
            unicode_package.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        unicode_build.status.success(),
        "{}",
        String::from_utf8_lossy(&unicode_build.stderr)
    );
    let unicode_verified = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args([
            "verify-package",
            "--json",
            unicode_package.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        unicode_verified.status.success(),
        "{}",
        String::from_utf8_lossy(&unicode_verified.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&unicode_verified.stdout).unwrap()["status"],
        "verified"
    );
    let unicode_launch = Command::new(unicode_package.join("run"))
        .current_dir(&base)
        .output()
        .unwrap();
    assert!(
        unicode_launch.status.success(),
        "{}",
        String::from_utf8_lossy(&unicode_launch.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&unicode_launch.stdout),
        "packaged\n"
    );

    let missing = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["package", "--json", base.join("missing").to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(missing.status.code(), Some(1));
    assert!(missing.stderr.is_empty());
    let diagnostic: serde_json::Value = serde_json::from_slice(&missing.stdout).unwrap();
    assert_eq!(diagnostic["kind"], "guion_diagnostic");
    assert_eq!(diagnostic["diagnostic"]["code"], "E_PROJECT_NOT_FOUND");

    let broken_project = base.join("broken project");
    fs::create_dir_all(&broken_project).unwrap();
    fs::write(
        broken_project.join("guion.toml"),
        "[project]\nname = \"broken-json-package\"\nversion = \"0.1.0\"\nentry = \"main.gu\"\n",
    )
    .unwrap();
    fs::write(
        broken_project.join("main.gu"),
        "fn main() -> Int:\n    return missing_name\n",
    )
    .unwrap();
    let broken = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args([
            "package",
            "--json",
            broken_project.to_str().unwrap(),
            "--output",
            base.join("broken package").to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(broken.status.code(), Some(1));
    assert!(broken.stderr.is_empty());
    let diagnostic: serde_json::Value = serde_json::from_slice(&broken.stdout).unwrap();
    assert_eq!(diagnostic["diagnostic"]["code"], "E_NAME_UNKNOWN");
    assert_eq!(diagnostic["diagnostic"]["line"], 2);
    let _ = fs::remove_dir_all(base);
}

#[test]
fn package_builds_a_relocatable_app_with_declared_resources() {
    let base = std::env::temp_dir().join(format!("guion-package-{}", std::process::id()));
    let project = base.join("project");
    let output_dir = base.join("out");
    let package = output_dir.join("greeting-app");
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(project.join("src")).unwrap();
    fs::create_dir_all(project.join("assets")).unwrap();
    fs::write(
        project.join("guion.toml"),
        "[project]\nname = \"greeting-app\"\nversion = \"0.1.0\"\nentry = \"src/main.gu\"\n\n[package]\nresources = [\"assets/greeting.txt\", \"assets/unused.txt\"]\n",
    )
    .unwrap();
    fs::write(
        project.join("src/main.gu"),
        "fn main() -> Int:\n    print(unwrap(read_text(\"assets/greeting.txt\")))\n    return 0\n",
    )
    .unwrap();
    fs::write(
        project.join("assets/greeting.txt"),
        "Hello from the package!\n",
    )
    .unwrap();
    fs::write(project.join("assets/unused.txt"), "Unused package data.\n").unwrap();

    let packaged = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args([
            "package",
            project.to_str().unwrap(),
            "--output",
            package.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        packaged.status.success(),
        "{}",
        String::from_utf8_lossy(&packaged.stderr)
    );
    assert_eq!(
        fs::read_to_string(package.join("assets/greeting.txt")).unwrap(),
        "Hello from the package!\n"
    );
    assert!(package.join("app.gub").is_file());
    assert!(package.join("guion").is_file());
    let package_report: serde_json::Value =
        serde_json::from_slice(&fs::read(package.join("PACKAGE.json")).unwrap()).unwrap();
    let digest = |bytes: &[u8]| format!("{:x}", Sha256::digest(bytes));
    assert_eq!(
        package_report["target"],
        format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
    );
    assert_eq!(
        package_report["artifact_sha256"],
        digest(&fs::read(package.join("app.gub")).unwrap())
    );
    assert_eq!(
        package_report["runtime_sha256"],
        digest(&fs::read(package.join("guion")).unwrap())
    );
    assert_eq!(
        package_report["resources"][0]["path"],
        "assets/greeting.txt"
    );
    assert_eq!(
        package_report["resources"][0]["sha256"],
        digest(&fs::read(package.join("assets/greeting.txt")).unwrap())
    );
    let verified = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["verify-package", package.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        verified.status.success(),
        "{}",
        String::from_utf8_lossy(&verified.stderr)
    );

    fs::remove_dir_all(&project).unwrap();
    let launched = Command::new(package.join("run"))
        .current_dir(&base)
        .output()
        .unwrap();
    assert!(
        launched.status.success(),
        "{}",
        String::from_utf8_lossy(&launched.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&launched.stdout),
        "Hello from the package!\n\n"
    );
    fs::write(package.join("assets/unused.txt"), "corrupted\n").unwrap();
    let fast_launch = Command::new(package.join("run"))
        .current_dir(&base)
        .output()
        .unwrap();
    assert!(
        fast_launch.status.success(),
        "{}",
        String::from_utf8_lossy(&fast_launch.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&fast_launch.stdout),
        "Hello from the package!\n\n"
    );
    let rejected = Command::new(package.join("run"))
        .env("GUION_VERIFY_PACKAGE", "1")
        .current_dir(&base)
        .output()
        .unwrap();
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("resource integrity check failed"));
    assert!(String::from_utf8_lossy(&rejected.stdout).is_empty());
    let _ = fs::remove_dir_all(base);
}

#[test]
fn packaged_cli_can_read_inputs_from_its_invocation_directory() {
    let base =
        std::env::temp_dir().join(format!("guion-package-input-root-{}", std::process::id()));
    let project = base.join("project");
    let package = base.join("bundle");
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(&project).unwrap();
    fs::write(
        project.join("guion.toml"),
        "[project]\nname = \"input-root\"\nversion = \"0.1.0\"\nentry = \"main.gu\"\n",
    )
    .unwrap();
    fs::write(
        project.join("main.gu"),
        "fn main() -> Int:\n    let values = args()\n    print(unwrap(read_text(values[0])))\n    unwrap(write_text(values[1], \"saved output\\n\"))\n    return 0\n",
    )
    .unwrap();
    fs::write(base.join("input.txt"), "caller data\n").unwrap();

    let packaged = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args([
            "package",
            project.to_str().unwrap(),
            "--output",
            package.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        packaged.status.success(),
        "{}{}",
        String::from_utf8_lossy(&packaged.stdout),
        String::from_utf8_lossy(&packaged.stderr)
    );

    let launched = Command::new(package.join("run"))
        .current_dir(&base)
        .args(["input.txt", "output.txt"])
        .output()
        .unwrap();
    assert!(
        launched.status.success(),
        "{}{}",
        String::from_utf8_lossy(&launched.stdout),
        String::from_utf8_lossy(&launched.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&launched.stdout), "caller data\n\n");
    assert_eq!(
        fs::read_to_string(base.join("output.txt")).unwrap(),
        "saved output\n"
    );
    let _ = fs::remove_dir_all(base);
}

#[test]
fn package_rejects_python_calls_instead_of_emitting_a_broken_bundle() {
    let base = std::env::temp_dir().join(format!("guion-package-python-{}", std::process::id()));
    let project = base.join("project");
    let output = base.join("bundle");
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(&project).unwrap();
    fs::write(
        project.join("guion.toml"),
        "[project]\nname = \"python-app\"\nversion = \"0.1.0\"\nentry = \"main.gu\"\n",
    )
    .unwrap();
    fs::write(
        project.join("main.gu"),
        "extern python \"math:sqrt\" fn square_root(value: Float64) -> Float64\n\nfn main() -> Int:\n    return 0\n",
    )
    .unwrap();

    let packaged = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args([
            "package",
            project.to_str().unwrap(),
            "--output",
            output.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!packaged.status.success());
    assert!(
        String::from_utf8_lossy(&packaged.stderr).contains("no managed [python] project to bundle")
    );
    assert!(!output.exists());
    let _ = fs::remove_dir_all(base);
}

#[test]
fn package_rejects_resources_that_collide_with_runtime_files() {
    let base = std::env::temp_dir().join(format!("guion-package-collision-{}", std::process::id()));
    let project = base.join("project");
    let output = base.join("bundle");
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(&project).unwrap();
    fs::write(
        project.join("guion.toml"),
        "[project]\nname = \"collision-app\"\nversion = \"0.1.0\"\nentry = \"main.gu\"\n\n[package]\nresources = [\"app.gub\"]\n",
    )
    .unwrap();
    fs::write(project.join("main.gu"), "fn main() -> Int:\n    return 0\n").unwrap();
    fs::write(
        project.join("app.gub"),
        "this must not overwrite the compiled artifact",
    )
    .unwrap();

    let packaged = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args([
            "package",
            project.to_str().unwrap(),
            "--output",
            output.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!packaged.status.success());
    assert!(String::from_utf8_lossy(&packaged.stderr).contains("reserved by the package runtime"));
    assert!(!output.exists());
    let _ = fs::remove_dir_all(base);
}

#[test]
fn package_bundles_python_runtime_and_dependencies_for_offline_relocated_run() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let source_project = root.join("../../evaluation/development/feed_inbox");
    let base = std::env::temp_dir().join(format!(
        "guion package 起点 python run-{}",
        std::process::id()
    ));
    let package = base.join("initial");
    let project = base.join("feed-inbox-source");
    let relocated = base.join("moved").join("feed-inbox");
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(&base).unwrap();
    let uv = std::env::var("GUION_UV")
        .expect("set GUION_UV to the pinned uv executable for this package test");
    copy_project_without_managed_environment(&source_project, &project);
    let synced = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["sync", "--update"])
        .arg(&project)
        .env("GUION_UV", &uv)
        .output()
        .unwrap();
    assert!(
        synced.status.success(),
        "{}{}",
        String::from_utf8_lossy(&synced.stdout),
        String::from_utf8_lossy(&synced.stderr)
    );

    let packaged = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["package", project.to_str().unwrap(), "--output", "initial"])
        .current_dir(&base)
        .env("GUION_UV", &uv)
        .output()
        .unwrap();
    assert!(
        packaged.status.success(),
        "{}",
        String::from_utf8_lossy(&packaged.stderr)
    );
    assert!(package.join("python").is_dir());
    assert!(package.join("toolchains/uv").is_file());
    assert!(package.join(".uv-cache").is_dir());
    assert!(package.join(".guion/python-bridge/uv.lock").is_file());
    assert!(package.join("examples/sample-feed.xml").is_file());
    assert!(package.join("feed_inbox.py").is_file());
    assert!(!package.join("main.gu").exists());
    let package_metadata: serde_json::Value =
        serde_json::from_slice(&fs::read(package.join("PACKAGE.json")).unwrap()).unwrap();
    assert_eq!(package_metadata["schema_version"], 2);
    assert!(package_metadata["python_runtime"]["python_runtime_sha256"]
        .as_str()
        .is_some_and(|digest| digest.len() == 64));
    assert!(
        package_metadata["python_runtime"]["python_runtime_size_bytes"]
            .as_u64()
            .is_some_and(|size| size > 0)
    );
    assert!(package_metadata["python_runtime"]["python_cache_sha256"]
        .as_str()
        .is_some_and(|digest| digest.len() == 64));
    assert!(
        package_metadata["python_runtime"]["python_cache_size_bytes"]
            .as_u64()
            .is_some_and(|size| size > 0)
    );
    for entry in fs::read_dir(package.join("python")).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_symlink() {
            assert!(
                !fs::read_link(entry.path()).unwrap().is_absolute(),
                "Python runtime link must be relocatable: {}",
                entry.path().display()
            );
        }
    }

    fs::create_dir_all(relocated.parent().unwrap()).unwrap();
    fs::rename(&package, &relocated).unwrap();
    let launched = Command::new(relocated.join("run"))
        .args(["import-feed", "examples/sample-feed.xml"])
        .current_dir(&base)
        .env_remove("UV_PYTHON_INSTALL_DIR")
        .env_remove("UV_CACHE_DIR")
        .env_remove("UV_OFFLINE")
        .output()
        .unwrap();
    assert!(
        launched.status.success(),
        "{}",
        String::from_utf8_lossy(&launched.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&launched.stdout),
        "New items:\n2\nTotal items:\n2\n"
    );
    assert_eq!(
        fs::read_to_string(base.join("inbox.json"))
            .unwrap()
            .matches("https://example.test/")
            .count(),
        2
    );
    assert!(!relocated.join("inbox.json").exists());
    let repeated = Command::new(relocated.join("run"))
        .arg("summary")
        .current_dir(&base)
        .env_remove("GUION_UV")
        .env_remove("UV_PYTHON_INSTALL_DIR")
        .env_remove("UV_CACHE_DIR")
        .env_remove("UV_OFFLINE")
        .output()
        .unwrap();
    assert!(
        repeated.status.success(),
        "{}",
        String::from_utf8_lossy(&repeated.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&repeated.stdout),
        "Items:\n2\nUnread:\n2\nRead:\n0\n"
    );
    let _ = fs::remove_dir_all(base);
}

#[test]
fn package_runs_compiled_python_wheels_after_relocation_without_python_setup() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let source_project = root.join("../../evaluation/development/python_interop");
    let base =
        std::env::temp_dir().join(format!("guion native package 起点-{}", std::process::id()));
    let package = base.join("first package/python interop");
    let project = base.join("python-interop-source");
    let relocated = base.join("relocated apps/python interop");
    let _ = fs::remove_dir_all(&base);
    let uv = std::env::var("GUION_UV")
        .expect("set GUION_UV to the pinned uv executable for this package test");
    fs::create_dir_all(&base).unwrap();
    copy_project_without_managed_environment(&source_project, &project);
    let synced = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["sync", "--update"])
        .arg(&project)
        .env("GUION_UV", &uv)
        .output()
        .unwrap();
    assert!(
        synced.status.success(),
        "{}{}",
        String::from_utf8_lossy(&synced.stdout),
        String::from_utf8_lossy(&synced.stderr)
    );

    let packaged = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args([
            "package",
            project.to_str().unwrap(),
            "--output",
            package.to_str().unwrap(),
        ])
        .env("GUION_UV", &uv)
        .output()
        .unwrap();
    assert!(
        packaged.status.success(),
        "{}",
        String::from_utf8_lossy(&packaged.stderr)
    );
    assert!(!package.join("main.gu").exists());
    assert!(package.join("toolchains/uv").is_file());
    assert!(package.join(".guion/python-bridge/uv.lock").is_file());
    let metadata: serde_json::Value =
        serde_json::from_slice(&fs::read(package.join("PACKAGE.json")).unwrap()).unwrap();
    assert_eq!(metadata["python_runtime"]["version"], "3.14.8");
    assert!(metadata["python_runtime"]["python_runtime_size_bytes"]
        .as_u64()
        .is_some_and(|size| size > 0));
    let dependency_report: serde_json::Value =
        serde_json::from_slice(&fs::read(package.join("PYTHON-DEPENDENCIES.json")).unwrap())
            .unwrap();
    let packages = dependency_report["packages"].as_array().unwrap();
    assert!(packages.iter().any(|record| {
        record["name"] == "pip"
            && record["environment"] == "managed-interpreter"
            && record["license_status"] == "complete"
            && !record["license_files"].as_array().unwrap().is_empty()
    }));
    for required in ["numpy", "orjson", "requests"] {
        let record = packages
            .iter()
            .find(|record| record["name"] == required)
            .unwrap_or_else(|| panic!("dependency report is missing {required}"));
        assert_eq!(record["license_status"], "complete");
        assert!(!record["license_files"].as_array().unwrap().is_empty());
    }
    let runtime_components = dependency_report["runtime_components"].as_array().unwrap();
    for required in ["CPython", "uv", "Tcl/Tk"] {
        let component = runtime_components
            .iter()
            .find(|record| record["name"] == required)
            .unwrap_or_else(|| panic!("dependency report is missing runtime component {required}"));
        assert!(!component["license_files"].as_array().unwrap().is_empty());
    }
    let native_report: serde_json::Value =
        serde_json::from_slice(&fs::read(package.join("NATIVE-DEPENDENCIES.json")).unwrap())
            .unwrap();
    let cached_native_images = native_report["cached_images"]
        .as_array()
        .expect("native report should enumerate images in offline wheel-cache trees");
    let cached_numpy = cached_native_images
        .iter()
        .find(|image| {
            image["path"]
                .as_str()
                .is_some_and(|path| path.contains(".uv-cache/archive-v") && path.contains("numpy"))
        })
        .expect("offline wheel cache should record NumPy native images");
    assert_eq!(cached_numpy["owner"]["kind"], "python_distribution");
    assert_eq!(cached_numpy["owner"]["name"], "numpy");
    assert!(cached_numpy["owner"]["record"]
        .as_str()
        .is_some_and(|path| package.join(path).is_file()));
    assert!(cached_numpy["sha256"].as_str().is_some());
    assert!(!native_report["images"].as_array().unwrap().is_empty());
    assert!(native_report["unresolved"].as_array().unwrap().is_empty());
    assert!(native_report["unattributed_images"]
        .as_array()
        .unwrap()
        .is_empty());
    let native_python_images = native_report["images"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|image| image["path"].as_str().unwrap().contains("site-packages"))
        .collect::<Vec<_>>();
    assert!(!native_python_images.is_empty());
    for image in native_python_images {
        assert_eq!(image["owner"]["kind"], "python_distribution");
        assert!(!image["owner"]["name"]
            .as_str()
            .unwrap_or_default()
            .is_empty());
        assert!(!image["owner"]["license_files"]
            .as_array()
            .unwrap()
            .is_empty());
        let owner = &image["owner"];
        let distribution = packages
            .iter()
            .find(|package| {
                package["name"] == owner["name"]
                    && package["version"] == owner["version"]
                    && package["environment"] == owner["environment"]
            })
            .expect("native owner must exist in Python dependency inventory");
        assert_eq!(owner["license_files"], distribution["license_files"]);
    }
    let bundled_libraries = native_report["bundled_libraries"].as_array().unwrap();
    assert!(bundled_libraries.iter().any(|path| path
        .as_str()
        .unwrap()
        .to_ascii_lowercase()
        .contains("libtcl")));
    assert!(bundled_libraries.iter().any(|path| {
        let path = path.as_str().unwrap().to_ascii_lowercase();
        path.contains("libtk") || path.contains("libtcl9tk")
    }));
    let native_image = native_report["images"]
        .as_array()
        .unwrap()
        .iter()
        .find(|image| image["path"].as_str().unwrap().contains("orjson"))
        .unwrap()["path"]
        .as_str()
        .unwrap()
        .to_owned();
    let license_relative = packages
        .iter()
        .find(|record| record["name"] == "orjson")
        .unwrap()["license_files"][0]["path"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(package.join(&license_relative).is_file());

    fs::create_dir_all(relocated.parent().unwrap()).unwrap();
    fs::rename(&package, &relocated).unwrap();
    for _ in 0..2 {
        let launched = Command::new(relocated.join("run"))
            .current_dir(&base)
            .env_remove("GUION_UV")
            .env_remove("GUION_PYTHON")
            .env_remove("UV_PYTHON_INSTALL_DIR")
            .env_remove("UV_CACHE_DIR")
            .env_remove("UV_OFFLINE")
            .output()
            .unwrap();
        assert!(
            launched.status.success(),
            "{}",
            String::from_utf8_lossy(&launched.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&launched.stdout),
            "15\n9007199254741000\n3\npython-requests/2.32.5\n"
        );
    }
    let original_license = fs::read(relocated.join(&license_relative)).unwrap();
    fs::write(relocated.join(&license_relative), "tampered license\n").unwrap();
    let rejected_license = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["verify-package", relocated.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!rejected_license.status.success());
    assert!(String::from_utf8_lossy(&rejected_license.stderr)
        .contains("Python dependency license integrity check failed"));
    fs::write(relocated.join(&license_relative), original_license).unwrap();
    let original_native = fs::read(relocated.join(&native_image)).unwrap();
    fs::write(relocated.join(&native_image), b"tampered native image\n").unwrap();
    let rejected_native = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["verify-package", relocated.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!rejected_native.status.success());
    assert!(String::from_utf8_lossy(&rejected_native.stderr)
        .contains("native image integrity check failed"));
    fs::write(relocated.join(&native_image), original_native).unwrap();
    let native_report_path = relocated.join("NATIVE-DEPENDENCIES.json");
    let package_metadata_path = relocated.join("PACKAGE.json");
    let original_native_report = fs::read(&native_report_path).unwrap();
    let original_package_metadata = fs::read(&package_metadata_path).unwrap();
    let mut modified_report: serde_json::Value =
        serde_json::from_slice(&original_native_report).unwrap();
    let owner_image = modified_report["images"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|image| image["path"].as_str().unwrap().contains("site-packages"))
        .unwrap();
    owner_image["owner"]["name"] = serde_json::Value::String("forged-owner".into());
    let modified_report_bytes = serde_json::to_vec_pretty(&modified_report).unwrap();
    fs::write(&native_report_path, &modified_report_bytes).unwrap();
    let mut modified_metadata: serde_json::Value =
        serde_json::from_slice(&original_package_metadata).unwrap();
    modified_metadata["native_dependency_report_sha256"] =
        serde_json::Value::String(format!("{:x}", Sha256::digest(&modified_report_bytes)));
    fs::write(
        &package_metadata_path,
        serde_json::to_vec_pretty(&modified_metadata).unwrap(),
    )
    .unwrap();
    let rejected_owner = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["verify-package", relocated.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!rejected_owner.status.success());
    assert!(String::from_utf8_lossy(&rejected_owner.stderr).contains("owner forged-owner"));
    fs::write(&native_report_path, original_native_report).unwrap();
    fs::write(&package_metadata_path, original_package_metadata).unwrap();
    let python_gitignore = relocated.join("python/.gitignore");
    let original_python_gitignore = fs::read(&python_gitignore).ok();
    fs::write(&python_gitignore, "tampered runtime tree\n").unwrap();
    let rejected = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["verify-package", relocated.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!rejected.status.success());
    assert!(
        String::from_utf8_lossy(&rejected.stderr).contains("Python runtime integrity check failed")
    );
    if let Some(contents) = original_python_gitignore {
        fs::write(&python_gitignore, contents).unwrap();
    } else {
        fs::remove_file(&python_gitignore).unwrap();
    }
    fs::write(
        relocated.join(".uv-cache/tampered-cache-entry"),
        "tampered cache\n",
    )
    .unwrap();
    let rejected_cache = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["verify-package", relocated.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!rejected_cache.status.success());
    assert!(String::from_utf8_lossy(&rejected_cache.stderr)
        .contains("offline Python cache integrity check failed"));
    let _ = fs::remove_dir_all(base);
}

#[cfg(unix)]
#[test]
fn package_launcher_uses_and_cleans_a_temporary_uv_cache_when_bundle_is_read_only() {
    use std::os::unix::fs::PermissionsExt;

    let base = std::env::temp_dir().join(format!(
        "guion readonly package cache-{}",
        std::process::id()
    ));
    let project = base.join("project");
    let package = base.join("package");
    let tmp = base.join("tmp");
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(&project).unwrap();
    fs::create_dir_all(&tmp).unwrap();
    fs::write(
        project.join("guion.toml"),
        "[project]\nname = \"readonly-package\"\nversion = \"0.1.0\"\nentry = \"main.gu\"\n",
    )
    .unwrap();
    fs::write(project.join("main.gu"), "fn main() -> Int:\n    return 0\n").unwrap();

    let packaged = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args([
            "package",
            project.to_str().unwrap(),
            "--output",
            package.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        packaged.status.success(),
        "{}{}",
        String::from_utf8_lossy(&packaged.stdout),
        String::from_utf8_lossy(&packaged.stderr)
    );

    let toolchains = package.join("toolchains");
    fs::create_dir_all(&toolchains).unwrap();
    let fake_uv = toolchains.join("uv");
    fs::write(&fake_uv, "#!/bin/sh\nexit 0\n").unwrap();
    fs::set_permissions(&fake_uv, fs::Permissions::from_mode(0o555)).unwrap();
    fs::create_dir_all(package.join(".uv-cache")).unwrap();
    fs::write(
        package.join("guion"),
        "#!/bin/sh\nset -eu\nprintf '%s\\n' \"${UV_CACHE_DIR:?}\"\ntouch \"$UV_CACHE_DIR/write-probe\"\n",
    )
    .unwrap();
    fs::set_permissions(package.join("guion"), fs::Permissions::from_mode(0o555)).unwrap();
    fs::set_permissions(package.join(".uv-cache"), fs::Permissions::from_mode(0o555)).unwrap();
    fs::set_permissions(&package, fs::Permissions::from_mode(0o555)).unwrap();

    let launched = Command::new(package.join("run"))
        .env("TMPDIR", &tmp)
        .output()
        .unwrap();

    fs::set_permissions(&package, fs::Permissions::from_mode(0o755)).unwrap();
    fs::set_permissions(package.join(".uv-cache"), fs::Permissions::from_mode(0o755)).unwrap();

    assert!(
        launched.status.success(),
        "{}",
        String::from_utf8_lossy(&launched.stderr)
    );
    let cache = std::path::PathBuf::from(
        String::from_utf8(launched.stdout)
            .unwrap()
            .trim()
            .to_owned(),
    );
    assert!(
        cache.starts_with(&tmp),
        "cache was not outside the package: {cache:?}"
    );
    assert!(!cache.starts_with(&package));
    assert!(
        !cache.exists(),
        "temporary cache was not cleaned up: {cache:?}"
    );

    let _ = fs::remove_dir_all(&base);
}

#[test]
fn project_network_manifest_grants_only_the_listed_host() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0; 1024];
        let _ = stream.read(&mut request);
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\nfeed")
            .unwrap();
    });
    let dir = std::env::temp_dir().join(format!("guion-network-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("guion.toml"),
        "[project]\nname = \"network-test\"\nversion = \"0.1.0\"\n\n[network]\nhosts = [\"127.0.0.1\"]\ntimeout_ms = 2000\nmax_response_bytes = 64\nmax_redirects = 0\n",
    )
    .unwrap();
    fs::write(
        dir.join("main.gu"),
        format!(
            "fn main() -> Int:\n    let response = http_get(\"http://{address}/\")\n    if is_ok(response):\n        print(unwrap(response))\n        return 0\n    else:\n        let failure = error(response)\n        print(http_error_code(failure))\n        return 1\n"
        ),
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", dir.join("main.gu").to_str().unwrap()])
        .output()
        .unwrap();
    server.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "feed\n");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn run_accepts_an_explicit_one_invocation_http_host_grant() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = Vec::new();
        let mut buffer = [0; 1024];
        loop {
            let read = stream.read(&mut buffer).unwrap();
            if read == 0 {
                break;
            }
            request.extend_from_slice(&buffer[..read]);
            if let Some(body_start) = request.windows(4).position(|window| window == b"\r\n\r\n") {
                let header_end = body_start + 4;
                let headers = String::from_utf8_lossy(&request[..body_start]).to_ascii_lowercase();
                let content_length = headers
                    .lines()
                    .find_map(|line| line.strip_prefix("content-length: "))
                    .and_then(|value| value.parse::<usize>().ok())
                    .unwrap_or(0);
                if request.len() >= header_end + content_length {
                    break;
                }
            }
        }
        let mut response = b"HTTP/1.1 201 Created\r\nContent-Type: application/octet-stream\r\nContent-Length: 4\r\nConnection: close\r\n\r\n".to_vec();
        response.extend_from_slice(&[0, 0xff, 0x80, b'v']);
        stream.write_all(&response).unwrap();
        request
    });
    let dir = std::env::temp_dir().join(format!("guion-cli-http-grant-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("main.gu");
    fs::write(
        &file,
        format!(
            "import \"std:types\"\n\nfn main() -> Int:\n    let response = http_request_bytes(\"POST\", \"http://{address}/feed\", {{\"content-type\": \"application/octet-stream\"}}, bytes_from_ints([0, 255, 128]), 1000)\n    if is_ok(response):\n        let value = unwrap(response)\n        if value.status == 201 and value.body == bytes_from_ints([0, 255, 128, 118]):\n            return 0\n    return 1\n"
        ),
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args([
            "run",
            "--allow-http-host",
            "127.0.0.1",
            file.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let request = server.join().unwrap();
    assert!(request.windows(3).any(|window| window == [0, 0xff, 0x80]));
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn inspect_json_explains_exact_http_grants_for_runtime_urls() {
    let dir = std::env::temp_dir().join(format!("guion-inspect-network-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("main.gu");
    fs::write(
        &file,
        "fn main() -> Int:\n    let result = http_get(\"http://example.invalid/feed\")\n    return 0\n",
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["inspect", "--json", file.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["network_grant"]["required_for_entry"], true);
    assert_eq!(report["network_grant"]["manifest_key"], "[network].hosts");
    assert_eq!(
        report["network_grant"]["run_option"],
        "guion run --allow-http-host HOST"
    );
    assert_eq!(
        report["network_grant"]["runtime_supplied_urls_require_an_explicit_hostname_grant"],
        true
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn clock_grant_is_reported_and_can_be_enabled_by_manifest_or_run_option() {
    let dir = std::env::temp_dir().join(format!("guion-cli-clock-grant-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("main.gu");
    fs::write(
        &file,
        "import \"std:time\"\n\nfn main() -> Int:\n    let current = today_local()\n    if current.year > 1970:\n        return 0\n    else:\n        return 1\n",
    )
    .unwrap();

    let denied = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", file.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!denied.status.success());
    assert!(String::from_utf8_lossy(&denied.stderr).contains("R_CLOCK_DENIED"));

    let granted = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", "--allow-clock", file.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        granted.status.success(),
        "{}",
        String::from_utf8_lossy(&granted.stderr)
    );

    let inspect = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["inspect", "--json", file.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(inspect.status.success());
    let report: serde_json::Value = serde_json::from_slice(&inspect.stdout).unwrap();
    assert_eq!(report["clock_grant"]["required_for_entry"], true);
    assert_eq!(report["clock_grant"]["manifest_key"], "[clock].enabled");
    assert_eq!(
        report["clock_grant"]["run_option"],
        "guion run --allow-clock"
    );

    fs::write(dir.join("guion.toml"), "[clock]\nenabled = true\n").unwrap();
    let manifest_granted = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", file.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        manifest_granted.status.success(),
        "{}",
        String::from_utf8_lossy(&manifest_granted.stderr)
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn run_executes_a_source_file_and_surfaces_source_errors() {
    let dir = std::env::temp_dir().join(format!("guion-cli-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("main.gu");
    fs::write(&file, "fn main() -> Int:\n    print(6 * 7)\n    return 0\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", file.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "42\n");

    fs::write(&file, "fn main() -> Int:\n    return missing\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["check", file.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("E_NAME_UNKNOWN"));
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn inspect_json_reports_transitive_runtime_and_opaque_python_effects() {
    let dir = std::env::temp_dir().join(format!("guion-inspect-effects-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("main.gu");
    fs::write(
        &file,
        "extern python \"math:sqrt\" fn square_root(value: Float64) -> Float64\n\nfn load(path: Text) -> Text:\n    return unwrap(read_text(path))\n\nfn main() -> Int:\n    print(load(\"input.txt\"))\n    let value = square_root(4.0)\n    return 0\n",
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["inspect", "--json", file.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["kind"], "guion_inspection_report");
    assert_eq!(report["schema_version"], 1);
    assert_eq!(
        report["functions"]["main"]["effects"],
        serde_json::json!(["filesystem.read", "output.write", "python.opaque"])
    );
    assert_eq!(
        report["functions"]["load"]["effects"],
        serde_json::json!(["filesystem.read"])
    );
    assert_eq!(report["python_calls"][0]["effect"], "opaque_external");
    assert_eq!(
        report["capability_claim"],
        "static_bytecode_reachability_not_sandbox"
    );
    let human = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["inspect", file.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(human.status.success());
    let human_report = String::from_utf8_lossy(&human.stdout);
    assert!(human_report.contains("capabilities (static reachability; not a sandbox)"));
    assert!(human_report.contains("filesystem.read, output.write, python.opaque"));
    assert!(human_report.contains("Python declarations are opaque external effects"));
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn project_import_compiles_the_module_graph_and_calls_only_public_functions() {
    let dir = std::env::temp_dir().join(format!("guion-modules-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("guion.toml"),
        "[project]\nname = \"module-test\"\nversion = \"0.1.0\"\nentry = \"src/main.gu\"\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/main.gu"),
        "import \"math.gu\"\n\nfn main() -> Int:\n    let value = Offset(3)\n    print(twice(value.amount + 18))\n    return 0\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/math.gu"),
        "import \"helpers.gu\"\n\npub record Offset:\n    amount: Int\n\nfn hidden(value: Int) -> Int:\n    return value + 1\n\npub fn twice(value: Int) -> Int:\n    return scale(value)\n\npub fn divide() -> Int:\n    return 1 / 0\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/helpers.gu"),
        "pub fn scale(value: Int) -> Int:\n    return value * 2\n",
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", dir.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "42\n");

    fs::write(
        dir.join("src/main.gu"),
        "import \"math.gu\"\n\nfn main() -> Int:\n    print(hidden(21))\n    return 0\n",
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["check", dir.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("E_NAME_UNKNOWN"));

    fs::write(
        dir.join("src/main.gu"),
        "import \"math.gu\"\n\nfn main() -> Int:\n    return scale(21)\n",
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["check", dir.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("E_NAME_UNKNOWN"));

    fs::write(
        dir.join("src/main.gu"),
        "import \"math.gu\"\n\nfn main() -> Int:\n    return 0\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/math.gu"),
        "pub fn broken() -> Int:\n    return missing\n",
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["check", dir.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("src/math.gu"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("E_NAME_UNKNOWN"));

    fs::write(
        dir.join("src/main.gu"),
        "import \"math.gu\"\n\nfn main() -> Int:\n    return divide()\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/math.gu"),
        "pub fn divide() -> Int:\n    return 1 / 0\n",
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", dir.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("src/math.gu"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("R_DIV_ZERO"));

    let artifact = dir.join("modules.gub");
    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args([
            "build",
            dir.to_str().unwrap(),
            "-o",
            artifact.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    fs::remove_file(dir.join("src/main.gu")).unwrap();
    fs::remove_file(dir.join("src/math.gu")).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", artifact.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("src/math.gu"));

    fs::write(
        dir.join("src/main.gu"),
        "import \"math.gu\"\n\nfn main() -> Int:\n    return 0\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/math.gu"),
        "import \"main.gu\"\n\nfn helper() -> Int:\n    return 1\n",
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["check", dir.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("E_MODULE_CYCLE"));

    fs::write(
        dir.join("src/main.gu"),
        "import \"../outside.gu\"\n\nfn main() -> Int:\n    return 0\n",
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["check", dir.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("E_IMPORT_PATH"));

    fs::write(
        dir.join("src/main.gu"),
        "import \"large.gu\"\n\nfn main() -> Int:\n    return 0\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/large.gu"),
        format!("{}\n", " ".repeat(8 * 1024 * 1024)),
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["check", dir.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("E_MODULE_SIZE"));
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn build_defaults_to_the_guion_artifact_extension() {
    let base = std::env::temp_dir().join(format!("guion-default-build-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(&base).unwrap();
    let source = base.join("main.gu");
    fs::write(
        &source,
        "fn main() -> Int:\n    print(\"default artifact\")\n    return 0\n",
    )
    .unwrap();

    let built = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["build", source.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
    let artifact = source.with_extension("gub");
    assert!(
        artifact.is_file(),
        "default output should be {}",
        artifact.display()
    );
    assert!(
        !source.with_extension("vrb").exists(),
        "new builds must not write a Veritas-named artifact"
    );

    let run = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", artifact.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "default artifact\n");
    let _ = fs::remove_dir_all(base);
}

#[test]
fn build_json_reports_artifact_identity_and_source_diagnostics() {
    let base = std::env::temp_dir().join(format!("guion-json-build-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(&base).unwrap();
    let source = base.join("main.gu");
    let artifact = base.join("built app.gub");
    fs::write(&source, "fn main() -> Int:\n    return 0\n").unwrap();

    let built = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args([
            "build",
            "--json",
            source.to_str().unwrap(),
            "--output",
            artifact.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
    assert!(built.stderr.is_empty());
    let report: serde_json::Value = serde_json::from_slice(&built.stdout).unwrap();
    assert_eq!(report["kind"], "guion_build_report");
    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["source"], source.to_string_lossy().as_ref());
    assert_eq!(report["artifact"], artifact.to_string_lossy().as_ref());
    assert_eq!(report["artifact_format"], 33);
    assert_eq!(
        report["artifact_size_bytes"],
        fs::metadata(&artifact).unwrap().len()
    );
    assert_eq!(
        report["artifact_sha256"],
        format!("{:x}", Sha256::digest(fs::read(&artifact).unwrap()))
    );
    assert!(report["source_sha256"].as_str().is_some());
    assert_eq!(report["compiler_version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(
        report["target"],
        format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
    );

    fs::write(&source, "fn main() -> Int:\n    return missing_name\n").unwrap();
    let failed = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args([
            "build",
            "--json",
            source.to_str().unwrap(),
            "--output",
            artifact.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(failed.status.code(), Some(1));
    assert!(failed.stderr.is_empty());
    let diagnostic: serde_json::Value = serde_json::from_slice(&failed.stdout).unwrap();
    assert_eq!(diagnostic["kind"], "guion_diagnostic");
    assert_eq!(diagnostic["diagnostic"]["code"], "E_NAME_UNKNOWN");
    assert_eq!(diagnostic["diagnostic"]["line"], 2);
    let _ = fs::remove_dir_all(base);
}

#[test]
fn assemble_ir_creates_a_runnable_versioned_artifact() {
    let base = std::env::temp_dir().join(format!("guion-assemble-ir-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(&base).unwrap();
    let ir = base.join("program.gui.json");
    let artifact = base.join("program.gub");
    fs::write(
        &ir,
        r#"{
            "schema": "guion-bytecode-ir/v1",
            "source_name": "generated.gu",
            "entry": "main",
            "functions": [{
                "name": "main",
                "parameters": [],
                "result": "Int",
                "local_count": 0,
                "code": [
                    {"op": "ConstInt", "value": "42"},
                    {"op": "Return"}
                ]
            }]
        }"#,
    )
    .unwrap();

    let assembled = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args([
            "assemble-ir",
            ir.to_str().unwrap(),
            "--output",
            artifact.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        assembled.status.success(),
        "{}",
        String::from_utf8_lossy(&assembled.stderr)
    );
    assert_eq!(
        fs::read(&artifact).unwrap().get(..4),
        Some(b"VRTA".as_slice())
    );

    let run = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", artifact.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(run.status.code(), Some(42));
    assert!(run.stderr.is_empty());
    let _ = fs::remove_dir_all(base);
}

#[test]
fn assemble_ir_rejects_unknown_opcodes_without_creating_an_artifact() {
    let base =
        std::env::temp_dir().join(format!("guion-assemble-ir-invalid-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(&base).unwrap();
    let ir = base.join("bad.gui.json");
    let artifact = base.join("bad.gub");
    fs::write(
        &ir,
        r#"{"schema":"guion-bytecode-ir/v1","entry":"main","functions":[{"name":"main","parameters":[],"result":"Int","local_count":0,"code":[{"op":"MagicSpeedup"}]}]}"#,
    )
    .unwrap();
    let assembled = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args([
            "assemble-ir",
            ir.to_str().unwrap(),
            "--output",
            artifact.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(assembled.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&assembled.stderr).contains("unsupported IR opcode `MagicSpeedup`")
    );
    assert!(!artifact.exists());
    let _ = fs::remove_dir_all(base);
}

#[test]
fn assemble_ir_resolves_symbolic_control_flow_labels() {
    let base = std::env::temp_dir().join(format!("guion-ir-labels-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(&base).unwrap();
    let ir = base.join("loop.gui.json");
    let artifact = base.join("loop.gub");
    fs::write(
        &ir,
        r#"{"schema":"guion-bytecode-ir/v1","entry":"main","functions":[{"name":"main","parameters":[],"result":"Int","local_count":1,"code":[{"op":"ConstInt","value":"0"},{"op":"Store","local":0},{"op":"Label","id":10},{"op":"Load","local":0},{"op":"ConstInt","value":"3"},{"op":"Less"},{"op":"JumpIfFalse","label_id":20},{"op":"Load","local":0},{"op":"ConstInt","value":"1"},{"op":"Add"},{"op":"Store","local":0},{"op":"Jump","label_id":10},{"op":"Label","id":20},{"op":"Load","local":0},{"op":"Return"}]}]}"#,
    )
    .unwrap();
    let assembled = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args([
            "assemble-ir",
            ir.to_str().unwrap(),
            "--output",
            artifact.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        assembled.status.success(),
        "{}",
        String::from_utf8_lossy(&assembled.stderr)
    );
    let run = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", artifact.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(run.status.code(), Some(3));
    let _ = fs::remove_dir_all(base);
}

#[test]
fn guion_source_compiler_emits_ir_that_assembles_and_runs() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let compiler = root.join("language/self_host/compiler.gu");
    let base = std::env::temp_dir().join(format!("guion-source-compiler-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(&base).unwrap();
    let source_name = format!(".source-compiler-input-{}.gu", std::process::id());
    let source = root.join("language/self_host").join(&source_name);
    fs::write(
        &source,
        "fn increment(value: Int) -> Int:\n    return value + 1\n\nfn main() -> Int:\n    print(increment(41))\n    return 42\n",
    )
    .unwrap();
    let ir = base.join("program.gui.json");
    let artifact = base.join("program.gub");

    let generated = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", compiler.to_str().unwrap(), source_name.as_str()])
        .output()
        .unwrap();
    assert!(
        generated.status.success(),
        "{}",
        String::from_utf8_lossy(&generated.stderr)
    );
    assert!(String::from_utf8_lossy(&generated.stdout).contains("guion-bytecode-ir/v1"));
    fs::write(&ir, generated.stdout).unwrap();

    let assembled = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args([
            "assemble-ir",
            ir.to_str().unwrap(),
            "--output",
            artifact.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        assembled.status.success(),
        "{}",
        String::from_utf8_lossy(&assembled.stderr)
    );

    let run = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", artifact.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(run.status.code(), Some(42));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "42\n");
    assert!(run.stderr.is_empty());
    let _ = fs::remove_file(source);
    let _ = fs::remove_dir_all(base);
}

#[test]
fn guion_source_compiler_compiles_itself_and_real_programs_with_parity() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let source_project = root.join("language/self_host");
    let base = std::env::temp_dir().join(format!("guion-self-host-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    let _cleanup = RemoveDirOnDrop(base.clone());
    fs::create_dir_all(&base).unwrap();
    for filename in [
        "guion.toml",
        "compiler.gu",
        "lexer.gu",
        "parser.gu",
        "builtin_catalog.gu",
        "artifact.gu",
    ] {
        fs::copy(source_project.join(filename), base.join(filename)).unwrap();
    }

    let stage_zero = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", "compiler.gu", "compiler.gu"])
        .current_dir(&base)
        .output()
        .unwrap();
    assert!(
        stage_zero.status.success(),
        "{}{}",
        String::from_utf8_lossy(&stage_zero.stdout),
        String::from_utf8_lossy(&stage_zero.stderr)
    );

    let stage_zero_ir = base.join("stage-zero.gui.json");
    let stage_one_artifact = base.join(".compiler-stage-one.gub");
    fs::write(&stage_zero_ir, &stage_zero.stdout).unwrap();
    let assembled = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["assemble-ir", stage_zero_ir.to_str().unwrap(), "--output"])
        .arg(&stage_one_artifact)
        .current_dir(&base)
        .output()
        .unwrap();
    assert!(
        assembled.status.success(),
        "{}",
        String::from_utf8_lossy(&assembled.stderr)
    );

    let stage_one = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", "--json"])
        .arg(&stage_one_artifact)
        .arg("compiler.gu")
        .current_dir(&base)
        .output()
        .unwrap();
    assert!(
        stage_one.status.success(),
        "{}",
        String::from_utf8_lossy(&stage_one.stdout)
    );
    let report: serde_json::Value = serde_json::from_slice(&stage_one.stdout).unwrap();
    assert_eq!(report["exit_code"], 0);
    assert!(report["diagnostic"].is_null());
    assert_eq!(
        report["program_stdout"].as_str().unwrap().as_bytes(),
        stage_zero.stdout
    );

    let stage_one_ir = base.join("stage-one.gui.json");
    let stage_two_artifact = base.join(".compiler-stage-two.gub");
    fs::write(&stage_one_ir, report["program_stdout"].as_str().unwrap()).unwrap();
    let stage_two_assembled = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["assemble-ir", stage_one_ir.to_str().unwrap(), "--output"])
        .arg(&stage_two_artifact)
        .current_dir(&base)
        .output()
        .unwrap();
    assert!(
        stage_two_assembled.status.success(),
        "{}",
        String::from_utf8_lossy(&stage_two_assembled.stderr)
    );

    let stage_two = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", "--json"])
        .arg(&stage_two_artifact)
        .arg("compiler.gu")
        .current_dir(&base)
        .output()
        .unwrap();
    assert!(
        stage_two.status.success(),
        "{}",
        String::from_utf8_lossy(&stage_two.stdout)
    );
    let stage_two_report: serde_json::Value = serde_json::from_slice(&stage_two.stdout).unwrap();
    assert_eq!(stage_two_report["exit_code"], 0);
    assert!(stage_two_report["diagnostic"].is_null());
    assert_eq!(
        stage_two_report["program_stdout"]
            .as_str()
            .unwrap()
            .as_bytes(),
        stage_zero.stdout,
        "the Guion-compiled compiler must reproduce its own IR byte-for-byte"
    );

    let inventory_source = root.join("evaluation/development/inventory_reconcile");
    let inventory_host = base.join("inventory-host");
    let inventory_self = base.join("inventory-self");
    let inventory_python = base.join("inventory-python");
    for project in [&inventory_host, &inventory_self, &inventory_python] {
        copy_project_without_managed_environment(&inventory_source, project);
    }

    let host_build = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["build", "."])
        .current_dir(&inventory_host)
        .output()
        .unwrap();
    assert!(
        host_build.status.success(),
        "{}{}",
        String::from_utf8_lossy(&host_build.stdout),
        String::from_utf8_lossy(&host_build.stderr)
    );

    let inventory_stage_two = inventory_self.join(".compiler-stage-two.gub");
    fs::copy(&stage_two_artifact, &inventory_stage_two).unwrap();
    let self_compiled = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", "--json"])
        .arg(&inventory_stage_two)
        .arg("main.gu")
        .current_dir(&inventory_self)
        .output()
        .unwrap();
    assert!(
        self_compiled.status.success(),
        "{}",
        String::from_utf8_lossy(&self_compiled.stdout)
    );
    let self_report: serde_json::Value = serde_json::from_slice(&self_compiled.stdout).unwrap();
    assert_eq!(self_report["exit_code"], 0);
    assert!(self_report["diagnostic"].is_null());
    let self_ir = inventory_self.join("self-compiled-inventory.gui.json");
    fs::write(&self_ir, self_report["program_stdout"].as_str().unwrap()).unwrap();
    let self_artifact = inventory_self.join("self-main.gub");
    let self_assembled = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["assemble-ir", self_ir.to_str().unwrap(), "--output"])
        .arg(&self_artifact)
        .current_dir(&inventory_self)
        .output()
        .unwrap();
    assert!(
        self_assembled.status.success(),
        "{}",
        String::from_utf8_lossy(&self_assembled.stderr)
    );

    let large_inventory: Vec<serde_json::Value> = (0..80)
        .map(|index| serde_json::json!({"sku": format!("SKU-{index:03}"), "quantity": index + 10}))
        .collect();
    let generated_adjustments: Vec<serde_json::Value> = (0..600)
        .map(|index| {
            serde_json::json!({
                "sku": format!("SKU-{:03}", (index * 37) % 80),
                "delta": (index % 7) - 3,
                "reason": format!("count-{index}"),
                "source": "cycle-count"
            })
        })
        .chain((0..20).map(|index| {
            serde_json::json!({
                "sku": format!("NEW-{index:03}"),
                "delta": index + 1,
                "reason": "opening stock",
                "source": "receiving"
            })
        }))
        .collect();
    let cases = [
        (
            "unicode-and-ordered-adjustments",
            serde_json::json!({"items": [
                {"sku": "A-1", "quantity": 10},
                {"sku": "café-東京", "quantity": 4}
            ]}),
            serde_json::json!({"adjustments": [
                {"sku": "A-1", "delta": -3, "reason": "damaged", "source": "cycle-count"},
                {"sku": "NEW", "delta": 7, "reason": "initial stock", "source": "receiving"},
                {"sku": "A-1", "delta": 2, "reason": "found units", "source": "cycle-count"},
                {"sku": "café-東京", "delta": -4, "reason": "expired", "source": "inspection"},
                {"sku": "NEW", "delta": 5, "reason": "late delivery", "source": "receiving"}
            ]}),
            true,
        ),
        (
            "reject-negative-without-partial-write",
            serde_json::json!({"items": [{"sku": "A-1", "quantity": 2}]}),
            serde_json::json!({"adjustments": [{"sku": "A-1", "delta": -3, "reason": "damaged", "source": "inspection"}]}),
            false,
        ),
        (
            "generated-100-skus-620-adjustments",
            serde_json::json!({"items": large_inventory}),
            serde_json::json!({"adjustments": generated_adjustments}),
            true,
        ),
    ];

    for (case, inventory, adjustments, expected_success) in cases {
        for project in [&inventory_host, &inventory_self, &inventory_python] {
            fs::write(
                project.join("inventory.json"),
                serde_json::to_vec(&inventory).unwrap(),
            )
            .unwrap();
            fs::write(
                project.join("adjustments.json"),
                serde_json::to_vec(&adjustments).unwrap(),
            )
            .unwrap();
            if !expected_success {
                fs::write(project.join("reconciled.json"), b"previous result\n").unwrap();
            } else {
                let _ = fs::remove_file(project.join("reconciled.json"));
            }
        }

        let host_run = Command::new(env!("CARGO_BIN_EXE_guion"))
            .args([
                "run",
                "main.gub",
                "inventory.json",
                "adjustments.json",
                "reconciled.json",
            ])
            .current_dir(&inventory_host)
            .output()
            .unwrap();
        let self_run = Command::new(env!("CARGO_BIN_EXE_guion"))
            .args([
                "run",
                "self-main.gub",
                "inventory.json",
                "adjustments.json",
                "reconciled.json",
            ])
            .current_dir(&inventory_self)
            .output()
            .unwrap();
        let python_run = Command::new("python3")
            .args([
                "reference.py",
                "inventory.json",
                "adjustments.json",
                "reconciled.json",
            ])
            .current_dir(&inventory_python)
            .output()
            .unwrap();
        for (label, run) in [
            ("self-compiled", &self_run),
            ("Python reference", &python_run),
        ] {
            assert_eq!(
                host_run.status.code(),
                run.status.code(),
                "{case}: {label} exit differs"
            );
            assert_eq!(
                host_run.stdout, run.stdout,
                "{case}: {label} stdout differs"
            );
            assert_eq!(
                host_run.stderr, run.stderr,
                "{case}: {label} stderr differs"
            );
            assert_eq!(
                host_run.status.success(),
                expected_success,
                "{case}: unexpected host exit"
            );
            let host_output = fs::read(inventory_host.join("reconciled.json")).ok();
            let candidate_output = fs::read(if label == "self-compiled" {
                inventory_self.join("reconciled.json")
            } else {
                inventory_python.join("reconciled.json")
            })
            .ok();
            assert_eq!(
                host_output, candidate_output,
                "{case}: {label} output differs"
            );
        }
    }

    let richards_source = root.join("evaluation/development/famous_benchmarks/richards.vr");
    let richards_host = base.join("richards-host");
    let richards_self = base.join("richards-self");
    for project in [&richards_host, &richards_self] {
        fs::create_dir_all(project).unwrap();
        fs::copy(&richards_source, project.join("richards.gu")).unwrap();
        fs::write(
            project.join("guion.toml"),
            "[project]\nname = \"richards-bootstrap-probe\"\nversion = \"0.1.0\"\nentry = \"richards.gu\"\n\n[runtime]\ninstruction_limit = 30000000\n",
        )
        .unwrap();
    }

    let richards_host_build = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["build", "."])
        .current_dir(&richards_host)
        .output()
        .unwrap();
    assert!(
        richards_host_build.status.success(),
        "{}{}",
        String::from_utf8_lossy(&richards_host_build.stdout),
        String::from_utf8_lossy(&richards_host_build.stderr)
    );

    let richards_compiler = richards_self.join(".compiler-stage-two.gub");
    fs::copy(&stage_two_artifact, &richards_compiler).unwrap();
    let richards_self_ir_report = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", "--json"])
        .arg(&richards_compiler)
        .arg("richards.gu")
        .current_dir(&richards_self)
        .output()
        .unwrap();
    assert!(
        richards_self_ir_report.status.success(),
        "{}{}",
        String::from_utf8_lossy(&richards_self_ir_report.stdout),
        String::from_utf8_lossy(&richards_self_ir_report.stderr)
    );
    let richards_self_report: serde_json::Value =
        serde_json::from_slice(&richards_self_ir_report.stdout).unwrap();
    assert_eq!(richards_self_report["exit_code"], 0);
    assert!(richards_self_report["diagnostic"].is_null());
    let richards_self_ir = richards_self.join("richards-self.gui.json");
    fs::write(
        &richards_self_ir,
        richards_self_report["program_stdout"].as_str().unwrap(),
    )
    .unwrap();
    let richards_self_artifact = richards_self.join("richards-self.gub");
    let richards_assembled = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args([
            "assemble-ir",
            richards_self_ir.to_str().unwrap(),
            "--output",
        ])
        .arg(&richards_self_artifact)
        .current_dir(&richards_self)
        .output()
        .unwrap();
    assert!(
        richards_assembled.status.success(),
        "{}",
        String::from_utf8_lossy(&richards_assembled.stderr)
    );

    let richards_host_run = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", "--json"])
        .arg(richards_host.join("richards.gub"))
        .current_dir(&richards_host)
        .output()
        .unwrap();
    let richards_self_run = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", "--json"])
        .arg(&richards_self_artifact)
        .current_dir(&richards_self)
        .output()
        .unwrap();
    let python_reference =
        root.join("evaluation/development/famous_benchmarks/richards_reference.py");
    let richards_python_run = Command::new("python3")
        .arg(python_reference)
        .output()
        .unwrap();
    let richards_host_report: serde_json::Value =
        serde_json::from_slice(&richards_host_run.stdout).unwrap();
    let richards_self_run_report: serde_json::Value =
        serde_json::from_slice(&richards_self_run.stdout).unwrap();
    assert_eq!(richards_host_report["exit_code"], 0);
    assert_eq!(richards_self_run_report["exit_code"], 0);
    assert!(richards_host_report["diagnostic"].is_null());
    assert!(richards_self_run_report["diagnostic"].is_null());
    assert!(richards_python_run.status.success());
    assert_eq!(richards_host_report["program_stdout"], "9297\n23246\n");
    assert_eq!(
        richards_self_run_report["program_stdout"],
        richards_host_report["program_stdout"]
    );
    assert_eq!(
        String::from_utf8_lossy(&richards_python_run.stdout),
        richards_host_report["program_stdout"].as_str().unwrap()
    );

    let _ = fs::remove_dir_all(base);
}

#[test]
fn mutable_json_array_append_fits_a_bounded_allocation_budget() {
    let dir = std::env::temp_dir().join(format!("guion-json-append-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("guion.toml"),
        "[project]\nname = \"json-append-budget\"\nversion = \"0.1.0\"\nentry = \"src/main.gu\"\n\n[runtime]\nallocation_limit_bytes = 64000000\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/main.gu"),
        "fn main() -> Int:\n    var values = json_array()\n    var cursor = 0\n    while cursor < 2500:\n        set values = json_push(values, json_int(cursor))\n        set cursor = cursor + 1\n    print(json_len(values))\n    return 0\n",
    )
    .unwrap();

    let run = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", dir.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "2500\n");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn build_creates_a_portable_artifact_that_runs_without_source() {
    let dir = std::env::temp_dir().join(format!("guion-build-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let source = dir.join("main.gu");
    let artifact = dir.join("bundle.gub");
    fs::write(
        &source,
        "fn main() -> Int:\n    print(999999999999999999999999999999 + 1)\n    return 0\n",
    )
    .unwrap();
    let built = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args([
            "build",
            source.to_str().unwrap(),
            "-o",
            artifact.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
    fs::remove_file(&source).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", artifact.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "1000000000000000000000000000000\n"
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn run_preserves_output_produced_before_a_runtime_error() {
    let dir = std::env::temp_dir().join(format!("guion-partial-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("main.gu");
    fs::write(&file, "fn main() -> Int:\n    print(\"started\")\n    while true:\n        let again = 1\n    return 0\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", file.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout), "started\n");
    assert!(String::from_utf8_lossy(&output.stderr).contains("R_STEP_LIMIT"));
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn new_refuses_to_overwrite_an_existing_project() {
    let dir = std::env::temp_dir().join(format!("guion-new-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let original = dir.join("keep.txt");
    fs::write(&original, "preserve").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["new", dir.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(fs::read_to_string(original).unwrap(), "preserve");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn new_creates_a_runnable_project_without_requiring_python_naming() {
    let parent = std::env::temp_dir().join(format!("guion-new-ok-{}", std::process::id()));
    let project = parent.join("hello world");
    let _ = fs::remove_dir_all(&parent);
    fs::create_dir_all(&parent).unwrap();
    let created = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["new", project.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        created.status.success(),
        "{}",
        String::from_utf8_lossy(&created.stderr)
    );
    let manifest = fs::read_to_string(project.join("guion.toml")).unwrap();
    assert!(manifest.contains("name = \"hello-world\""), "{manifest}");
    let run = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", project.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "Hello, Guion!\n");
    let tests = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["test", project.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        tests.status.success(),
        "{}",
        String::from_utf8_lossy(&tests.stderr)
    );
    assert!(String::from_utf8_lossy(&tests.stdout).contains("1 tests passed"));
    let _ = fs::remove_dir_all(parent);
}

#[test]
fn project_manifest_rejects_entry_path_escape() {
    let parent = std::env::temp_dir().join(format!("guion-manifest-{}", std::process::id()));
    let project = parent.join("project");
    let _ = fs::remove_dir_all(&parent);
    fs::create_dir_all(&project).unwrap();
    fs::write(
        project.join("guion.toml"),
        "[project]\nname = \"escape\"\nversion = \"0.1.0\"\nentry = \"../outside.gu\"\n",
    )
    .unwrap();
    fs::write(
        parent.join("outside.gu"),
        "fn main() -> Int:\n    return 0\n",
    )
    .unwrap();
    let run = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", project.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!run.status.success());
    assert!(String::from_utf8_lossy(&run.stderr).contains("relative path"));
    let _ = fs::remove_dir_all(parent);
}

#[test]
fn project_runtime_manifest_enforces_the_allocation_budget() {
    let dir = std::env::temp_dir().join(format!("guion-runtime-policy-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("guion.toml"),
        "[project]\nname = \"runtime-policy\"\nversion = \"0.1.0\"\nentry = \"src/main.gu\"\n\n[runtime]\nallocation_limit_bytes = 1024\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/main.gu"),
        "fn main() -> Int:\n    var index = 0\n    while index < 40:\n        let text = \"12345678\" + \"abcdefgh\"\n        set index = index + 1\n    return 0\n",
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", dir.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("R_ALLOCATION_LIMIT"));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn built_artifact_carries_runtime_limits_when_moved_outside_project() {
    let base = std::env::temp_dir().join(format!("guion-portable-policy-{}", std::process::id()));
    let project = base.join("project");
    let detached = base.join("detached");
    let artifact = project.join("src/main.gub");
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(project.join("src")).unwrap();
    fs::create_dir_all(&detached).unwrap();
    fs::write(
        project.join("guion.toml"),
        "[project]\nname = \"portable-policy\"\nversion = \"0.1.0\"\nentry = \"src/main.gu\"\n\n[runtime]\nallocation_limit_bytes = 1024\n",
    )
    .unwrap();
    fs::write(
        project.join("src/main.gu"),
        "fn main() -> Int:\n    var index = 0\n    while index < 40:\n        let text = \"12345678\" + \"abcdefgh\"\n        set index = index + 1\n    return 0\n",
    )
    .unwrap();
    let built = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["build", project.join("src/main.gu").to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
    let moved_artifact = detached.join("program.gub");
    fs::copy(&artifact, &moved_artifact).unwrap();
    let inspected = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["inspect", moved_artifact.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        inspected.status.success(),
        "{}",
        String::from_utf8_lossy(&inspected.stderr)
    );
    assert!(String::from_utf8_lossy(&inspected.stdout).contains("allocation_limit_bytes: 1024"));
    let report = String::from_utf8_lossy(&inspected.stdout);
    let source_digest = report
        .lines()
        .find_map(|line| line.strip_prefix("source sha256: "))
        .expect("inspect reports the source digest");
    assert_eq!(source_digest.len(), 64);
    assert!(source_digest.bytes().all(|byte| byte.is_ascii_hexdigit()));
    assert!(report.contains("python dependency locks: absent"));
    let repeated_artifact = detached.join("repeat.gub");
    let repeated_build = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args([
            "build",
            project.join("src/main.gu").to_str().unwrap(),
            "--output",
            repeated_artifact.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(repeated_build.status.success());
    assert_eq!(
        fs::read(&artifact).unwrap(),
        fs::read(&repeated_artifact).unwrap()
    );
    fs::write(
        project.join("src/main.gu"),
        "fn main() -> Int:\n    print(1)\n    var index = 0\n    while index < 40:\n        let text = \"12345678\" + \"abcdefgh\"\n        set index = index + 1\n    return 0\n",
    )
    .unwrap();
    let rebuilt_artifact = detached.join("rebuilt.gub");
    let rebuilt = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args([
            "build",
            project.join("src/main.gu").to_str().unwrap(),
            "--output",
            rebuilt_artifact.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        rebuilt.status.success(),
        "{}",
        String::from_utf8_lossy(&rebuilt.stderr)
    );
    let rebuilt_inspect = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["inspect", rebuilt_artifact.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(rebuilt_inspect.status.success());
    let rebuilt_report = String::from_utf8_lossy(&rebuilt_inspect.stdout);
    let rebuilt_digest = rebuilt_report
        .lines()
        .find_map(|line| line.strip_prefix("source sha256: "))
        .expect("inspect reports the rebuilt source digest");
    assert_ne!(source_digest, rebuilt_digest);
    let run = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", moved_artifact.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!run.status.success());
    assert!(String::from_utf8_lossy(&run.stderr).contains("R_ALLOCATION_LIMIT"));
    let _ = fs::remove_dir_all(&base);
}

#[test]
fn build_metadata_tracks_python_lock_changes() {
    let base = std::env::temp_dir().join(format!("guion-lock-digest-{}", std::process::id()));
    let project = base.join("project");
    let bridge = project.join(".guion/python-bridge");
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(project.join("src")).unwrap();
    fs::create_dir_all(&bridge).unwrap();
    fs::write(
        project.join("guion.toml"),
        "[project]\nname = \"lock-digest\"\nversion = \"0.1.0\"\nentry = \"src/main.gu\"\n",
    )
    .unwrap();
    fs::write(
        project.join("src/main.gu"),
        "import \"helper.gu\"\nfn main() -> Int:\n    return helper()\n",
    )
    .unwrap();
    fs::write(
        project.join("src/helper.gu"),
        "pub fn helper() -> Int:\n    return 0\n",
    )
    .unwrap();
    fs::write(project.join("guion.lock"), "manifest_sha256 = \"abc\"\n").unwrap();
    fs::write(bridge.join("uv.lock"), "version = 1\npackage = \"one\"\n").unwrap();

    let first_artifact = base.join("first.gub");
    let first_build = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args([
            "build",
            project.join("src/main.gu").to_str().unwrap(),
            "-o",
            first_artifact.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        first_build.status.success(),
        "{}",
        String::from_utf8_lossy(&first_build.stderr)
    );
    let first_inspect = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["inspect", first_artifact.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(first_inspect.status.success());
    let first_report = String::from_utf8_lossy(&first_inspect.stdout);
    let first_digest = first_report
        .lines()
        .find_map(|line| line.strip_prefix("python dependency locks: "))
        .expect("inspect reports dependency lock identity");
    assert_eq!(first_digest.len(), 64);
    let first_source_digest = first_report
        .lines()
        .find_map(|line| line.strip_prefix("source sha256: "))
        .expect("inspect reports source graph identity")
        .to_owned();

    fs::write(bridge.join("uv.lock"), "version = 1\npackage = \"two\"\n").unwrap();
    let second_artifact = base.join("second.gub");
    let second_build = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args([
            "build",
            project.join("src/main.gu").to_str().unwrap(),
            "-o",
            second_artifact.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        second_build.status.success(),
        "{}",
        String::from_utf8_lossy(&second_build.stderr)
    );
    let second_inspect = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["inspect", second_artifact.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(second_inspect.status.success());
    let second_report = String::from_utf8_lossy(&second_inspect.stdout);
    let second_digest = second_report
        .lines()
        .find_map(|line| line.strip_prefix("python dependency locks: "))
        .expect("inspect reports updated dependency lock identity");
    assert_ne!(first_digest, second_digest);
    let second_source_digest = second_report
        .lines()
        .find_map(|line| line.strip_prefix("source sha256: "))
        .expect("inspect reports unchanged source graph identity");
    assert_eq!(first_source_digest, second_source_digest);

    fs::write(
        project.join("src/helper.gu"),
        "pub fn helper() -> Int:\n    return 1\n",
    )
    .unwrap();
    let source_changed_artifact = base.join("source-changed.gub");
    let source_changed_build = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args([
            "build",
            project.join("src/main.gu").to_str().unwrap(),
            "-o",
            source_changed_artifact.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        source_changed_build.status.success(),
        "{}",
        String::from_utf8_lossy(&source_changed_build.stderr)
    );
    let source_changed_inspect = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["inspect", source_changed_artifact.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(source_changed_inspect.status.success());
    let source_changed_report = String::from_utf8_lossy(&source_changed_inspect.stdout);
    let source_changed_digest = source_changed_report
        .lines()
        .find_map(|line| line.strip_prefix("source sha256: "))
        .expect("inspect reports changed source graph identity");
    assert_ne!(first_source_digest, source_changed_digest);
    fs::remove_file(project.join("guion.lock")).unwrap();
    let incomplete_build = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args([
            "build",
            project.join("src/main.gu").to_str().unwrap(),
            "-o",
            base.join("incomplete.gub").to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!incomplete_build.status.success());
    assert!(String::from_utf8_lossy(&incomplete_build.stderr)
        .contains("incomplete Python dependency locks"));
    let _ = fs::remove_dir_all(&base);
}

#[test]
fn formatter_normalizes_line_endings_and_trailing_space_without_losing_comments_or_strings() {
    let dir = std::env::temp_dir().join(format!("guion-fmt-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let source = dir.join("main.gu");
    fs::write(
        &source,
        "# keep this comment\r\nfn main() -> Int:  \r\n    print(\"two spaces  \" )  \r\n    return 0\r\n\r\n",
    )
    .unwrap();
    let formatted = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["fmt", source.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        formatted.status.success(),
        "{}",
        String::from_utf8_lossy(&formatted.stderr)
    );
    let text = fs::read_to_string(&source).unwrap();
    assert!(text.starts_with("# keep this comment\n"));
    assert!(text.contains("two spaces  "));
    assert!(text.ends_with("return 0\n"));
    let checked = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["fmt", "--check", source.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        checked.status.success(),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn cli_runs_typed_python_imports_only_when_an_interpreter_is_explicitly_configured() {
    let dir = std::env::temp_dir().join(format!("guion-python-cli-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let source = dir.join("main.gu");
    fs::write(
        &source,
        "extern python \"math:factorial\" fn factorial(value: Int) -> Int\n\nfn main() -> Int:\n    print(factorial(20))\n    return 0\n",
    )
    .unwrap();
    let denied = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", source.to_str().unwrap()])
        .env_remove("GUION_PYTHON")
        .output()
        .unwrap();
    assert!(!denied.status.success());
    assert!(String::from_utf8_lossy(&denied.stderr).contains("R_PYTHON_UNCONFIGURED"));

    let run = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", source.to_str().unwrap()])
        .env("GUION_PYTHON", python3_path())
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "2432902008176640000\n"
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn cli_file_capabilities_are_limited_to_the_source_directory() {
    let dir = std::env::temp_dir().join(format!("guion-cli-files-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(dir.join("input.txt"), "local data").unwrap();
    fs::write(
        dir.join("guion.toml"),
        "[project]\nname = \"files\"\nversion = \"0.1.0\"\nentry = \"src/main.gu\"\n",
    )
    .unwrap();
    let source = dir.join("src/main.gu");
    fs::write(
        &source,
        "fn main() -> Int:\n    print(unwrap(read_text(\"input.txt\")))\n    return 0\n",
    )
    .unwrap();
    let run = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", source.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "local data\n");
    fs::write(
        &source,
        "fn main() -> Int:\n    let result = read_text(\"../outside.txt\")\n    assert(not is_ok(result))\n    print(file_error_code(error(result)))\n    return 0\n",
    )
    .unwrap();
    let escaped = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", source.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        escaped.status.success(),
        "{}",
        String::from_utf8_lossy(&escaped.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&escaped.stdout), "R_HOST_DENIED\n");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn run_passes_user_arguments_to_guion_source() {
    let dir = std::env::temp_dir().join(format!("guion-cli-args-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let source = dir.join("args.gu");
    fs::write(
        &source,
        "fn main() -> Int:\n    let values = args()\n    print(values[0])\n    return 0\n",
    )
    .unwrap();
    let run = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", source.to_str().unwrap(), "hello world", "é"])
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "hello world\n");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn run_maps_main_results_to_process_exit_statuses() {
    let dir = std::env::temp_dir().join(format!("guion-cli-exit-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    let unit_source = dir.join("unit.gu");
    fs::write(&unit_source, "fn main() -> Unit:\n    print(\"unit\")\n").unwrap();
    let unit = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", unit_source.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(unit.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&unit.stdout), "unit\n");

    let int_source = dir.join("int.gu");
    fs::write(&int_source, "fn main() -> Int:\n    return 23\n").unwrap();
    let int = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", int_source.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(int.status.code(), Some(23));

    fs::write(&int_source, "fn main() -> Int:\n    return 256\n").unwrap();
    let invalid = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", int_source.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(invalid.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("outside 0..255"));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn test_runs_discovered_source_tests_and_fails_on_assertions() {
    let dir = std::env::temp_dir().join(format!("guion-tests-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let source = dir.join("main.gu");
    fs::write(&source, "fn main() -> Int:\n    return 0\n\nfn test_sum() -> Unit:\n    assert(1 + 1 == 2)\n\nfn test_text() -> Unit:\n    assert(\"a\" + \"b\" == \"ab\")\n").unwrap();
    let passed = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["test", source.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        passed.status.success(),
        "{}",
        String::from_utf8_lossy(&passed.stderr)
    );
    assert!(String::from_utf8_lossy(&passed.stdout).contains("2 tests passed"));
    fs::write(
        &source,
        "fn main() -> Int:\n    return 0\n\nfn test_failure() -> Unit:\n    assert(false)\n",
    )
    .unwrap();
    let failed = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["test", source.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!failed.status.success());
    assert!(String::from_utf8_lossy(&failed.stderr).contains("R_ASSERTION"));
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn doc_json_reports_function_record_and_python_signatures() {
    let base = std::env::temp_dir().join(format!("guion-doc-json-{}", std::process::id()));
    let source = base.join("api.gu");
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(&base).unwrap();
    fs::write(
        &source,
        "record Point:\n    x: Int\n    y: Int\n\nextern python \"builtins:pow\" fn power(base: Int, exp: Int, mod: Int) -> Int\n\nfn translate(point: Point, dx: Int, dy: Int) -> Point:\n    return Point(point.x + dx, point.y + dy)\n\nfn main() -> Int:\n    return 0\n",
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["doc", "--json", source.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["kind"], "guion_api_report");
    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["functions"][0]["name"], "translate");
    assert_eq!(report["functions"][0]["parameters"][0]["type"], "Point");
    assert_eq!(report["records"][0]["name"], "Point");
    assert_eq!(report["records"][0]["fields"][0]["name"], "x");
    assert_eq!(report["python_functions"][0]["name"], "power");
    assert_eq!(
        report["python_functions"][0]["parameters"][1]["name"],
        "exp"
    );
    assert_eq!(report["python_functions"][0]["module"], "builtins");
    let human = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["doc", source.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(human.status.success());
    assert!(
        String::from_utf8_lossy(&human.stdout).contains("fn translate(Point, Int, Int) -> Point")
    );
    let _ = fs::remove_dir_all(base);
}

#[test]
fn test_json_report_is_machine_readable_on_success_and_failure() {
    let dir = std::env::temp_dir().join(format!("guion-json-test-report-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let source = dir.join("main.gu");
    fs::write(
        &source,
        "fn main() -> Int:\n    return 0\n\nfn test_first() -> Unit:\n    assert(1 + 1 == 2)\n\nfn test_second() -> Unit:\n    assert(2 + 2 == 4)\n",
    )
    .unwrap();
    let passed = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["test", source.to_str().unwrap(), "--json"])
        .output()
        .unwrap();
    assert!(passed.status.success());
    let report: serde_json::Value = serde_json::from_slice(&passed.stdout).unwrap();
    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["kind"], "guion_test_report");
    assert_eq!(report["summary"]["total"], 2);
    assert_eq!(report["summary"]["passed"], 2);
    assert_eq!(report["summary"]["failed"], 0);
    assert_eq!(report["tests"][0]["name"], "test_first");
    assert_eq!(report["tests"][0]["status"], "passed");
    assert!(String::from_utf8_lossy(&passed.stderr).is_empty());

    fs::write(
        &source,
        "fn main() -> Int:\n    return 0\n\nfn test_failure() -> Unit:\n    assert(false)\n",
    )
    .unwrap();
    let failed = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["test", source.to_str().unwrap(), "--json"])
        .output()
        .unwrap();
    assert!(!failed.status.success());
    let report: serde_json::Value = serde_json::from_slice(&failed.stdout).unwrap();
    assert_eq!(report["summary"]["failed"], 1);
    assert_eq!(report["tests"][0]["status"], "failed");
    assert_eq!(report["tests"][0]["diagnostic"]["code"], "R_ASSERTION");
    assert_eq!(
        report["tests"][0]["diagnostic"]["trace"][0]["source"],
        "main.gu"
    );
    assert!(report["tests"][0]["diagnostic"]["trace"][0]["line"]
        .as_u64()
        .is_some());
    assert!(String::from_utf8_lossy(&failed.stderr).is_empty());
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn check_json_reports_compiler_success_and_source_diagnostics() {
    let dir = std::env::temp_dir().join(format!("guion-json-check-report-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let source = dir.join("main.gu");
    fs::write(&source, "fn main() -> Int:\n    return 0\n").unwrap();

    let passed = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["check", source.to_str().unwrap(), "--json"])
        .output()
        .unwrap();
    assert!(passed.status.success());
    let report: serde_json::Value = serde_json::from_slice(&passed.stdout).unwrap();
    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["kind"], "guion_check_report");
    assert_eq!(report["status"], "passed");
    assert_eq!(report["function_count"], 1);

    fs::write(&source, "fn main() -> Int:\n    return missing_name\n").unwrap();
    let failed = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["check", source.to_str().unwrap(), "--json"])
        .output()
        .unwrap();
    assert_eq!(failed.status.code(), Some(1));
    let report: serde_json::Value = serde_json::from_slice(&failed.stdout).unwrap();
    assert_eq!(report["kind"], "guion_diagnostic");
    assert_eq!(report["diagnostic"]["code"], "E_NAME_UNKNOWN");
    assert_eq!(report["diagnostic"]["source"], "main.gu");
    assert!(report["diagnostic"]["line"].as_u64().is_some());
    assert!(report["diagnostic"]["column"].as_u64().is_some());
    assert!(String::from_utf8_lossy(&failed.stderr).is_empty());
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn run_json_report_captures_program_output_status_and_runtime_failure() {
    let dir = std::env::temp_dir().join(format!("guion-json-run-report-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let source = dir.join("main.gu");
    fs::write(
        &source,
        "fn sum_to(n: Int) -> Int:\n    var total = 0\n    var i = n\n    while i > 0:\n        set total = total + i\n        set i = i - 1\n    return total\n\nfn main() -> Int:\n    print(\"hello\")\n    eprint(\"duplicate id\")\n    print(sum_to(5))\n    return 23\n",
    )
    .unwrap();

    let passed = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", "--json", source.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(passed.status.code(), Some(23));
    let report: serde_json::Value = serde_json::from_slice(&passed.stdout).unwrap();
    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["kind"], "guion_run_report");
    assert_eq!(report["status"], "passed");
    assert_eq!(report["program_stdout"], "hello\n15\n");
    assert_eq!(report["program_stderr"], "duplicate id\n");
    assert_eq!(report["effects"][0]["kind"], "program_output");
    assert_eq!(report["effects"][0]["outcome"], "returned");
    assert_eq!(report["effects"][0]["target"], "6 bytes");
    assert_eq!(report["effects"][0]["target_truncated"], false);
    assert_eq!(report["effects"][1]["kind"], "program_error_output");
    assert_eq!(report["effects"][1]["target"], "13 bytes");
    assert_eq!(report["omitted_effects"], 0);
    assert_eq!(report["native_backend"]["compiled_functions"][0], "sum_to");
    assert_eq!(report["native_backend"]["calls"], 1);
    assert_eq!(report["native_backend"]["deoptimizations"], 0);
    assert_eq!(report["exit_code"], 23);
    assert!(String::from_utf8_lossy(&passed.stderr).is_empty());

    fs::write(
        &source,
        "fn main() -> Int:\n    print(\"before failure\")\n    return 1 / 0\n",
    )
    .unwrap();
    let failed = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", "--json", source.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(failed.status.code(), Some(1));
    let report: serde_json::Value = serde_json::from_slice(&failed.stdout).unwrap();
    assert_eq!(report["kind"], "guion_run_report");
    assert_eq!(report["status"], "failed");
    assert_eq!(report["program_stdout"], "before failure\n");
    assert_eq!(report["diagnostic"]["code"], "R_DIV_ZERO");
    assert_eq!(report["diagnostic"]["trace"][0]["source"], "main.gu");
    assert!(String::from_utf8_lossy(&failed.stderr).is_empty());

    fs::write(&source, "fn main() -> Int:\n    return missing_name\n").unwrap();
    let compile_failed = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", "--json", source.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(compile_failed.status.code(), Some(1));
    let report: serde_json::Value = serde_json::from_slice(&compile_failed.stdout).unwrap();
    assert_eq!(report["kind"], "guion_diagnostic");
    assert_eq!(report["diagnostic"]["code"], "E_NAME_UNKNOWN");
    assert_eq!(report["diagnostic"]["source"], "main.gu");
    assert!(String::from_utf8_lossy(&compile_failed.stderr).is_empty());
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn parallel_map_reuses_one_native_backend_across_workers() {
    let dir = std::env::temp_dir().join(format!(
        "guion-parallel-native-reuse-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let source = dir.join("main.gu");
    fs::write(
        &source,
        "fn work(count: Int) -> Int:\n    var index = 0\n    var total = 0\n    while index < count:\n        set total = total + index % 97\n        set index = index + 1\n    return total\n\nfn main() -> Int:\n    let tasks = [200, 200, 200, 200]\n    let outcome = parallel_map(tasks, work, 4)\n    if is_ok(outcome):\n        let results = unwrap(outcome)\n        print(results[0])\n        print(results[1])\n        print(results[2])\n        print(results[3])\n    return 0\n",
    )
    .unwrap();

    let run = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", "--json", source.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(run.status.code(), Some(0));
    let report: serde_json::Value = serde_json::from_slice(&run.stdout).unwrap();
    assert_eq!(report["program_stdout"], "9327\n9327\n9327\n9327\n");
    assert_eq!(report["native_backend"]["compilations"], 1);
    assert!(report["native_backend"]["calls"].as_u64().unwrap() >= 4);
    assert_eq!(report["native_backend"]["deoptimizations"], 0);
    assert!(String::from_utf8_lossy(&run.stderr).is_empty());

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn native_lowering_coalesces_straight_line_bytecode() {
    let dir = std::env::temp_dir().join(format!(
        "guion-native-block-coalescing-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let source = dir.join("main.gu");
    fs::write(
        &source,
        "fn add(left: Int, right: Int) -> Int:\n    return left + right\n\nfn main() -> Int:\n    print(add(20, 22))\n    return 0\n",
    )
    .unwrap();

    let run = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", "--json", source.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(run.status.code(), Some(0));
    let report: serde_json::Value = serde_json::from_slice(&run.stdout).unwrap();
    assert_eq!(report["program_stdout"], "42\n");
    let native = &report["native_backend"];
    let instructions = native["bytecode_instructions"].as_u64().unwrap();
    let blocks = native["bytecode_blocks"].as_u64().unwrap();
    let ir_blocks = native["cranelift_ir_blocks"].as_u64().unwrap();
    let ir_instructions = native["cranelift_ir_instructions"].as_u64().unwrap();
    let profile = native["compile_profile"]["functions"].as_array().unwrap();
    assert!(instructions >= 6);
    assert!(blocks > 0 && blocks < instructions);
    assert!(ir_blocks >= blocks);
    assert!(ir_instructions >= instructions);
    assert_eq!(profile.len(), 2);
    assert!(profile.iter().any(|entry| entry["name"] == "add"));
    assert!(String::from_utf8_lossy(&run.stderr).is_empty());

    let _ = fs::remove_dir_all(dir);
}
