use crate::builtin_catalog;
use crate::completion::{audit_closeout, validate_completion_schema};
use crate::inventory::validate_inventory;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::Instant;

#[derive(Clone)]
struct CheckResult {
    id: String,
    status: String,
    command: Vec<String>,
    exit_status: Option<i32>,
    output: String,
}

struct Invocation {
    report_name: String,
    command_name: String,
    task: Option<u64>,
    args: Vec<String>,
    checks: Vec<CheckResult>,
    explicit_status: Option<String>,
    message: Option<String>,
    requested_targets: Vec<String>,
    audit_details: Option<Value>,
    exit_status: i32,
}

fn read_json(root: &Path, relative: &str) -> Result<Value, String> {
    let path = root.join(relative);
    let contents =
        fs::read_to_string(&path).map_err(|error| format!("read {}: {error}", path.display()))?;
    serde_json::from_str(&contents).map_err(|error| format!("parse {}: {error}", path.display()))
}

fn run_output(
    root: &Path,
    program: &str,
    args: &[String],
    env_offline: bool,
) -> Result<Output, String> {
    let mut command = Command::new(program);
    command.current_dir(root).args(args);
    if env_offline {
        command.env("CARGO_NET_OFFLINE", "true");
        command.env("CARGO_NET_RETRY", "0");
    }
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("start {program}: {error}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or("child stdout pipe was not available")?;
    let stderr = child
        .stderr
        .take()
        .ok_or("child stderr pipe was not available")?;
    let stdout_reader = thread::spawn(move || forward_child_output(stdout, false));
    let stderr_reader = thread::spawn(move || forward_child_output(stderr, true));
    let status = child
        .wait()
        .map_err(|error| format!("wait for {program}: {error}"))?;
    let stdout = stdout_reader
        .join()
        .map_err(|_| format!("capture {program} stdout thread panicked"))?;
    let stderr = stderr_reader
        .join()
        .map_err(|_| format!("capture {program} stderr thread panicked"))?;
    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

fn forward_child_output(mut reader: impl Read, to_stderr: bool) -> Vec<u8> {
    let mut captured = Vec::new();
    let mut buffer = [0; 8192];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(count) => {
                let bytes = &buffer[..count];
                captured.extend_from_slice(bytes);
                if to_stderr {
                    let mut output = io::stderr().lock();
                    let _ = output.write_all(bytes);
                    let _ = output.flush();
                } else {
                    let mut output = io::stdout().lock();
                    let _ = output.write_all(bytes);
                    let _ = output.flush();
                }
            }
        }
    }
    captured
}

fn git_value(root: &Path, args: &[&str]) -> String {
    Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".into())
}

fn digest_entries<I>(root: &Path, entries: I) -> String
where
    I: IntoIterator<Item = PathBuf>,
{
    let mut paths: Vec<PathBuf> = entries.into_iter().collect();
    paths.sort();
    paths.dedup();
    let mut digest = Sha256::new();
    for relative in paths {
        let text = relative.to_string_lossy();
        digest.update((text.len() as u64).to_le_bytes());
        digest.update(text.as_bytes());
        let full = root.join(&relative);
        match fs::File::open(&full) {
            Ok(mut file) => {
                let mut buffer = [0u8; 64 * 1024];
                loop {
                    match file.read(&mut buffer) {
                        Ok(0) => break,
                        Ok(count) => digest.update(&buffer[..count]),
                        Err(error) => {
                            digest.update(format!("READ_ERROR:{error}").as_bytes());
                            break;
                        }
                    }
                }
            }
            Err(error) => digest.update(format!("MISSING:{error}").as_bytes()),
        }
        digest.update([0]);
    }
    format!("sha256:{:x}", digest.finalize())
}

fn changed_paths(root: &Path) -> Vec<PathBuf> {
    let mut paths = BTreeSet::new();
    for args in [
        ["diff", "HEAD", "--name-only", "-z"].as_slice(),
        ["ls-files", "--others", "--exclude-standard", "-z"].as_slice(),
    ] {
        if let Ok(output) = Command::new("git").current_dir(root).args(args).output() {
            if output.status.success() {
                for raw in output
                    .stdout
                    .split(|byte| *byte == 0)
                    .filter(|raw| !raw.is_empty())
                {
                    paths.insert(PathBuf::from(String::from_utf8_lossy(raw).into_owned()));
                }
            }
        }
    }
    paths
        .into_iter()
        .filter(|path| crate::snapshot::include_in_product_snapshot(path))
        .collect()
}

fn directory_digest(root: &Path, relative: &str) -> Option<String> {
    let path = root.join(relative);
    if !path.exists() {
        return None;
    }
    let mut files = Vec::new();
    let mut pending = vec![path.clone()];
    while let Some(directory) = pending.pop() {
        let Ok(entries) = fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let entry_path = entry.path();
            if entry_path.is_dir() {
                pending.push(entry_path);
            } else if entry_path.is_file() {
                if let Ok(relative_path) = entry_path.strip_prefix(root) {
                    files.push(relative_path.to_path_buf());
                }
            }
        }
    }
    Some(digest_entries(root, files))
}

fn file_digest(root: &Path, relative: &str) -> Option<String> {
    let bytes = fs::read(root.join(relative)).ok()?;
    Some(format!("sha256:{:x}", Sha256::digest(bytes)))
}

fn target_name() -> String {
    target_name_for(std::env::consts::OS, std::env::consts::ARCH)
}

