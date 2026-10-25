use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use xtask::{
    completion::{audit_closeout, validate_completion_schema},
    inventory::validate_inventory,
};

struct TempTree(PathBuf);

impl TempTree {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock is after Unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("guion-xtask-{}-{nonce}", std::process::id()));
        fs::create_dir_all(&path).expect("create temporary test tree");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempTree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn completion_missing_evidence_fails() {
    let tree = TempTree::new();
    let completion_dir = tree.path().join("evaluation/completion");
    fs::create_dir_all(&completion_dir).expect("create completion directory");
    fs::write(
        completion_dir.join("status.json"),
        r#"{
          "schema_version": 1,
          "current_task": 1,
          "next_command": "cargo xtask conformance --inventory-only",
          "evidence_paths": [],
          "unsatisfied_goal_ids": ["G1", "G2", "G3"],
          "required_evidence": ["evaluation/completion/results/checkpoint.json"]
        }"#,
    )
    .expect("write status fixture");

    let audit = audit_closeout(tree.path()).expect("audit closeout fixture");

    assert_eq!(audit.status, "incomplete");
    assert_eq!(audit.exit_code, 1);
    assert_eq!(
        audit.missing_evidence,
        ["evaluation/completion/results/checkpoint.json"]
    );
}

#[test]
fn completion_schema_accepts_numeric_task_ids() {
    let tree = TempTree::new();
    let completion_dir = tree.path().join("evaluation/completion");
    fs::create_dir_all(&completion_dir).expect("create completion directory");
    fs::write(
        completion_dir.join("status.json"),
        r#"{
          "schema_version": 1,
          "current_task": 2,
          "next_command": "cargo xtask conformance",
          "source_state": {},
          "evidence_paths": [],
          "required_evidence": [],
          "required_targets": [],
          "target_evidence_paths": [],
          "required_check_ids": [],
          "unsatisfied_goal_ids": ["G1"],
          "completed_tasks": [1]
        }"#,
    )
    .expect("write status fixture");

    validate_completion_schema(tree.path())
        .expect("the task ledger stores completed task numbers as integers");
}

#[test]
fn closeout_requires_release_and_performance_from_the_same_final_snapshot() {
    let tree = TempTree::new();
    let results = tree.path().join("evaluation/completion/results");
    fs::create_dir_all(&results).expect("create result directory");
    fs::write(
        tree.path().join("evaluation/completion/status.json"),
        r#"{
          "schema_version": 1,
          "current_task": 11,
          "next_command": "cargo xtask closeout",
          "evidence_paths": [],
          "unsatisfied_goal_ids": [],
          "required_evidence": [
            "evaluation/completion/results/release.json",
            "evaluation/completion/results/performance.json"
          ],
          "required_targets": ["macos-aarch64", "debian-aarch64"],
          "target_evidence_paths": ["evaluation/completion/results/release.json"],
          "required_check_ids": [
            "release-target-parity",
            "performance-correctness-and-confidence"
          ]
        }"#,
    )
    .expect("write status fixture");
    let artifact_hash = format!("sha256:{}", "a".repeat(64));
    for (name, check, digest, targets) in [
        (
            "release",
            "release-target-parity",
            "sha256:1111111111111111111111111111111111111111111111111111111111111111",
            r#"["macos-aarch64", "debian-aarch64"]"#,
        ),
        (
            "performance",
            "performance-correctness-and-confidence",
            "sha256:2222222222222222222222222222222222222222222222222222222222222222",
            r#"["macos-aarch64"]"#,
        ),
    ] {
        fs::write(
            results.join(format!("{name}.json")),
            format!(
                r#"{{
                  "schema_version": 1,
                  "status": "passed",
                  "checks": [{{"id":"{check}","status":"passed"}}],
                  "required_targets": ["macos-aarch64", "debian-aarch64"],
                  "completed_targets": {targets},
                  "missing_evidence": [],
                  "missing_checks": [],
                  "skipped_targets": [],
                  "source_revision": "abc123",
                  "working_tree_digest": "{digest}",
                  "seed_hash": "{artifact_hash}",
                  "stdlib_hash": "{artifact_hash}",
                  "runtime_hash": "{artifact_hash}",
                  "artifact_identities": {{
                    "artifact_schema": "{artifact_hash}",
                    "compiler_seed": "{artifact_hash}",
                    "compiler_seed_manifest": "{artifact_hash}",
                    "runtime": "{artifact_hash}",
                    "stdlib": "{artifact_hash}"
                  }}
                }}"#
            ),
        )
        .expect("write gate report fixture");
    }

    let audit = audit_closeout(tree.path()).expect("audit closeout fixture");

    assert_eq!(audit.status, "incomplete");
    assert!(audit
        .missing_checks
        .iter()
        .any(|check| check.contains("final-artifact-snapshot")));
}

