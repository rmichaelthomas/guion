//! Runs the external, optional comparison harness against the installed binary.
use serde_json::Value;
use std::{path::Path, process::Command};

#[derive(Clone, Debug)]
pub struct Check {
    pub id: String,
    pub status: String,
    pub command: Vec<String>,
    pub exit_status: Option<i32>,
    pub output: String,
}

pub fn run_performance(root: &Path) -> Vec<Check> {
    let (slug, target) = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => ("macos-aarch64", "aarch64-apple-darwin"),
        ("linux", "aarch64") => ("debian-aarch64", "aarch64-unknown-linux-gnu"),
        _ => {
            return vec![Check {
            id: "performance-installed-release".into(),
            status: "incomplete".into(),
            command: vec!["cargo xtask performance --installed".into()],
            exit_status: None,
            output:
                "the final performance evaluation requires one of the supported ARM64 release hosts"
                    .into(),
        }]
        }
    };
    let binary = root.join(format!(
        "target/completion/releases/{slug}/current/bin/guion"
    ));
    let report = root.join("evaluation/completion/results/performance-guion.json");
    let command = vec![
        "python3".into(),
        "evaluation/completion/performance/runner.py".into(),
        "--installed".into(),
        "--binary".into(),
        binary.display().to_string(),
    ];
    if !binary.is_file() {
        return vec![Check {
            id: "performance-installed-release".into(),
            status: "incomplete".into(),
            command,
            exit_status: None,
            output: format!(
                "installed {target} release is missing at {}",
                binary.display()
            ),
        }];
    }
    eprintln!("running 15 paired, randomized trials for each installed-release workload");
    let status = Command::new("python3")
        .current_dir(root)
        .arg(root.join("evaluation/completion/performance/runner.py"))
        .arg("--installed")
        .arg("--binary")
        .arg(&binary)
        .status();
    let status = match status {
        Ok(status) => status,
        Err(error) => {
            return vec![Check {
                id: "performance-correctness-and-confidence".into(),
                status: "failed".into(),
                command,
                exit_status: Some(127),
                output: format!("could not start the external CPython comparison harness: {error}"),
            }]
        }
    };
    let contents = std::fs::read_to_string(&report);
    let report_value: Value = match contents
        .and_then(|text| serde_json::from_str(&text).map_err(std::io::Error::other))
    {
        Ok(value) => value,
        Err(error) => {
            return vec![Check {
                id: "performance-correctness-and-confidence".into(),
                status: "failed".into(),
                command,
                exit_status: status.code(),
                output: format!(
                    "performance harness did not produce valid evidence at {}: {error}",
                    report.display()
                ),
            }]
        }
    };
    let result = report_value
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("failed");
    let wins = report_value
        .pointer("/required_speed_wins/all_required_wins_observed")
        .and_then(Value::as_bool);
    vec![
        Check {
            id: "performance-correctness-and-confidence".into(),
            status: if status.success() && result == "passed" { "passed" } else { "failed" }.into(),
            command: command.clone(),
            exit_status: status.code(),
            output: format!("installed performance report: {}; required speed wins: {}; Python: {}; free-threaded CPython: {}", report.display(), wins.map(|value| value.to_string()).unwrap_or_else(|| "not established".into()), report_value.pointer("/python/version").and_then(Value::as_str).unwrap_or("unknown"), report_value.pointer("/free_threaded_python/status").and_then(Value::as_str).unwrap_or("unverified")),
        },
        Check {
            id: "performance-pinned-cpython".into(),
            status: if report_value.pointer("/python/matches_pinned").and_then(Value::as_bool) == Some(true) { "passed" } else { "failed" }.into(),
            command,
            exit_status: status.code(),
            output: format!("CPython {} (required baseline {})", report_value.pointer("/python/version").and_then(Value::as_str).unwrap_or("unknown"), report_value.pointer("/python/pinned_version").and_then(Value::as_str).unwrap_or("3.14.7")),
        },
    ]
}