fn target_name_for(os: &str, arch: &str) -> String {
    match (os, arch) {
        ("macos", "aarch64") => "macos-aarch64".into(),
        ("linux", "aarch64") => "debian-aarch64".into(),
        ("linux", "x86_64") => "debian-x86_64".into(),
        (os, arch) => format!("{os}-{arch}"),
    }
}

fn report_path(name: &str) -> String {
    format!("evaluation/completion/results/{name}.json")
}

fn task_for(command: &str, group: Option<&str>) -> Option<u64> {
    match command {
        "conformance" => Some(2),
        "bootstrap" => Some(3),
        "stdlib" if group == Some("data") => Some(4),
        "stdlib" if group == Some("services") => Some(5),
        "runtime" => Some(6),
        "product" if group == Some("dependencies") => Some(7),
        "product" if group == Some("lifecycle") => Some(8),
        "product" if group == Some("applications") => Some(9),
        // Verification commands depend on the preceding implementation task;
        // requiring their own task to be marked complete would block its evidence.
        "release" | "performance" => Some(9),
        "builders" => Some(10),
        _ => None,
    }
}

fn invocation(command: &str, args: Vec<String>) -> Result<Invocation, String> {
    let group = args
        .windows(2)
        .find(|pair| pair[0] == "--group")
        .map(|pair| pair[1].as_str());
    let report_name: String = match command {
        "conformance" if args.iter().any(|arg| arg == "--inventory-only") => {
            "conformance-inventory".into()
        }
        "conformance" => "conformance".into(),
        "bootstrap" => "bootstrap".into(),
        "stdlib" => match group {
            Some("data") => "stdlib-data",
            Some("services") => "stdlib-services",
            _ => return Err("usage: cargo xtask stdlib --group data|services".into()),
        }
        .into(),
        "runtime" => "runtime".into(),
        "product" => match group {
            Some("dependencies") => "product-dependencies",
            Some("lifecycle") => "product-lifecycle",
            Some("applications") => "product-applications",
            _ => {
                return Err(
                    "usage: cargo xtask product --group dependencies|lifecycle|applications".into(),
                )
            }
        }
        .into(),
        "release" => "release".into(),
        "performance" => "performance".into(),
        "builders" => "builders".into(),
        "closeout" if args.iter().any(|arg| arg == "--check-schema") => "closeout-schema".into(),
        "closeout" => "closeout".into(),
        _ => return Err(format!("unknown xtask command `{command}`")),
    };
    Ok(Invocation {
        report_name: report_name.clone(),
        command_name: command.to_string(),
        task: if report_name == "conformance-inventory" {
            None
        } else {
            task_for(command, group)
        },
        args,
        checks: Vec::new(),
        explicit_status: None,
        message: None,
        requested_targets: Vec::new(),
        audit_details: None,
        exit_status: 0,
    })
}

fn check_result(id: impl Into<String>, command: Vec<String>, output: Output) -> CheckResult {
    let status = if output.status.success() {
        "passed"
    } else {
        "failed"
    };
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stderr.is_empty() {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&stderr);
    }
    CheckResult {
        id: id.into(),
        status: status.into(),
        command,
        exit_status: output.status.code(),
        output: text,
    }
}

fn append_cargo_check(root: &Path, invocation: &mut Invocation, id: &str, argv: &[String]) {
    let mut args = vec!["--locked".to_string(), "--offline".to_string()];
    args.extend_from_slice(argv);
    let started = Instant::now();
    eprintln!("running check {id}: cargo {}", args.join(" "));
    match run_output(root, "cargo", &args, true) {
        Ok(output) => {
            let check = check_result(
                id,
                std::iter::once("cargo".to_string()).chain(args).collect(),
                output,
            );
            if check.status == "failed" {
                invocation.exit_status = 1;
            }
            eprintln!(
                "check {id}: {} after {:.1}s",
                check.status,
                started.elapsed().as_secs_f64()
            );
            invocation.checks.push(check);
        }
        Err(error) => {
            eprintln!(
                "check {id}: failed to start after {:.1}s",
                started.elapsed().as_secs_f64()
            );
            invocation.checks.push(CheckResult {
                id: id.into(),
                status: "failed".into(),
                command: std::iter::once("cargo".to_string()).chain(args).collect(),
                exit_status: Some(127),
                output: error,
            });
            invocation.exit_status = 1;
        }
    }
}

