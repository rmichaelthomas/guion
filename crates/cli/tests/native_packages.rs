use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT_TEMP_ROOT: AtomicU64 = AtomicU64::new(0);

struct TempRoot(PathBuf);

impl TempRoot {
    fn new() -> Self {
        let nonce = NEXT_TEMP_ROOT.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "veritas native packages Ω-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn veritas(args: &[&str], current_dir: &std::path::Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_veritas"))
        .args(args)
        .current_dir(current_dir)
        .output()
        .unwrap()
}

#[test]
fn separately_packaged_library_is_added_offline_and_survives_source_removal() {
    let temporary = TempRoot::new();
    let library = temporary.path().join("separate library");
    let packaged = temporary.path().join("published library");
    let project = temporary.path().join("new caller");
    fs::create_dir_all(library.join("src")).unwrap();
    fs::write(
        library.join("veritas.toml"),
        "[package]\nname = \"tiny-math\"\nversion = \"1.0.0\"\nentry = \"src/lib.vr\"\nexports = [\"src/lib.vr\"]\n",
    )
    .unwrap();
    fs::write(
        library.join("src/lib.vr"),
        "pub fn increment(value: Int) -> Int:\n    return value + 1\n",
    )
    .unwrap();

    let output = veritas(
        &[
            "package",
            "--library",
            library.to_str().unwrap(),
            "--output",
            packaged.to_str().unwrap(),
        ],
        temporary.path(),
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    fs::create_dir_all(project.join("src")).unwrap();
    fs::write(
        project.join("veritas.toml"),
        "[project]\nname = \"caller\"\nversion = \"0.1.0\"\nentry = \"src/main.vr\"\n",
    )
    .unwrap();
    fs::write(
        project.join("src/main.vr"),
        "import \"pkg:tiny-math/src/lib.vr\"\n\nfn main() -> Int:\n    print(increment(41))\n    return 0\n",
    )
    .unwrap();
    let added = veritas(
        &["add", "tiny-math", "--path", packaged.to_str().unwrap()],
        &project,
    );
    assert!(
        added.status.success(),
        "{}",
        String::from_utf8_lossy(&added.stderr)
    );

    let lock: toml::Value =
        toml::from_str(&fs::read_to_string(project.join("veritas.lock")).unwrap()).unwrap();
    assert_eq!(lock["format_version"].as_integer(), Some(2));
    assert_eq!(
        lock["native"]["tiny-math"]["version"].as_str(),
        Some("1.0.0")
    );
    assert!(project
        .join(".veritas/packages/tiny-math/1.0.0/src/lib.vr")
        .is_file());
    fs::remove_dir_all(&packaged).unwrap();

    let check = veritas(
        &["check", project.join("src/main.vr").to_str().unwrap()],
        &project,
    );
    assert!(
        check.status.success(),
        "{}",
        String::from_utf8_lossy(&check.stderr)
    );
    let run = veritas(
        &["run", project.join("src/main.vr").to_str().unwrap()],
        &project,
    );
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "42\n");
}

