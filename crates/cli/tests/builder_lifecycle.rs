use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT_ROOT: AtomicU64 = AtomicU64::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let nonce = NEXT_ROOT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "veritas lifecycle Ω-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn copy_tree(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let kind = entry.file_type().unwrap();
        if kind.is_dir() {
            copy_tree(&entry.path(), &destination.join(entry.file_name()));
        } else if kind.is_file() {
            fs::copy(entry.path(), destination.join(entry.file_name())).unwrap();
        }
    }
}

fn prepare_installed_core(root: &Path, clean_path: &Path) -> PathBuf {
    let bin = root.join("bin");
    let shared = root.join("share/veritas");
    fs::create_dir_all(&bin).unwrap();
    fs::create_dir_all(&shared).unwrap();
    fs::copy(env!("CARGO_BIN_EXE_veritas"), bin.join("veritas")).unwrap();
    let stdlib = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../language/stdlib");
    copy_tree(&stdlib, &shared.join("stdlib"));
    fs::create_dir_all(clean_path).unwrap();
    for tool in ["dirname", "readelf", "ldd"] {
        let candidate = Path::new("/usr/bin").join(tool);
        if candidate.exists() {
            #[cfg(unix)]
            std::os::unix::fs::symlink(candidate, clean_path.join(tool)).unwrap();
        }
    }
    bin.join("veritas")
}

fn invoke(veritas: &Path, args: &[&str], directory: &Path, path: &Path) -> Output {
    Command::new(veritas)
        .args(args)
        .current_dir(directory)
        .env("PATH", path)
        .env_remove("VERITAS_STDLIB_ROOT")
        .env_remove("VERITAS_PYTHON")
        .env_remove("VERITAS_UV")
        .env_remove("VERITAS_PYTHON_INSTALL_DIR")
        .env_remove("PYTHONHOME")
        .env_remove("VIRTUAL_ENV")
        .output()
        .unwrap()
}