fn expected_task_checks(root: &Path, invocation: &mut Invocation) -> Result<(), String> {
    let command_name = invocation.command_name.clone();
    let command = command_name.as_str();
    let group = invocation
        .args
        .windows(2)
        .find(|pair| pair[0] == "--group")
        .map(|pair| pair[1].as_str());
    let mut targets: Vec<(String, Vec<String>, String)> = match command {
        "conformance" if invocation.report_name == "conformance-inventory" => return Ok(()),
        "conformance" => vec![
            (
                "compiler-conformance-current-cases".into(),
                vec![
                    "test".into(),
                    "-p".into(),
                    "guion-cli".into(),
                    "--test".into(),
                    "compiler_conformance".into(),
                ],
                "crates/cli/tests/compiler_conformance.rs".into(),
            ),
            (
                "full-language-conformance".into(),
                vec![
                    "test".into(),
                    "-p".into(),
                    "guion-cli".into(),
                    "--test".into(),
                    "language_conformance".into(),
                    "--".into(),
                    "--skip".into(),
                    "compiler_stages_compile_full_task_two_corpus_and_locked_roots".into(),
                    "--test-threads=1".into(),
                ],
                "crates/cli/tests/language_conformance.rs".into(),
            ),
        ],
        "bootstrap" => vec![
            (
                "compiler-seed-regeneration".into(),
                vec![
                    "run".into(),
                    "-p".into(),
                    "guion-bootstrap".into(),
                    "--bin".into(),
                    "guion-stage0".into(),
                    "--".into(),
                    "seed".into(),
                    ".".into(),
                ],
                "crates/bootstrap/src/main.rs".into(),
            ),
            (
                "compiler-stage-equality".into(),
                vec![
                    "test".into(),
                    "-p".into(),
                    "guion-cli".into(),
                    "--test".into(),
                    "bootstrap".into(),
                ],
                "crates/cli/tests/bootstrap.rs".into(),
            ),
            (
                "default-cli-embedded-selfhost".into(),
                vec![
                    "test".into(),
                    "-p".into(),
                    "guion-cli".into(),
                    "--test".into(),
                    "compiler_driver".into(),
                ],
                "crates/cli/tests/compiler_driver.rs".into(),
            ),
        ],
        "stdlib" if group == Some("data") => vec![
            (
                "stdlib-data".into(),
                vec![
                    "test".into(),
                    "-p".into(),
                    "guion-runtime".into(),
                    "--test".into(),
                    "stdlib_data".into(),
                ],
                "crates/runtime/tests/stdlib_data.rs".into(),
            ),
            (
                "stdlib-data-embedded-selfhost".into(),
                vec![
                    "test".into(),
                    "-p".into(),
                    "guion-cli".into(),
                    "--test".into(),
                    "stdlib_data".into(),
                ],
                "crates/cli/tests/stdlib_data.rs".into(),
            ),
        ],
        "stdlib" if group == Some("services") => vec![(
            "stdlib-services".into(),
            vec![
                "test".into(),
                "-p".into(),
                "guion-runtime".into(),
                "--test".into(),
                "stdlib_services".into(),
            ],
            "crates/runtime/tests/stdlib_services.rs".into(),
        )],
        "runtime" => vec![
            (
                "runtime-execution".into(),
                vec![
                    "test".into(),
                    "-p".into(),
                    "guion-runtime".into(),
                    "--test".into(),
                    "execution".into(),
                ],
                "crates/runtime/tests/execution.rs".into(),
            ),
            (
                "runtime-http".into(),
                vec![
                    "test".into(),
                    "-p".into(),
                    "guion-runtime".into(),
                    "--test".into(),
                    "http".into(),
                ],
                "crates/runtime/tests/http.rs".into(),
            ),
            (
                "runtime-lifecycle".into(),
                vec![
                    "test".into(),
                    "-p".into(),
                    "guion-runtime".into(),
                    "--test".into(),
                    "lifecycle".into(),
                ],
                "crates/runtime/tests/lifecycle.rs".into(),
            ),
        ],
        "product" if group == Some("dependencies") => vec![(
            "native-packages".into(),
            vec![
                "test".into(),
                "-p".into(),
                "guion-cli".into(),
                "--test".into(),
                "native_packages".into(),
            ],
            "crates/cli/tests/native_packages.rs".into(),
        )],
        "product" if group == Some("lifecycle") => vec![(
            "builder-lifecycle".into(),
            vec![
                "test".into(),
                "-p".into(),
                "guion-cli".into(),
                "--test".into(),
                "builder_lifecycle".into(),
            ],
            "crates/cli/tests/builder_lifecycle.rs".into(),
        )],
        "product" if group == Some("applications") => vec![(
            "all-nine-applications-initial-and-follow-up".into(),
            vec![
                "test".into(),
                "-p".into(),
                "guion-cli".into(),
                "--test".into(),
                "application_acceptance".into(),
            ],
            "crates/cli/tests/application_acceptance.rs".into(),
        )],
        "release" | "performance" => Vec::new(),
        "builders" => Vec::new(),
        _ => Vec::new(),
    };
    if command == "conformance"
        && invocation
            .args
            .windows(2)
            .any(|pair| pair[0] == "--compilers" && pair[1] == "stage2,stage3")
    {
        targets.push((
            "stage2-stage3-full-corpus".into(),
            vec![
                "test".into(),
                "-p".into(),
                "guion-cli".into(),
                "--test".into(),
                "language_conformance".into(),
                "compiler_stages_compile_full_task_two_corpus_and_locked_roots".into(),
                "--".into(),
                "--exact".into(),
                "--nocapture".into(),
            ],
            "crates/cli/tests/language_conformance.rs".into(),
        ));
    }
    for (id, argv, source) in targets {
        if !root.join(&source).is_file() {
            invocation.checks.push(CheckResult {
                id,
                status: "incomplete".into(),
                command: std::iter::once("cargo".to_string()).chain(argv).collect(),
                exit_status: None,
                output: format!("required check source is missing: {source}"),
            });
            invocation.exit_status = 1;
        } else {
            append_cargo_check(root, invocation, &id, &argv);
        }
    }
    if command == "product"
        && invocation
            .args
            .windows(2)
            .any(|pair| pair[0] == "--group" && pair[1] == "dependencies")
    {
        let args = vec![
            "tree".into(),
            "--locked".into(),
            "--offline".into(),
            "-p".into(),
            "guion-cli".into(),
            "--edges".into(),
            "normal,build,features".into(),
        ];
        let command_line = std::iter::once("cargo".to_string())
            .chain(args.iter().cloned())
            .collect::<Vec<_>>();
        match run_output(root, "cargo", &args, true) {
            Ok(output) => {
                let text = format!(
                    "{}{}{}",
                    String::from_utf8_lossy(&output.stdout),
                    if output.stdout.is_empty() { "" } else { "\n" },
                    String::from_utf8_lossy(&output.stderr)
                );
                let clean = output.status.success()
                    && !text.contains("guion-compiler")
                    && !text.contains("python-interop");
                if !clean {
                    invocation.exit_status = 1;
                }
                invocation.checks.push(CheckResult {
                    id: "python-free-core-product".into(),
                    status: if clean { "passed" } else { "failed" }.into(),
                    command: command_line,
                    exit_status: output.status.code(),
                    output: text,
                });
            }
            Err(error) => {
                invocation.exit_status = 1;
                invocation.checks.push(CheckResult {
                    id: "python-free-core-product".into(),
                    status: "failed".into(),
                    command: command_line,
                    exit_status: Some(127),
                    output: error,
                });
            }
        }
    }
    if command == "bootstrap" {
        let args = vec![
            "tree".into(),
            "--locked".into(),
            "--offline".into(),
            "-p".into(),
            "guion-cli".into(),
            "--edges".into(),
            "normal,build".into(),
        ];
        let command_line = std::iter::once("cargo".to_string())
            .chain(args.iter().cloned())
            .collect::<Vec<_>>();
        match run_output(root, "cargo", &args, true) {
            Ok(output) => {
                let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
                let stderr = String::from_utf8_lossy(&output.stderr);
                if !stderr.is_empty() {
                    if !text.is_empty() {
                        text.push('\n');
                    }
                    text.push_str(&stderr);
                }
                let clean = output.status.success() && !text.contains("guion-compiler");
                if !clean {
                    invocation.exit_status = 1;
                }
                invocation.checks.push(CheckResult {
                    id: "default-cli-dependency-graph".into(),
                    status: if clean { "passed" } else { "failed" }.into(),
                    command: command_line,
                    exit_status: output.status.code(),
                    output: text,
                });
            }
            Err(error) => {
                invocation.exit_status = 1;
                invocation.checks.push(CheckResult {
                    id: "default-cli-dependency-graph".into(),
                    status: "failed".into(),
                    command: command_line,
                    exit_status: Some(127),
                    output: error,
                });
            }
        }
    }
    Ok(())
}

