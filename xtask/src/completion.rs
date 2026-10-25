use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloseoutAudit {
    pub status: String,
    pub exit_code: i32,
    pub missing_evidence: Vec<String>,
    pub missing_checks: Vec<String>,
    pub skipped_targets: Vec<String>,
    pub unsatisfied_goal_ids: Vec<String>,
}

fn parse_json(path: &Path) -> Result<Value, String> {
    let contents =
        fs::read_to_string(path).map_err(|error| format!("read {}: {error}", path.display()))?;
    serde_json::from_str(&contents).map_err(|error| format!("parse {}: {error}", path.display()))
}

fn relative_path(value: &str) -> Result<PathBuf, String> {
    let path = Path::new(value);
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(format!("unsafe or empty evidence path: {value}"));
    }
    Ok(path.to_path_buf())
}

fn optional_string_list(value: Option<&Value>, label: &str) -> Result<Vec<String>, String> {
    match value {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(value) => string_list(Some(value), label),
    }
}

fn string_list(value: Option<&Value>, label: &str) -> Result<Vec<String>, String> {
    let values = value
        .and_then(Value::as_array)
        .ok_or_else(|| format!("{label} must be an array"))?;
    values
        .iter()
        .map(|item| {
            item.as_str()
                .map(str::to_string)
                .ok_or_else(|| format!("{label} entries must be strings"))
        })
        .collect()
}

fn task_id_list(value: Option<&Value>, label: &str) -> Result<Vec<u64>, String> {
    let values = value
        .and_then(Value::as_array)
        .ok_or_else(|| format!("{label} must be an array"))?;
    values
        .iter()
        .map(|item| {
            item.as_u64()
                .filter(|id| *id > 0)
                .ok_or_else(|| format!("{label} entries must be positive integers"))
        })
        .collect()
}

fn valid_sha256(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|digest| {
        digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
    })
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
        match fs::read(root.join(&relative)) {
            Ok(contents) => digest.update(contents),
            Err(error) => digest.update(format!("READ_ERROR:{error}").as_bytes()),
        }
        digest.update([0]);
    }
    format!("sha256:{:x}", digest.finalize())
}

fn file_digest(root: &Path, relative: &str) -> Option<String> {
    let contents = fs::read(root.join(relative)).ok()?;
    Some(format!("sha256:{:x}", Sha256::digest(contents)))
}