#[test]
fn explicit_update_changes_locked_package_and_rolls_back_a_later_invalid_package() {
    let temporary = TempRoot::new();
    let project = temporary.path().join("update caller");
    let alpha = temporary.path().join("alpha lib");
    let zeta = temporary.path().join("zeta lib");
    fs::create_dir_all(project.join("src")).unwrap();
    fs::write(
        project.join("veritas.toml"),
        "[project]\nname = \"caller\"\nversion = \"0.1.0\"\nentry = \"src/main.vr\"\n",
    )
    .unwrap();
    fs::write(
        project.join("src/main.vr"),
        "fn main() -> Int:\n    return 0\n",
    )
    .unwrap();
    for (root, name, version) in [(&alpha, "alpha-lib", "1.0.0"), (&zeta, "zeta-lib", "1.0.0")] {
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("veritas.toml"),
            format!("[package]\nname = \"{name}\"\nversion = \"{version}\"\nentry = \"src/lib.vr\"\nexports = [\"src/lib.vr\"]\n"),
        )
        .unwrap();
        fs::write(
            root.join("src/lib.vr"),
            "pub fn value() -> Int:\n    return 1\n",
        )
        .unwrap();
        let added = veritas(&["add", name, "--path", root.to_str().unwrap()], &project);
        assert!(
            added.status.success(),
            "{}",
            String::from_utf8_lossy(&added.stderr)
        );
    }

    fs::write(
        alpha.join("veritas.toml"),
        "[package]\nname = \"alpha-lib\"\nversion = \"1.1.0\"\nentry = \"src/lib.vr\"\nexports = [\"src/lib.vr\"]\n",
    )
    .unwrap();
    fs::write(
        alpha.join("src/lib.vr"),
        "pub fn value() -> Int:\n    return 2\n",
    )
    .unwrap();
    let updated = veritas(&["update"], &project);
    assert!(
        updated.status.success(),
        "{}",
        String::from_utf8_lossy(&updated.stderr)
    );
    let lock_path = project.join("veritas.lock");
    let after_update = fs::read(&lock_path).unwrap();
    let lock: toml::Value = toml::from_slice(&after_update).unwrap();
    assert_eq!(
        lock["native"]["alpha-lib"]["version"].as_str(),
        Some("1.1.0")
    );
    assert!(project
        .join(".veritas/packages/alpha-lib/1.1.0/src/lib.vr")
        .is_file());

    fs::write(
        zeta.join("veritas.toml"),
        "[package]\nname = \"zeta-lib\"\nversion = \"1.1.0\"\nentry = \"src/lib.vr\"\nexports = [\"src/missing.vr\"]\n",
    )
    .unwrap();
    let invalid = veritas(&["update"], &project);
    assert!(!invalid.status.success());
    assert_eq!(fs::read(&lock_path).unwrap(), after_update);
    assert!(project
        .join(".veritas/packages/alpha-lib/1.1.0/src/lib.vr")
        .is_file());
    assert!(!project.join(".veritas/packages/zeta-lib/1.1.0").exists());
}

#[test]
fn core_commands_leave_legacy_python_lock_alone_until_explicit_offline_sync() {
    let temporary = TempRoot::new();
    let project = temporary.path().join("migration caller");
    fs::create_dir_all(project.join("src")).unwrap();
    fs::write(
        project.join("veritas.toml"),
        "[project]\nname = \"caller\"\nversion = \"0.1.0\"\nentry = \"src/main.vr\"\n",
    )
    .unwrap();
    fs::write(
        project.join("src/main.vr"),
        "fn main() -> Int:\n    return 0\n",
    )
    .unwrap();
    let legacy_lock = "format_version = 1\nmanifest_sha256 = \"abc\"\npython_version = \"3.14.8\"\ntarget = \"aarch64-apple-darwin\"\nuv_lock_sha256 = \"def\"\nuv_version = \"0.11.18\"\n";
    let lock_path = project.join("veritas.lock");
    fs::write(&lock_path, legacy_lock).unwrap();

    let check = veritas(
        &["check", project.join("src/main.vr").to_str().unwrap()],
        &project,
    );
    assert!(
        check.status.success(),
        "{}",
        String::from_utf8_lossy(&check.stderr)
    );
    assert_eq!(fs::read_to_string(&lock_path).unwrap(), legacy_lock);

    let sync = veritas(&["sync", "--locked", "--offline"], &project);
    assert!(
        sync.status.success(),
        "{}",
        String::from_utf8_lossy(&sync.stderr)
    );
    let migrated: toml::Value = toml::from_str(&fs::read_to_string(&lock_path).unwrap()).unwrap();
    assert_eq!(migrated["format_version"].as_integer(), Some(2));
    assert_eq!(
        migrated["python"]["python_version"].as_str(),
        Some("3.14.8")
    );
    assert_eq!(migrated["python"]["uv_lock_sha256"].as_str(), Some("def"));
    assert_eq!(migrated["native"].as_table().unwrap().len(), 0);

    let failed_project = temporary.path().join("failed migration");
    fs::create_dir_all(failed_project.join("src")).unwrap();
    fs::write(
        failed_project.join("veritas.toml"),
        "[project]\nname = \"failed\"\nversion = \"0.1.0\"\nentry = \"src/main.vr\"\n\n[dependencies.missing]\nversion = \"1.0.0\"\npath = \"../missing\"\n",
    )
    .unwrap();
    fs::write(
        failed_project.join("src/main.vr"),
        "fn main() -> Int:\n    return 0\n",
    )
    .unwrap();
    let failed_lock = failed_project.join("veritas.lock");
    fs::write(&failed_lock, legacy_lock).unwrap();
    let sync = veritas(&["sync", "--locked", "--offline"], &failed_project);
    assert!(!sync.status.success());
    assert_eq!(fs::read_to_string(failed_lock).unwrap(), legacy_lock);
}