fn requested_targets(command: &str, args: &[String]) -> Vec<String> {
    let Some(index) = args.iter().position(|arg| arg == "--target") else {
        return if matches!(
            command,
            "conformance"
                | "bootstrap"
                | "stdlib"
                | "runtime"
                | "product"
                | "release"
                | "performance"
                | "builders"
        ) {
            vec![target_name()]
        } else {
            Vec::new()
        };
    };
    match args.get(index + 1).map(String::as_str) {
        Some("both") => vec![
            "macos-aarch64".into(),
            "debian-aarch64".into(),
            "debian-x86_64".into(),
        ],
        Some(target) => vec![target.to_string()],
        None => Vec::new(),
    }
}

fn completed_tasks(root: &Path) -> Result<BTreeSet<u64>, String> {
    let status = read_json(root, "evaluation/completion/status.json")?;
    let tasks = status
        .get("completed_tasks")
        .and_then(Value::as_array)
        .ok_or("completed_tasks must be an array")?;
    Ok(tasks.iter().filter_map(Value::as_u64).collect())
}

fn build_report(root: &Path, invocation: &mut Invocation) -> Result<Value, String> {
    let status_file = read_json(root, "evaluation/completion/status.json")?;
    if invocation.requested_targets.is_empty() {
        invocation.requested_targets = vec![target_name()];
    }

    let inventory_missing = if invocation.report_name == "conformance-inventory" {
        validate_inventory(root)?
    } else {
        Vec::new()
    };
    if !inventory_missing.is_empty() {
        invocation.checks.push(CheckResult {
            id: "inventory-complete".into(),
            status: "failed".into(),
            command: vec![
                "cargo".into(),
                "xtask".into(),
                "conformance".into(),
                "--inventory-only".into(),
            ],
            exit_status: Some(1),
            output: inventory_missing.join("\n"),
        });
        invocation.exit_status = 1;
    } else if invocation.report_name == "conformance-inventory" {
        invocation.checks.push(CheckResult {
            id: "inventory-complete".into(),
            status: "passed".into(),
            command: vec![
                "cargo".into(),
                "xtask".into(),
                "conformance".into(),
                "--inventory-only".into(),
            ],
            exit_status: Some(0),
            output: "all current Type, Op, host API and CLI entries are mapped".into(),
        });
    }

    let tasks = completed_tasks(root)?;
    if let Some(task) = invocation.task {
        if !tasks.contains(&task) {
            invocation.message.get_or_insert_with(|| {
                format!("task {task} is not marked complete in evaluation/completion/status.json")
            });
            if invocation.exit_status == 0 {
                invocation.exit_status = 1;
            }
        }
    }
    let completed_targets: Vec<String> = invocation
        .requested_targets
        .iter()
        .filter(|target| **target == target_name())
        .cloned()
        .collect();
    let skipped_targets: Vec<String> = invocation
        .requested_targets
        .iter()
        .filter(|target| !completed_targets.contains(target))
        .cloned()
        .collect();
    let check_values: Vec<Value> = invocation
        .checks
        .iter()
        .map(|check| {
            json!({
                "id": check.id,
                "status": check.status,
                "command": check.command,
                "exit_status": check.exit_status,
                "output": check.output,
            })
        })
        .collect();
    let failed_check = invocation
        .checks
        .iter()
        .any(|check| check.status == "failed");
    let incomplete_check = invocation
        .checks
        .iter()
        .any(|check| check.status == "incomplete");
    let explicit_status = invocation.explicit_status.as_deref();
    let status = if explicit_status == Some("incomplete")
        || !skipped_targets.is_empty()
        || incomplete_check
        || invocation.exit_status != 0
    {
        if failed_check {
            "failed"
        } else {
            "incomplete"
        }
    } else if failed_check {
        "failed"
    } else {
        "passed"
    };

    let mut artifacts = BTreeMap::new();
    artifacts.insert(
        "compiler_seed",
        file_digest(root, "language/self_host/compiler.gub"),
    );
    artifacts.insert(
        "compiler_seed_manifest",
        file_digest(root, "language/self_host/compiler-seed.json"),
    );
    artifacts.insert(
        "artifact_schema",
        file_digest(root, "language/artifact-format.md"),
    );
    artifacts.insert("stdlib", directory_digest(root, "language/stdlib"));
    artifacts.insert("runtime", directory_digest(root, "crates/runtime"));
    let changed = changed_paths(root);
    let changed_path_count = changed.len();
    let working_tree_digest = digest_entries(root, changed);
    Ok(json!({
        "schema_version": 1,
        "command": invocation.command_name,
        "report": invocation.report_name,
        "status": status,
        "message": invocation.message,
        "source_revision": git_value(root, &["rev-parse", "HEAD"]),
        "working_tree_digest": working_tree_digest,
        "changed_path_count": changed_path_count,
        "target": target_name(),
        "exact_command": std::iter::once("cargo xtask".to_string()).chain(std::iter::once(invocation.command_name.clone())).chain(invocation.args.clone()).collect::<Vec<_>>().join(" "),
        "exit_status": invocation.exit_status,
        "checks": check_values,
        "required_targets": invocation.requested_targets,
        "completed_targets": completed_targets,
        "skipped_targets": skipped_targets,
        "seed_hash": artifacts.get("compiler_seed").and_then(Clone::clone),
        "stdlib_hash": artifacts.get("stdlib").and_then(Clone::clone),
        "runtime_hash": artifacts.get("runtime").and_then(Clone::clone),
        "artifact_identities": artifacts,
        "missing_evidence": invocation.audit_details.as_ref().and_then(|details| details.get("missing_evidence")).cloned().unwrap_or_else(|| json!([])),
        "missing_checks": invocation.audit_details.as_ref().and_then(|details| details.get("missing_checks")).cloned().unwrap_or_else(|| json!([])),
        "unsatisfied_goal_ids": invocation.audit_details.as_ref().and_then(|details| details.get("unsatisfied_goal_ids")).cloned().unwrap_or_else(|| status_file.get("unsatisfied_goal_ids").cloned().unwrap_or_else(|| json!([]))),
    }))
}

