use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

const APPLICATIONS: [&str; 9] = [
    "sla-report",
    "jsonl-summary",
    "release-notes",
    "calendar-conflicts",
    "webhook-replay",
    "health-digest",
    "config-migrator",
    "doc-index",
    "lockfile-auditor",
];

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("CLI crate is nested in the repository")
        .to_path_buf()
}

struct RemoveDirOnDrop(PathBuf);

impl Drop for RemoveDirOnDrop {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn copy_fixture_tree(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        if entry.file_name() == "dist" {
            continue;
        }
        let from = entry.path();
        let to = destination.join(entry.file_name());
        let kind = entry.file_type().unwrap();
        if kind.is_dir() {
            copy_fixture_tree(&from, &to);
        } else if kind.is_file() {
            fs::copy(&from, &to).unwrap();
        } else {
            panic!("unsupported frozen application entry: {}", from.display());
        }
    }
}

fn run(command: &mut Command, description: &str) -> std::process::Output {
    let output = command
        .output()
        .unwrap_or_else(|error| panic!("could not run {description}: {error}"));
    assert!(
        output.status.success(),
        "{description} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

#[test]
fn all_nine_applications_pass_initial_and_follow_up_from_relocated_packages() {
    let root = repository_root();
    let cli = PathBuf::from(env!("CARGO_BIN_EXE_guion"));
    let source_applications_root = std::env::var_os("GUION_APPLICATIONS_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("evaluation/completion/applications"));
    let scratch = std::env::temp_dir().join(format!(
        "guion-application-acceptance-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&scratch);
    let _cleanup = RemoveDirOnDrop(scratch.clone());
    let applications_root = scratch.join("applications");
    for name in APPLICATIONS {
        copy_fixture_tree(
            &source_applications_root.join(name),
            &applications_root.join(name),
        );
    }
    let applications_root = applications_root.canonicalize().unwrap();

    for name in APPLICATIONS {
        let application = applications_root.join(name);
        let source = fs::read_to_string(application.join("main.vr"))
            .unwrap_or_else(|error| panic!("cannot read {name} source: {error}"));
        assert!(
            !source.contains("extern python"),
            "{name} must not call the optional Python bridge"
        );
        let manifest = fs::read_to_string(application.join("veritas.toml"))
            .unwrap_or_else(|error| panic!("cannot read {name} manifest: {error}"));
        assert!(
            !manifest.contains("[python]"),
            "{name} must not declare a managed Python environment"
        );

        let package = application.join("dist").join(name);
        let package_metadata_path = package.join("PACKAGE.json");
        let package_time = package_metadata_path
            .metadata()
            .and_then(|metadata| metadata.modified())
            .ok();
        let lock_path = application.join("veritas.lock");
        let lock_is_native =
            fs::read_to_string(&lock_path).is_ok_and(|lock| lock.contains("format_version = 2"));
        let source_files = ["main.vr", "veritas.toml", "delivery.json"];
        let needs_rebuild = package_time.is_none()
            || !lock_is_native
            || source_files.iter().any(|relative| {
                application
                    .join(relative)
                    .metadata()
                    .and_then(|metadata| metadata.modified())
                    .is_ok_and(|modified| Some(modified) > package_time)
            });
        if needs_rebuild {
            run(
                Command::new(&cli)
                    .args(["sync", "--locked", "--offline"])
                    .current_dir(&application),
                &format!("offline lock validation for {name}"),
            );
            let lock = fs::read_to_string(&lock_path).unwrap_or_else(|error| {
                panic!("{name} did not write legacy veritas.lock: {error}")
            });
            assert!(
                lock.contains("format_version = 2"),
                "{name} lock must be native"
            );
            if package.exists() {
                fs::remove_dir_all(&package)
                    .unwrap_or_else(|error| panic!("cannot refresh {name} package: {error}"));
            }
            run(
                Command::new(&cli)
                    .args([
                        "package",
                        application.to_str().expect("Unicode application path"),
                        "--output",
                        package.to_str().expect("Unicode package path"),
                    ])
                    .current_dir(&root),
                &format!("package build for {name}"),
            );
        }
        let lock = fs::read_to_string(&lock_path)
            .unwrap_or_else(|error| panic!("{name} did not retain legacy veritas.lock: {error}"));
        assert!(
            lock.contains("format_version = 2"),
            "{name} lock must be native"
        );
        run(
            Command::new(&cli)
                .args([
                    "verify-package",
                    package.to_str().expect("Unicode package path"),
                ])
                .current_dir(&root),
            &format!("package verification for {name}"),
        );
        let metadata: Value = serde_json::from_slice(
            &fs::read(package_metadata_path).expect("package metadata exists"),
        )
        .expect("package metadata is valid JSON");
        assert!(
            metadata["python_runtime"].is_null(),
            "{name} package embeds Python"
        );
        assert!(
            package.join("app.gub").is_file(),
            "{name} has no compiled artifact"
        );
        assert!(
            package.join("run").is_file(),
            "{name} has no package launcher"
        );
    }

    let runner = root.join("evaluation/completion/run_applications.py");
    let reports_root = std::env::var_os("GUION_APPLICATION_RESULTS")
        .map(PathBuf::from)
        .unwrap_or_else(|| scratch.join("reports"));
    let output = run(
        Command::new("python3")
            .arg(runner)
            .args(["--timeout", "120", "--applications-root"])
            .arg(&applications_root)
            .args(["--output"])
            .arg(&reports_root)
            .current_dir(&root),
        "nine-application initial and follow-up matrix",
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    for name in APPLICATIONS {
        assert!(
            stdout.contains(&format!("\"application\": \"{name}\"")),
            "application runner did not report {name}: {stdout}"
        );
        let report_path = reports_root.join(format!("{name}.json"));
        let report: Value = serde_json::from_slice(
            &fs::read(&report_path)
                .unwrap_or_else(|error| panic!("missing {name} report: {error}")),
        )
        .unwrap_or_else(|error| panic!("invalid {name} report: {error}"));
        assert_eq!(report["passed"], true, "{name} application cases failed");
        assert_eq!(report["mode"], "source-free relocated package");
        let generated_path = reports_root.join(format!("{name}-generated.json"));
        let generated: Value = serde_json::from_slice(
            &fs::read(&generated_path)
                .unwrap_or_else(|error| panic!("missing {name} generated report: {error}")),
        )
        .unwrap_or_else(|error| panic!("invalid {name} generated report: {error}"));
        assert_eq!(generated["passed"], true, "{name} generated cases failed");
        assert_eq!(generated["seed"], 20261024);
        assert_eq!(generated["sizes"], serde_json::json!([24, 128]));
        assert_eq!(
            generated["mode"],
            "seeded independent-oracle cases against relocated package"
        );
        if name == "webhook-replay" {
            assert_eq!(generated["concurrency"]["passed"], true);
            assert_eq!(generated["concurrency"]["round_count"], 3);
        }
    }
}