#[test]
fn closeout_rejects_incomplete_required_report_even_when_named_checks_pass() {
    let tree = TempTree::new();
    let results = tree.path().join("evaluation/completion/results");
    fs::create_dir_all(&results).expect("create result directory");
    fs::write(
        tree.path().join("evaluation/completion/status.json"),
        r#"{
          "schema_version": 1,
          "current_task": 11,
          "next_command": "cargo xtask closeout",
          "source_state": {},
          "evidence_paths": [],
          "unsatisfied_goal_ids": [],
          "required_evidence": ["evaluation/completion/results/required.json"],
          "required_targets": [],
          "target_evidence_paths": [],
          "required_check_ids": ["required-check"]
        }"#,
    )
    .expect("write status fixture");
    fs::write(
        results.join("required.json"),
        r#"{
          "schema_version": 1,
          "status": "incomplete",
          "checks": [{"id":"required-check","status":"passed"}],
          "required_targets": [],
          "completed_targets": [],
          "missing_evidence": [],
          "missing_checks": [],
          "skipped_targets": []
        }"#,
    )
    .expect("write incomplete required report");

    let audit = audit_closeout(tree.path()).expect("audit closeout fixture");

    assert_eq!(audit.status, "incomplete");
    assert_eq!(audit.exit_code, 1);
    assert!(audit
        .missing_checks
        .iter()
        .any(|check| check.contains("required report") && check.contains("required.json")));
}

#[test]
fn closeout_rejects_required_report_with_nested_missing_or_skipped_entries() {
    let tree = TempTree::new();
    let results = tree.path().join("evaluation/completion/results");
    fs::create_dir_all(&results).expect("create result directory");
    fs::write(
        tree.path().join("evaluation/completion/status.json"),
        r#"{
          "schema_version": 1,
          "current_task": 11,
          "next_command": "cargo xtask closeout",
          "source_state": {},
          "evidence_paths": [],
          "unsatisfied_goal_ids": [],
          "required_evidence": ["evaluation/completion/results/required.json"],
          "required_targets": [],
          "target_evidence_paths": [],
          "required_check_ids": ["required-check"]
        }"#,
    )
    .expect("write status fixture");
    fs::write(
        results.join("required.json"),
        r#"{
          "schema_version": 1,
          "status": "passed",
          "checks": [{"id":"required-check","status":"passed"}],
          "required_targets": [],
          "completed_targets": [],
          "missing_evidence": ["nested.json"],
          "missing_checks": ["nested-check"],
          "skipped_targets": ["debian-aarch64"]
        }"#,
    )
    .expect("write incomplete-content report");

    let audit = audit_closeout(tree.path()).expect("audit closeout fixture");

    assert_eq!(audit.status, "incomplete");
    assert_eq!(audit.exit_code, 1);
    assert!(audit
        .missing_evidence
        .iter()
        .any(|item| item.contains("nested.json")));
    assert!(audit
        .missing_checks
        .iter()
        .any(|item| item.contains("nested-check")));
    assert!(audit
        .skipped_targets
        .iter()
        .any(|item| item.contains("debian-aarch64")));
}

#[test]
fn completion_schema_rejects_unknown_report_status() {
    let tree = TempTree::new();
    let completion_dir = tree.path().join("evaluation/completion");
    let results = completion_dir.join("results");
    fs::create_dir_all(&results).expect("create result directory");
    fs::write(
        completion_dir.join("status.json"),
        r#"{
          "schema_version": 1,
          "current_task": 1,
          "next_command": "cargo xtask closeout",
          "source_state": {},
          "evidence_paths": [],
          "required_evidence": ["evaluation/completion/results/required.json"],
          "required_targets": [],
          "target_evidence_paths": [],
          "required_check_ids": [],
          "unsatisfied_goal_ids": [],
          "completed_tasks": []
        }"#,
    )
    .expect("write status fixture");
    fs::write(
        results.join("required.json"),
        r#"{
          "schema_version": 1,
          "status": "maybe",
          "checks": [],
          "required_targets": [],
          "completed_targets": [],
          "missing_evidence": [],
          "missing_checks": [],
          "skipped_targets": []
        }"#,
    )
    .expect("write report with unknown status");

    let error = validate_completion_schema(tree.path())
        .expect_err("unknown report statuses must not pass schema validation");
    assert!(error.contains("status must be passed, incomplete, failed, or complete"));
}

#[test]
fn inventory_covers_every_executable_type_operator_and_host_signature() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let missing = validate_inventory(root).expect("read and validate conformance inventory");

    assert!(
        missing.is_empty(),
        "inventory is missing executable entries: {missing:#?}"
    );
}