fn target_report_path(root: &Path, name: &str, target: &str) -> PathBuf {
    root.join("evaluation/completion/results/target-runs")
        .join(format!("{name}-{target}.json"))
}

fn write_target_report(
    root: &Path,
    name: &str,
    target: &str,
    report: &Value,
) -> Result<(), String> {
    let path = target_report_path(root, name, target);
    let parent = path
        .parent()
        .ok_or_else(|| format!("target report path has no parent: {}", path.display()))?;
    fs::create_dir_all(parent).map_err(|error| format!("create {}: {error}", parent.display()))?;
    let temporary = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(report)
        .map_err(|error| format!("serialize target report: {error}"))?;
    fs::write(&temporary, bytes)
        .map_err(|error| format!("write {}: {error}", temporary.display()))?;
    fs::rename(&temporary, &path).map_err(|error| format!("replace {}: {error}", path.display()))
}

fn same_source_snapshot(left: &Value, right: &Value) -> bool {
    [
        "source_revision",
        "working_tree_digest",
        "seed_hash",
        "stdlib_hash",
        "runtime_hash",
        "required_targets",
        "exact_command",
    ]
    .iter()
    .all(|field| left.get(field) == right.get(field))
}

fn aggregate_target_reports(root: &Path, name: &str, report: Value) -> Result<Value, String> {
    let required_targets = report
        .get("required_targets")
        .and_then(Value::as_array)
        .ok_or("target report required_targets must be an array")?;
    if required_targets.len() < 2 {
        return Ok(report);
    }

    let current_target = report
        .get("target")
        .and_then(Value::as_str)
        .ok_or("target report target must be a string")?;
    write_target_report(root, name, current_target, &report)?;

    let required: Vec<String> = required_targets
        .iter()
        .map(|target| {
            target
                .as_str()
                .map(str::to_string)
                .ok_or("required target entries must be strings")
        })
        .collect::<Result<_, _>>()?;

    let mut reports = Vec::new();
    let mut stale_target = false;
    for target in &required {
        let path = target_report_path(root, name, target);
        let Ok(contents) = fs::read_to_string(&path) else {
            continue;
        };
        let candidate: Value = serde_json::from_str(&contents)
            .map_err(|error| format!("parse {}: {error}", path.display()))?;
        if candidate.get("target").and_then(Value::as_str) != Some(target) {
            stale_target = true;
            continue;
        }
        if !same_source_snapshot(&report, &candidate) {
            stale_target = true;
            continue;
        }
        reports.push((target.clone(), candidate));
    }

    let completed: Vec<String> = required
        .iter()
        .filter(|target| reports.iter().any(|(present, _)| present == *target))
        .cloned()
        .collect();
    let skipped: Vec<String> = required
        .iter()
        .filter(|target| !completed.contains(target))
        .cloned()
        .collect();

    let mut checks = Vec::new();
    let mut report_messages = Vec::new();
    let mut failed = false;
    let mut incomplete = false;
    for (target, target_report) in &reports {
        if target_report.get("status").and_then(Value::as_str) == Some("failed") {
            failed = true;
        }
        if let Some(message) = target_report.get("message").and_then(Value::as_str) {
            if !message.is_empty() {
                report_messages.push(format!("{target}: {message}"));
            }
        }
        let target_checks = target_report
            .get("checks")
            .and_then(Value::as_array)
            .ok_or("target report checks must be an array")?;
        for check in target_checks {
            let status = check.get("status").and_then(Value::as_str).unwrap_or("");
            failed |= status == "failed";
            incomplete |= status != "passed" && status != "failed";
            let mut check = check.clone();
            if let Some(object) = check.as_object_mut() {
                object.insert("target".into(), Value::String(target.clone()));
            }
            checks.push(check);
        }
    }

    let all_targets_present = skipped.is_empty() && reports.len() == required.len();
    let all_checks_passed = !checks.is_empty()
        && checks
            .iter()
            .all(|check| check.get("status").and_then(Value::as_str) == Some("passed"));
    let status = if failed {
        "failed"
    } else if all_targets_present
        && all_checks_passed
        && !incomplete
        && report_messages.is_empty()
        && !stale_target
    {
        "passed"
    } else {
        "incomplete"
    };

    let mut aggregate = report;
    aggregate["target"] = Value::String("both".into());
    aggregate["completed_targets"] = serde_json::json!(completed);
    aggregate["skipped_targets"] = serde_json::json!(skipped);
    aggregate["checks"] = Value::Array(checks);
    aggregate["status"] = Value::String(status.into());
    aggregate["exit_status"] = serde_json::json!(if status == "passed" { 0 } else { 1 });
    aggregate["message"] = if stale_target {
        Value::String(
            "target reports use different source snapshots; rerun every required target command"
                .into(),
        )
    } else if !report_messages.is_empty() {
        Value::String(report_messages.join("; "))
    } else if !all_targets_present {
        Value::String("waiting for matching evidence from every required target".into())
    } else {
        Value::Null
    };
    Ok(aggregate)
}

