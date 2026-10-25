use std::{fs, process::Command};

#[test]
fn unavailable_uv_does_not_create_a_partial_environment() {
    let root = std::env::temp_dir().join(format!(
        "guion-managed-python-missing-uv-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("guion.toml"),
        "[project]\nname = \"no-uv\"\nversion = \"0.1.0\"\nentry = \"src/main.gu\"\n\n[python]\nversion = \"3.14.8\"\n",
    )
    .unwrap();
    fs::write(
        root.join("src/main.gu"),
        "fn main() -> Int:\n    return 0\n",
    )
    .unwrap();
    let missing_uv = root.join("missing-uv");
    let result = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["sync", "--json", root.to_str().unwrap()])
        .env("GUION_UV", missing_uv)
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(result.stderr.is_empty());
    let report: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report["kind"], "guion_sync_report");
    assert_eq!(report["status"], "failed");
    assert_eq!(report["diagnostic"]["code"], "E_UV_NOT_FOUND");
    assert!(
        !root.join(".guion").exists(),
        "a failed sync must not leave a partial bridge project"
    );

    let wrong_version_uv = root.join("wrong-version-uv");
    fs::write(&wrong_version_uv, "#!/bin/sh\nprintf 'uv 9.9.9\\n'\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(&wrong_version_uv).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&wrong_version_uv, permissions).unwrap();
    }
    let wrong_version = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["sync", "--json", root.to_str().unwrap()])
        .env("GUION_UV", &wrong_version_uv)
        .output()
        .unwrap();
    assert!(!wrong_version.status.success());
    let report: serde_json::Value = serde_json::from_slice(&wrong_version.stdout).unwrap();
    assert_eq!(report["diagnostic"]["code"], "E_UV_VERSION_MISMATCH");
    assert!(
        !root.join(".guion").exists(),
        "an unpinned resolver must not create a partial bridge project"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn locked_python_dependencies_sync_and_run_through_managed_interpreter() {
    if std::env::var_os("GUION_TEST_UV_INTEGRATION").is_none() {
        eprintln!("skipping managed Python integration test: GUION_TEST_UV_INTEGRATION is unset");
        return;
    }
    let uv = std::env::var_os("GUION_UV").expect("GUION_UV is required for this integration test");
    let root =
        std::env::temp_dir().join(format!("guion-managed-python-{}-space", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../evaluation/development/python_interop");
    fs::create_dir_all(root.join(".veritas/python-bridge")).unwrap();
    for relative in [
        "veritas.toml",
        "main.vr",
        "veritas.lock",
        ".veritas/python-bridge/pyproject.toml",
        ".veritas/python-bridge/uv.lock",
    ] {
        fs::copy(fixture.join(relative), root.join(relative)).unwrap();
    }

    let synced = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["sync", "--json", root.to_str().unwrap()])
        .env("GUION_UV", &uv)
        .env("UV_OFFLINE", "1")
        .output()
        .unwrap();
    assert!(
        synced.status.success(),
        "sync failed: {}",
        String::from_utf8_lossy(&synced.stdout)
    );
    assert!(synced.stderr.is_empty());
    let sync_report: serde_json::Value = serde_json::from_slice(&synced.stdout).unwrap();
    assert_eq!(sync_report["kind"], "guion_sync_report");
    assert_eq!(sync_report["schema_version"], 1);
    assert_eq!(sync_report["status"], "succeeded");
    assert_eq!(sync_report["updated"], false);
    assert_eq!(sync_report["lock_sha256"].as_str().unwrap().len(), 64);
    assert_eq!(
        sync_report["bridge_lock_sha256"].as_str().unwrap().len(),
        64
    );
    assert!(root.join("veritas.lock").is_file());
    assert!(root.join(".veritas/python-bridge/uv.lock").is_file());
    let lock_before_run = fs::read(root.join(".veritas/python-bridge/uv.lock")).unwrap();

    let run = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", root.to_str().unwrap()])
        .env("GUION_UV", &uv)
        .env("UV_OFFLINE", "1")
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "run failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "15\n9007199254741000\n3\npython-requests/2.32.5\n"
    );
    assert_eq!(
        fs::read(root.join(".veritas/python-bridge/uv.lock")).unwrap(),
        lock_before_run,
        "locked run must not rewrite uv.lock"
    );

    let _ = fs::remove_dir_all(root);
}