#[test]
fn package_url_requires_https_and_an_exact_sha256() {
    let temporary = TempRoot::new();
    let project = temporary.path().join("url caller");
    fs::create_dir_all(project.join("src")).unwrap();
    fs::write(
        project.join("veritas.toml"),
        "[project]\nname = \"caller\"\nversion = \"0.1.0\"\nentry = \"src/main.vr\"\n",
    )
    .unwrap();
    fs::write(
        project.join("src/main.vr"),
        "fn main() -> Int:\n    return 0\n",
    )
    .unwrap();
    let insecure = veritas(
        &[
            "add",
            "tiny",
            "--url",
            "http://example.invalid/tiny.tar",
            "--version",
            "1.0.0",
            "--sha256",
            &"0".repeat(64),
        ],
        &project,
    );
    assert!(!insecure.status.success());
    assert!(String::from_utf8_lossy(&insecure.stderr).contains("HTTPS"));
    let invalid_hash = veritas(
        &[
            "add",
            "tiny",
            "--url",
            "https://example.invalid/tiny.tar",
            "--version",
            "1.0.0",
            "--sha256",
            "bad",
        ],
        &project,
    );
    assert!(!invalid_hash.status.success());
    assert!(String::from_utf8_lossy(&invalid_hash.stderr).contains("64 hexadecimal"));
}

#[test]
fn core_doctor_ignores_configured_python_and_reports_interop_disabled() {
    let output = Command::new(env!("CARGO_BIN_EXE_veritas"))
        .args(["doctor", "--json"])
        .env("VERITAS_PYTHON", "/definitely/missing/python3")
        .output()
        .unwrap();
    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["python_interop"].as_str(), Some("disabled"));
    assert!(String::from_utf8_lossy(&output.stdout).contains("optional"));
}

#[test]
fn core_check_works_with_python_and_uv_unavailable_on_path() {
    let temporary = TempRoot::new();
    let project = temporary.path().join("python-free caller");
    fs::create_dir_all(project.join("src")).unwrap();
    fs::create_dir_all(temporary.path().join("empty-path")).unwrap();
    fs::write(
        project.join("veritas.toml"),
        "[project]\nname = \"caller\"\nversion = \"0.1.0\"\nentry = \"src/main.vr\"\n",
    )
    .unwrap();
    fs::write(
        project.join("src/main.vr"),
        "fn main() -> Int:\n    return 0\n",
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_veritas"))
        .args(["check", project.join("src/main.vr").to_str().unwrap()])
        .current_dir(&project)
        .env("PATH", temporary.path().join("empty-path"))
        .env("VERITAS_PYTHON", "/definitely/missing/python3")
        .env("VERITAS_UV", "/definitely/missing/uv")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!project.join(".veritas").exists());
}