fn success(output: Output) -> String {
    assert!(
        output.status.success(),
        "exit={:?}\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn installed_core_completes_new_repair_format_lint_build_package_install_and_run() {
    let fixture = Fixture::new();
    let clean_path = fixture.path().join("clean PATH");
    let installed_root = fixture.path().join("installed Veritas Ω");
    let veritas = prepare_installed_core(&installed_root, &clean_path);
    let project = fixture.path().join("Veritas Ω project with spaces");

    success(invoke(
        &veritas,
        &["new", project.to_str().unwrap()],
        fixture.path(),
        &clean_path,
    ));
    let project_guide = fs::read_to_string(project.join("README.md")).unwrap();
    assert!(project_guide.contains("veritas check"));

    let library = fixture.path().join("greeting library");
    fs::create_dir_all(library.join("src")).unwrap();
    fs::write(
        library.join("veritas.toml"),
        "[package]\nname = \"greeting\"\nversion = \"1.0.0\"\nentry = \"src/lib.vr\"\nexports = [\"src/lib.vr\"]\n",
    )
    .unwrap();
    fs::write(
        library.join("src/lib.vr"),
        "pub fn greeting(name: Text) -> Text:\n    return \"hello \" + name\n",
    )
    .unwrap();
    success(invoke(
        &veritas,
        &["add", "greeting", "--path", library.to_str().unwrap()],
        &project,
        &clean_path,
    ));
    success(invoke(
        &veritas,
        &["sync", "--locked", "--offline"],
        &project,
        &clean_path,
    ));
    success(invoke(&veritas, &["update"], &project, &clean_path));

    let source_path = project.join("src/main.vr");
    let broken = "import \"pkg:greeting/src/lib.vr\"\n\nfn main() -> Int:\n    print(no_such_function())\n    return 0\n\nfn test_starter() -> Unit:\n    assert(1 + 1 == 2)\n";
    fs::write(&source_path, broken).unwrap();
    let rejected = invoke(
        &veritas,
        &["check", "--json", source_path.to_str().unwrap()],
        &project,
        &clean_path,
    );
    assert!(!rejected.status.success());
    let diagnostic: Value = serde_json::from_slice(&rejected.stdout).unwrap();
    assert_eq!(diagnostic["diagnostic"]["code"], "E_NAME_UNKNOWN");
    let lint_rejected = invoke(
        &veritas,
        &["lint", "--json", source_path.to_str().unwrap()],
        &project,
        &clean_path,
    );
    assert!(!lint_rejected.status.success());
    let lint_diagnostic: Value = serde_json::from_slice(&lint_rejected.stdout).unwrap();
    assert_eq!(lint_diagnostic["diagnostic"]["code"], "E_NAME_UNKNOWN");

    let source = "import \"pkg:greeting/src/lib.vr\"\nimport \"unused.vr\"\n\nfn main()   ->   Int: # preserve comment  tail  \n    let abandoned   = \"  literal  # text \"\n    print( greeting(\"Veritas\") )\n    return   0\n    print(\"unreachable\")\n\nfn test_starter() -> Unit:\n    assert(1 + 1 == 2)\n";
    let unused = project.join("src/unused.vr");
    fs::write(unused, "pub fn unused() -> Int:\n    return 1\n").unwrap();
    fs::write(&source_path, source).unwrap();
    let lint = invoke(
        &veritas,
        &["lint", "--json", source_path.to_str().unwrap()],
        &project,
        &clean_path,
    );
    assert!(
        lint.status.success(),
        "{}",
        String::from_utf8_lossy(&lint.stderr)
    );
    let lint_report: Value = serde_json::from_slice(&lint.stdout).unwrap();
    let codes = lint_report["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["code"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(codes.contains(&"W_UNUSED_IMPORT"), "{codes:?}");
    assert!(codes.contains(&"W_UNUSED_LOCAL"), "{codes:?}");
    assert!(codes.contains(&"W_UNREACHABLE_STATEMENT"), "{codes:?}");

    let unchanged = fs::read(&source_path).unwrap();
    let fmt_check = invoke(
        &veritas,
        &["fmt", "--check", source_path.to_str().unwrap()],
        &project,
        &clean_path,
    );
    assert!(!fmt_check.status.success());
    assert_eq!(fs::read(&source_path).unwrap(), unchanged);
    success(invoke(
        &veritas,
        &["fmt", source_path.to_str().unwrap()],
        &project,
        &clean_path,
    ));
    success(invoke(
        &veritas,
        &["fmt", "--check", source_path.to_str().unwrap()],
        &project,
        &clean_path,
    ));
    let formatted = fs::read_to_string(&source_path).unwrap();
    assert!(formatted.contains("\"  literal  # text \""));
    assert!(formatted.contains("# preserve comment  tail  "));
    let clean_source = "import \"pkg:greeting/src/lib.vr\"\n\nfn main() -> Int:\n    print(greeting(\"Veritas\"))\n    return 0\n\nfn test_starter() -> Unit:\n    assert(1 + 1 == 2)\n";
    fs::write(&source_path, clean_source).unwrap();
    let clean_lint = invoke(
        &veritas,
        &["lint", source_path.to_str().unwrap()],
        &project,
        &clean_path,
    );
    assert!(success(clean_lint).contains("no warnings"));
    success(invoke(
        &veritas,
        &["check", project.to_str().unwrap()],
        &project,
        &clean_path,
    ));
    let run = invoke(
        &veritas,
        &["run", project.to_str().unwrap()],
        &project,
        &clean_path,
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "hello Veritas\n");
    assert!(success(invoke(
        &veritas,
        &["test", project.to_str().unwrap()],
        &project,
        &clean_path
    ))
    .contains("ok test_starter"));
    let api: Value = serde_json::from_slice(
        &invoke(
            &veritas,
            &["doc", "--json", source_path.to_str().unwrap()],
            &project,
            &clean_path,
        )
        .stdout,
    )
    .unwrap();
    let greeting = api["functions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|function| function["name"] == "greeting")
        .unwrap();
    assert_eq!(greeting["parameters"][0]["name"], "name");
    assert!(api["standard_library"]["available_modules"]
        .as_array()
        .is_some());
    let capabilities: Value = serde_json::from_slice(
        &invoke(
            &veritas,
            &["inspect", "--json", source_path.to_str().unwrap()],
            &project,
            &clean_path,
        )
        .stdout,
    )
    .unwrap();
    assert_eq!(capabilities["schema_version"], 1);
    let doctor: Value = serde_json::from_slice(
        &invoke(&veritas, &["doctor", "--json"], &project, &clean_path).stdout,
    )
    .unwrap();
    assert_eq!(doctor["python_interop"], "disabled");

    success(invoke(
        &veritas,
        &["build", "--json", source_path.to_str().unwrap()],
        &project,
        &clean_path,
    ));
    let package = fixture.path().join("deliverable app Ω");
    success(invoke(
        &veritas,
        &[
            "package",
            project.to_str().unwrap(),
            "--output",
            package.to_str().unwrap(),
        ],
        &project,
        &clean_path,
    ));
    success(invoke(
        &veritas,
        &["verify-package", package.to_str().unwrap()],
        &project,
        &clean_path,
    ));
    let prefix = fixture.path().join("install prefix with spaces");
    success(invoke(
        &veritas,
        &[
            "install",
            package.to_str().unwrap(),
            "--prefix",
            prefix.to_str().unwrap(),
        ],
        &project,
        &clean_path,
    ));
    let metadata: Value =
        serde_json::from_slice(&fs::read(package.join("PACKAGE.json")).unwrap()).unwrap();
    let app_id = metadata["application"]["id"].as_str().unwrap();
    let launched = Command::new(prefix.join("bin").join(app_id))
        .current_dir(fixture.path())
        .env("PATH", &clean_path)
        .env_remove("VERITAS_PYTHON")
        .env_remove("VERITAS_UV")
        .output()
        .unwrap();
    assert!(
        launched.status.success(),
        "{}",
        String::from_utf8_lossy(&launched.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&launched.stdout), "hello Veritas\n");
    let installed_command = prefix.join("bin").join(app_id);
    fs::write(&installed_command, "keep unrelated command\n").unwrap();
    let protected = invoke(
        &veritas,
        &[
            "install",
            package.to_str().unwrap(),
            "--prefix",
            prefix.to_str().unwrap(),
        ],
        &project,
        &clean_path,
    );
    assert!(!protected.status.success());
    assert_eq!(
        fs::read_to_string(installed_command).unwrap(),
        "keep unrelated command\n"
    );

    let examples = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../language/examples");
    for example in [
        "sum.vr",
        "records.vr",
        "matching.vr",
        "option.vr",
        "modules_main.vr",
    ] {
        let example = examples.join(example);
        success(invoke(
            &veritas,
            &["check", example.to_str().unwrap()],
            fixture.path(),
            &clean_path,
        ));
    }
}