fn write_report(root: &Path, name: &str, report: &Value) -> Result<(), String> {
    let relative = report_path(name);
    let path = root.join(relative);
    let parent = path
        .parent()
        .ok_or_else(|| format!("report path has no parent: {}", path.display()))?;
    fs::create_dir_all(parent).map_err(|error| format!("create {}: {error}", parent.display()))?;
    let temporary = path.with_extension("json.tmp");
    let bytes =
        serde_json::to_vec_pretty(report).map_err(|error| format!("serialize report: {error}"))?;
    fs::write(&temporary, bytes)
        .map_err(|error| format!("write {}: {error}", temporary.display()))?;
    fs::rename(&temporary, &path)
        .map_err(|error| format!("replace {}: {error}", path.display()))?;
    Ok(())
}

fn print_json(value: &Value) -> Result<(), String> {
    println!(
        "{}",
        serde_json::to_string_pretty(value).map_err(|error| format!("format output: {error}"))?
    );
    Ok(())
}

pub fn run(root: &Path, args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("usage: cargo xtask <conformance|bootstrap|stdlib|runtime|product|release|performance|builders|closeout|agent-reference> [options]");
        return 2;
    }
    let command = args[0].as_str();
    if command == "catalog" {
        let result = if args.iter().any(|arg| arg == "--write") {
            builtin_catalog::write(root)
        } else {
            builtin_catalog::validate(root)
        };
        return match result {
            Ok(()) => {
                println!("builtin catalog is valid");
                0
            }
            Err(error) => {
                eprintln!("{error}");
                1
            }
        };
    }
    if command == "agent-reference" {
        let result = if args.iter().any(|arg| arg == "--write") {
            crate::agent_reference::write(root)
        } else {
            crate::agent_reference::check(root)
        };
        return match result {
            Ok(()) => {
                println!("agent reference is current");
                0
            }
            Err(error) => {
                eprintln!("{error}");
                1
            }
        };
    }
    let mut invocation = match invocation(command, args[1..].to_vec()) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("{error}");
            return 2;
        }
    };
    invocation.requested_targets = requested_targets(command, &invocation.args);
    if invocation.report_name == "conformance-inventory"
        || invocation.report_name == "closeout-schema"
        || command == "closeout"
    {
        invocation.requested_targets.clear();
    }

    if invocation.report_name == "closeout-schema" {
        match validate_completion_schema(root) {
            Ok(()) => {
                invocation.explicit_status = Some("incomplete".into());
                invocation.message =
                    Some("report schemas are valid; product closeout remains incomplete".into());
                invocation.checks.push(CheckResult {
                    id: "completion-schema".into(),
                    status: "passed".into(),
                    command: vec![
                        "cargo".into(),
                        "xtask".into(),
                        "closeout".into(),
                        "--check-schema".into(),
                    ],
                    exit_status: Some(0),
                    output: "status and existing report schemas are valid".into(),
                });
            }
            Err(error) => {
                invocation.explicit_status = Some("failed".into());
                invocation.exit_status = 1;
                invocation.message = Some(error.clone());
                invocation.checks.push(CheckResult {
                    id: "completion-schema".into(),
                    status: "failed".into(),
                    command: vec![
                        "cargo".into(),
                        "xtask".into(),
                        "closeout".into(),
                        "--check-schema".into(),
                    ],
                    exit_status: Some(1),
                    output: error,
                });
            }
        }
    } else if command == "closeout" {
        match audit_closeout(root) {
            Ok(audit) => {
                invocation.explicit_status = Some(audit.status.clone());
                invocation.exit_status = audit.exit_code;
                invocation.message = Some(format!("missing evidence: {}; missing checks: {}; skipped targets: {}; unsatisfied goals: {}", audit.missing_evidence.len(), audit.missing_checks.len(), audit.skipped_targets.len(), audit.unsatisfied_goal_ids.join(", ")));
                invocation.audit_details = Some(
                    json!({"missing_evidence": audit.missing_evidence, "missing_checks": audit.missing_checks, "skipped_targets": audit.skipped_targets, "unsatisfied_goal_ids": audit.unsatisfied_goal_ids}),
                );
                invocation.checks.push(CheckResult {
                    id: "closeout-audit".into(),
                    status: audit.status.clone(),
                    command: vec!["cargo".into(), "xtask".into(), "closeout".into()],
                    exit_status: Some(audit.exit_code),
                    output: serde_json::to_string(invocation.audit_details.as_ref().unwrap())
                        .unwrap_or_default(),
                });
            }
            Err(error) => {
                invocation.explicit_status = Some("failed".into());
                invocation.exit_status = 1;
                invocation.message = Some(error.clone());
                invocation.checks.push(CheckResult {
                    id: "closeout-audit".into(),
                    status: "failed".into(),
                    command: vec!["cargo".into(), "xtask".into(), "closeout".into()],
                    exit_status: Some(1),
                    output: error,
                });
            }
        }
    } else if invocation.report_name == "conformance-inventory" {
        // Inventory-only is intentionally self-contained and does not claim Task 2 completion.
        if let Err(error) = builtin_catalog::validate(root) {
            invocation.explicit_status = Some("failed".into());
            invocation.exit_status = 1;
            invocation.message = Some(error.clone());
            invocation.checks.push(CheckResult {
                id: "builtin-signature-catalog".into(),
                status: "failed".into(),
                command: vec![
                    "cargo".into(),
                    "xtask".into(),
                    "catalog".into(),
                    "--check".into(),
                ],
                exit_status: Some(1),
                output: error,
            });
        } else {
            invocation.checks.push(CheckResult {
                id: "builtin-signature-catalog".into(),
                status: "passed".into(),
                command: vec![
                    "cargo".into(),
                    "xtask".into(),
                    "catalog".into(),
                    "--check".into(),
                ],
                exit_status: Some(0),
                output:
                    "declarative builtin signatures validate and generated Guion catalog is current"
                        .into(),
            });
        }
    } else {
        if command == "conformance" {
            if let Err(error) = builtin_catalog::validate(root) {
                invocation.explicit_status = Some("failed".into());
                invocation.exit_status = 1;
                invocation.message = Some(error.clone());
                invocation.checks.push(CheckResult {
                    id: "builtin-signature-catalog".into(),
                    status: "failed".into(),
                    command: vec![
                        "cargo".into(),
                        "xtask".into(),
                        "catalog".into(),
                        "--check".into(),
                    ],
                    exit_status: Some(1),
                    output: error,
                });
            }
        }
        let manifest = match read_json(root, "language/conformance/manifest.json") {
            Ok(value) => value,
            Err(error) => {
                eprintln!("{error}");
                return 2;
            }
        };
        if let Some(checks) = manifest.get("existing_checks").and_then(Value::as_array) {
            let product_group = invocation
                .args
                .windows(2)
                .find(|pair| pair[0] == "--group")
                .map(|pair| pair[1].clone());
            let group = match command {
                "conformance" => Some("conformance"),
                "bootstrap" => Some("bootstrap"),
                "runtime" => Some("runtime"),
                "product" => product_group.as_deref(),
                _ => None,
            };
            for check in checks.iter().filter(|check| {
                let check_group = check.get("group").and_then(Value::as_str);
                group.is_some_and(|group| check_group == Some(group))
            }) {
                if let (Some(id), Some(argv)) = (
                    check.get("id").and_then(Value::as_str),
                    check.get("argv").and_then(Value::as_array),
                ) {
                    let args: Vec<String> = argv
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect();
                    append_cargo_check(root, &mut invocation, id, &args);
                }
            }
        }
        if let Err(error) = expected_task_checks(root, &mut invocation) {
            invocation.explicit_status = Some("failed".into());
            invocation.exit_status = 1;
            invocation.message = Some(error);
        }
        if command == "release" {
            for item in crate::release::run_release(root, &invocation.args) {
                if item.status != "passed" {
                    invocation.exit_status = 1;
                }
                invocation.checks.push(CheckResult {
                    id: item.id,
                    status: item.status,
                    command: item.command,
                    exit_status: item.exit_status,
                    output: item.output,
                });
            }
        }
        if command == "performance" {
            for item in crate::performance::run_performance(root) {
                if item.status != "passed" {
                    invocation.exit_status = 1;
                }
                invocation.checks.push(CheckResult {
                    id: item.id,
                    status: item.status,
                    command: item.command,
                    exit_status: item.exit_status,
                    output: item.output,
                });
            }
        }
        if command == "builders" {
            match crate::builders::audit(root) {
                Ok(classes) => invocation.checks.push(CheckResult {
                    id: "fresh-builders-three-classes".into(),
                    status: "passed".into(),
                    command: vec!["cargo".into(), "xtask".into(), "builders".into()],
                    exit_status: Some(0),
                    output: classes.join("; "),
                }),
                Err(error) => {
                    invocation.explicit_status = Some("incomplete".into());
                    invocation.message = Some(error.clone());
                    invocation.exit_status = 1;
                    invocation.checks.push(CheckResult {
                        id: "fresh-builders-three-classes".into(),
                        status: "incomplete".into(),
                        command: vec!["cargo".into(), "xtask".into(), "builders".into()],
                        exit_status: None,
                        output: error,
                    });
                }
            }
        }
    }

    let report = match build_report(root, &mut invocation) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("{error}");
            return 2;
        }
    };
    let report = match aggregate_target_reports(root, &invocation.report_name, report) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("{error}");
            return 2;
        }
    };
    if let Err(error) = write_report(root, &invocation.report_name, &report) {
        eprintln!("{error}");
        return 2;
    }
    if let Err(error) = print_json(&report) {
        eprintln!("{error}");
        return 2;
    }
    invocation.exit_status
}