#[test]
fn package_add_resolves_transitive_exact_dependencies_and_rejects_cycles() {
    let temporary = TempRoot::new();
    let project = temporary.path().join("transitive caller");
    let base = temporary.path().join("base library");
    let top = temporary.path().join("top library");
    fs::create_dir_all(base.join("src")).unwrap();
    fs::write(
        base.join("veritas.toml"),
        "[package]\nname = \"base-lib\"\nversion = \"1.0.0\"\nentry = \"src/lib.vr\"\nexports = [\"src/lib.vr\"]\n",
    )
    .unwrap();
    fs::write(
        base.join("src/lib.vr"),
        "pub fn base_value() -> Int:\n    return 40\n",
    )
    .unwrap();
    fs::create_dir_all(top.join("src")).unwrap();
    fs::write(
        top.join("veritas.toml"),
        format!(
            "[package]\nname = \"top-lib\"\nversion = \"1.0.0\"\nentry = \"src/lib.vr\"\nexports = [\"src/lib.vr\"]\n\n[dependencies.base-lib]\nversion = \"1.0.0\"\npath = {:?}\n",
            "../base library"
        ),
    )
    .unwrap();
    fs::write(
        top.join("src/lib.vr"),
        "import \"pkg:base-lib/src/lib.vr\"\npub fn answer() -> Int:\n    return base_value() + 2\n",
    )
    .unwrap();
    fs::create_dir_all(project.join("src")).unwrap();
    fs::write(
        project.join("veritas.toml"),
        "[project]\nname = \"caller\"\nversion = \"0.1.0\"\nentry = \"src/main.vr\"\n",
    )
    .unwrap();
    fs::write(
        project.join("src/main.vr"),
        "import \"pkg:top-lib/src/lib.vr\"\nfn main() -> Int:\n    print(answer())\n    return 0\n",
    )
    .unwrap();
    let added = veritas(
        &["add", "top-lib", "--path", top.to_str().unwrap()],
        &project,
    );
    assert!(
        added.status.success(),
        "{}",
        String::from_utf8_lossy(&added.stderr)
    );
    let lock: toml::Value =
        toml::from_str(&fs::read_to_string(project.join("veritas.lock")).unwrap()).unwrap();
    assert_eq!(
        lock["native"]["base-lib"]["version"].as_str(),
        Some("1.0.0")
    );
    assert_eq!(
        lock["native"]["top-lib"]["dependencies"]["base-lib"]["version"].as_str(),
        Some("1.0.0")
    );
    let run = veritas(
        &["run", project.join("src/main.vr").to_str().unwrap()],
        &project,
    );
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "42\n");

    fs::write(
        base.join("veritas.toml"),
        "[package]\nname = \"base-lib\"\nversion = \"1.0.0\"\nentry = \"src/lib.vr\"\nexports = [\"src/lib.vr\"]\n\n[dependencies.top-lib]\nversion = \"1.0.0\"\npath = \"../top library\"\n",
    )
    .unwrap();
    let cycle_project = temporary.path().join("cycle caller");
    fs::create_dir_all(cycle_project.join("src")).unwrap();
    fs::write(
        cycle_project.join("veritas.toml"),
        "[project]\nname = \"cycle\"\nversion = \"0.1.0\"\nentry = \"src/main.vr\"\n",
    )
    .unwrap();
    fs::write(
        cycle_project.join("src/main.vr"),
        "fn main() -> Int:\n    return 0\n",
    )
    .unwrap();
    let cycle = veritas(
        &["add", "top-lib", "--path", top.to_str().unwrap()],
        &cycle_project,
    );
    assert!(!cycle.status.success());
    assert!(String::from_utf8_lossy(&cycle.stderr).contains("cycle"));
    assert!(!cycle_project.join("veritas.lock").exists());
    assert!(!cycle_project.join(".veritas/packages").exists());
}