fn directory_digest(root: &Path, relative: &str) -> Option<String> {
    let path = root.join(relative);
    if !path.exists() {
        return None;
    }
    let mut files = Vec::new();
    let mut pending = vec![path];
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

fn current_working_tree_digest(root: &Path) -> Option<String> {
    let mut paths = BTreeSet::new();
    for args in [
        ["diff", "HEAD", "--name-only", "-z"].as_slice(),
        ["ls-files", "--others", "--exclude-standard", "-z"].as_slice(),
    ] {
        let output = Command::new("git")
            .current_dir(root)
            .args(args)
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        for raw in output
            .stdout
            .split(|byte| *byte == 0)
            .filter(|raw| !raw.is_empty())
        {
            let path = PathBuf::from(String::from_utf8_lossy(raw).into_owned());
            if crate::snapshot::include_in_product_snapshot(&path) {
                paths.insert(path);
            }
        }
    }
    Some(digest_entries(root, paths))
}

fn validate_final_artifact_snapshot(
    root: &Path,
    required_targets: &[String],
) -> Result<(), String> {
    let release_path = "evaluation/completion/results/release.json";
    let performance_path = "evaluation/completion/results/performance.json";
    let release = parse_json(&root.join(release_path))?;
    let performance = parse_json(&root.join(performance_path))?;
    for (label, report) in [("release", &release), ("performance", &performance)] {
        if report.get("status").and_then(Value::as_str) != Some("passed") {
            return Err(format!("{label} report is not passed"));
        }
    }
    let source_digest = release
        .get("working_tree_digest")
        .and_then(Value::as_str)
        .filter(|value| valid_sha256(value))
        .ok_or("release report has no valid source snapshot digest")?;
    if performance
        .get("working_tree_digest")
        .and_then(Value::as_str)
        != Some(source_digest)
        || release.get("source_revision") != performance.get("source_revision")
    {
        return Err("release and performance reports use different source snapshots".into());
    }
    if let Some(current_digest) = current_working_tree_digest(root) {
        if current_digest != source_digest {
            return Err(
                "release and performance evidence is stale for the current source tree".into(),
            );
        }
    }

    let released_targets = release
        .get("completed_targets")
        .and_then(Value::as_array)
        .ok_or("release report has no completed target list")?;
    let mut actual_targets = released_targets
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect::<Vec<_>>();
    let mut expected_targets = required_targets.to_vec();
    actual_targets.sort();
    actual_targets.dedup();
    expected_targets.sort();
    expected_targets.dedup();
    if actual_targets != expected_targets {
        return Err("release report does not cover every required target".into());
    }

    let required_identities = [
        "artifact_schema",
        "compiler_seed",
        "compiler_seed_manifest",
        "runtime",
        "stdlib",
    ];
    for field in required_identities {
        let release_hash = release
            .get("artifact_identities")
            .and_then(|value| value.get(field))
            .and_then(Value::as_str)
            .filter(|value| valid_sha256(value))
            .ok_or_else(|| format!("release report has no valid {field} hash"))?;
        let performance_hash = performance
            .get("artifact_identities")
            .and_then(|value| value.get(field))
            .and_then(Value::as_str)
            .filter(|value| valid_sha256(value))
            .ok_or_else(|| format!("performance report has no valid {field} hash"))?;
        if release_hash != performance_hash {
            return Err(format!("release and performance {field} hashes differ"));
        }
        let actual = match field {
            "artifact_schema" => file_digest(root, "language/artifact-format.md"),
            "compiler_seed" => file_digest(root, "language/self_host/compiler.gub"),
            "compiler_seed_manifest" => file_digest(root, "language/self_host/compiler-seed.json"),
            "runtime" => directory_digest(root, "crates/runtime"),
            "stdlib" => directory_digest(root, "language/stdlib"),
            _ => None,
        }
        .ok_or_else(|| format!("current {field} source is missing"))?;
        if release_hash != actual {
            return Err(format!("release {field} hash is stale for current source"));
        }
    }
    for (report_name, report) in [("release", &release), ("performance", &performance)] {
        for (field, identity) in [
            ("seed_hash", "compiler_seed"),
            ("runtime_hash", "runtime"),
            ("stdlib_hash", "stdlib"),
        ] {
            if report.get(field)
                != report
                    .get("artifact_identities")
                    .and_then(|value| value.get(identity))
            {
                return Err(format!(
                    "{report_name} {field} does not match its artifact identity"
                ));
            }
        }
    }
    Ok(())
}

pub fn validate_completion_schema(root: &Path) -> Result<(), String> {
    let status_path = root.join("evaluation/completion/status.json");
    let status = parse_json(&status_path)?;
    if status.get("schema_version").and_then(Value::as_u64) != Some(1) {
        return Err("status.json schema_version must be 1".into());
    }
    for field in [
        "required_evidence",
        "evidence_paths",
        "required_targets",
        "target_evidence_paths",
        "required_check_ids",
        "unsatisfied_goal_ids",
    ] {
        string_list(status.get(field), field)?;
    }
    task_id_list(status.get("completed_tasks"), "completed_tasks")?;
    if status.get("current_task").and_then(Value::as_u64).is_none() {
        return Err("current_task must be a positive integer".into());
    }
    for field in [
        "required_evidence",
        "evidence_paths",
        "target_evidence_paths",
    ] {
        for value in string_list(status.get(field), field)? {
            relative_path(&value)?;
        }
    }
    if status
        .get("source_state")
        .and_then(Value::as_object)
        .is_none()
    {
        return Err("source_state must be an object".into());
    }
    if status.get("next_command").and_then(Value::as_str).is_none() {
        return Err("next_command must be a string".into());
    }
    let required = string_list(status.get("required_evidence"), "required_evidence")?;
    for evidence in required {
        let path = relative_path(&evidence)?;
        let full = root.join(path);
        if full.is_file() {
            let report = parse_json(&full)?;
            validate_report(&report, &evidence)?;
        }
    }
    Ok(())
}

fn validate_report(report: &Value, label: &str) -> Result<(), String> {
    if report.get("schema_version").and_then(Value::as_u64) != Some(1) {
        return Err(format!("{label}: schema_version must be 1"));
    }
    if !matches!(
        report.get("status").and_then(Value::as_str),
        Some("passed" | "incomplete" | "failed" | "complete")
    ) {
        return Err(format!(
            "{label}: status must be passed, incomplete, failed, or complete"
        ));
    }
    for field in [
        "checks",
        "required_targets",
        "completed_targets",
        "missing_evidence",
        "missing_checks",
        "skipped_targets",
    ] {
        if report.get(field).and_then(Value::as_array).is_none() {
            return Err(format!("{label}: {field} must be an array"));
        }
    }
    Ok(())
}

pub fn audit_closeout(root: &Path) -> Result<CloseoutAudit, String> {
    let status_path = root.join("evaluation/completion/status.json");
    let status = parse_json(&status_path)?;
    if status.get("schema_version").and_then(Value::as_u64) != Some(1) {
        return Err("status.json schema_version must be 1".into());
    }
    let required_evidence = string_list(status.get("required_evidence"), "required_evidence")?;
    let required_checks =
        optional_string_list(status.get("required_check_ids"), "required_check_ids")?;
    let validate_final_snapshot = required_checks
        .iter()
        .any(|check| check == "release-target-parity")
        && required_checks
            .iter()
            .any(|check| check == "performance-correctness-and-confidence");
    let required_targets =
        optional_string_list(status.get("required_targets"), "required_targets")?;
    let target_paths =
        optional_string_list(status.get("target_evidence_paths"), "target_evidence_paths")?;
    let unsatisfied_goal_ids =
        optional_string_list(status.get("unsatisfied_goal_ids"), "unsatisfied_goal_ids")?;

    let mut missing_evidence = Vec::new();
    let mut missing_checks: BTreeSet<String> = required_checks.into_iter().collect();
    let mut skipped_targets = BTreeSet::new();
    let mut failed = false;
    for evidence in &required_evidence {
        let relative = relative_path(evidence)?;
        let full = root.join(&relative);
        if !full.is_file() {
            missing_evidence.push(evidence.clone());
            continue;
        }
        let report = parse_json(&full)?;
        validate_report(&report, evidence)?;
        let report_status = report["status"].as_str().unwrap_or_default();
        if report_status == "failed" {
            failed = true;
        }
        if report_status != "passed" {
            missing_checks.insert(format!(
                "required report not passed ({evidence}: {report_status})"
            ));
        }
        for item in string_list(report.get("missing_evidence"), "missing_evidence")? {
            missing_evidence.push(format!("{evidence}: {item}"));
        }
        for item in string_list(report.get("missing_checks"), "missing_checks")? {
            missing_checks.insert(format!("{evidence}: {item}"));
        }
        for item in string_list(report.get("skipped_targets"), "skipped_targets")? {
            skipped_targets.insert(format!("{evidence}: {item}"));
        }
        if let Some(checks) = report.get("checks").and_then(Value::as_array) {
            for check in checks {
                let Some(id) = check.get("id").and_then(Value::as_str) else {
                    continue;
                };
                if check.get("status").and_then(Value::as_str) == Some("passed") {
                    missing_checks.remove(id);
                } else if check.get("status").and_then(Value::as_str) == Some("failed") {
                    failed = true;
                }
            }
        }
        if target_paths.contains(evidence) {
            let completed = string_list(report.get("completed_targets"), "completed_targets")?;
            for target in required_targets
                .iter()
                .filter(|target| !completed.contains(target))
            {
                skipped_targets.insert(target.clone());
            }
        }
    }
    if validate_final_snapshot {
        match validate_final_artifact_snapshot(root, &required_targets) {
            Ok(()) => {
                // This is a derived gate: the release aggregate plus its hash and target
                // checks establish parity, so no individual cargo test emits this ID.
                missing_checks.remove("release-target-parity");
            }
            Err(error) => {
                missing_checks.insert(format!("final-artifact-snapshot ({error})"));
            }
        }
    }
    if let Some(defects) = status.get("open_defects").and_then(Value::as_array) {
        for defect in defects {
            if let Some(defect) = defect.as_str() {
                missing_checks.insert(format!("open-defect ({defect})"));
            } else {
                missing_checks.insert("open-defect (entry must be text)".into());
            }
        }
    }
    missing_evidence.sort();
    let missing_checks: Vec<_> = missing_checks.into_iter().collect();
    let skipped_targets: Vec<_> = skipped_targets.into_iter().collect();
    let complete = missing_evidence.is_empty()
        && missing_checks.is_empty()
        && skipped_targets.is_empty()
        && unsatisfied_goal_ids.is_empty()
        && !failed;
    let audit_status = if failed {
        "failed"
    } else if complete {
        "complete"
    } else {
        "incomplete"
    };
    Ok(CloseoutAudit {
        status: audit_status.to_string(),
        exit_code: if complete { 0 } else { 1 },
        missing_evidence,
        missing_checks,
        skipped_targets,
        unsatisfied_goal_ids,
    })
}