#[cfg(test)]
mod target_report_tests {
    use super::{aggregate_target_reports, requested_targets, target_name_for, task_for};
    use serde_json::json;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn final_gates_require_the_prior_task_before_marking_their_own_task_complete() {
        assert_eq!(task_for("release", None), Some(9));
        assert_eq!(task_for("performance", None), Some(9));
        assert_eq!(task_for("builders", None), Some(10));
    }

    #[test]
    fn names_x86_64_linux_as_the_debian_release_target() {
        assert_eq!(target_name_for("linux", "x86_64"), "debian-x86_64");
    }

    #[test]
    fn both_target_selection_includes_all_three_release_platforms() {
        assert_eq!(
            requested_targets("runtime", &["--target".into(), "both".into()]),
            ["macos-aarch64", "debian-aarch64", "debian-x86_64"]
        );
    }

    fn report(target: &str) -> serde_json::Value {
        json!({
            "schema_version": 1,
            "status": "incomplete",
            "message": null,
            "source_revision": "abc123",
            "working_tree_digest": "sha256:source",
            "seed_hash": "sha256:seed",
            "stdlib_hash": "sha256:stdlib",
            "runtime_hash": "sha256:runtime",
            "exact_command": "cargo xtask product --group applications --target both",
            "target": target,
            "required_targets": ["macos-aarch64", "debian-aarch64", "debian-x86_64"],
            "completed_targets": [target],
            "skipped_targets": match target {
                "macos-aarch64" => ["debian-aarch64", "debian-x86_64"],
                "debian-aarch64" => ["macos-aarch64", "debian-x86_64"],
                _ => ["macos-aarch64", "debian-aarch64"]
            },
            "checks": [{
                "id": "application-acceptance",
                "status": "passed",
                "command": ["cargo", "test"],
                "exit_status": 0,
                "output": "passed"
            }]
        })
    }

    #[test]
    fn aggregates_only_matching_passed_reports_for_every_target() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("guion-target-report-{nonce}"));
        std::fs::create_dir_all(&root).unwrap();

        let first = aggregate_target_reports(&root, "sample", report("macos-aarch64")).unwrap();
        assert_eq!(first["status"], "incomplete");
        assert_eq!(first["completed_targets"], json!(["macos-aarch64"]));
        assert_eq!(
            first["skipped_targets"],
            json!(["debian-aarch64", "debian-x86_64"])
        );

        let second = aggregate_target_reports(&root, "sample", report("debian-aarch64")).unwrap();
        assert_eq!(second["status"], "incomplete");
        assert_eq!(
            second["completed_targets"],
            json!(["macos-aarch64", "debian-aarch64"])
        );
        let combined = aggregate_target_reports(&root, "sample", report("debian-x86_64")).unwrap();
        assert_eq!(combined["status"], "passed");
        assert_eq!(combined["exit_status"], 0);
        assert_eq!(
            combined["completed_targets"],
            json!(["macos-aarch64", "debian-aarch64", "debian-x86_64"])
        );
        assert_eq!(combined["skipped_targets"], json!([]));
        let checks = combined["checks"].as_array().unwrap();
        assert_eq!(checks.len(), 3);
        assert!(checks
            .iter()
            .any(|check| check["target"] == "macos-aarch64"));
        assert!(checks
            .iter()
            .any(|check| check["target"] == "debian-aarch64"));
        assert!(checks
            .iter()
            .any(|check| check["target"] == "debian-x86_64"));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn does_not_combine_reports_from_different_source_snapshots() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("guion-stale-target-report-{nonce}"));
        std::fs::create_dir_all(&root).unwrap();

        let first = aggregate_target_reports(&root, "sample", report("macos-aarch64")).unwrap();
        let mut changed = report("debian-x86_64");
        changed["working_tree_digest"] = json!("sha256:different");
        let combined = aggregate_target_reports(&root, "sample", changed).unwrap();

        assert_eq!(first["status"], "incomplete");
        assert_eq!(combined["status"], "incomplete");
        assert_eq!(
            combined["message"],
            "target reports use different source snapshots; rerun every required target command"
        );
        assert_eq!(combined["completed_targets"], json!(["debian-x86_64"]));
        assert_eq!(
            combined["skipped_targets"],
            json!(["macos-aarch64", "debian-aarch64"])
        );

        std::fs::remove_dir_all(root).unwrap();
    }
}
