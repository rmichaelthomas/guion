//! Run the release assembly without overwriting the retained closeout reports.
use std::{env, fs, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let output =
        root.join("evaluation/completion/results/real-task-sprint/macos-release-checks.json");
    let checks = xtask::release::run_release(&root, &[]);
    let passed = checks.iter().all(|check| check.status == "passed");
    let results: Vec<_> = checks
        .iter()
        .map(|check| {
            serde_json::json!({
                "id": check.id,
                "status": check.status,
                "command": check.command,
                "exit_status": check.exit_status,
                "output": check.output,
            })
        })
        .collect();
    fs::create_dir_all(output.parent().unwrap())?;
    fs::write(
        &output,
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema_version": 1,
            "passed": passed,
            "checks": results,
        }))?,
    )?;
    println!(
        "Private release checks: {passed}; report: {}",
        output.display()
    );
    if !passed {
        std::process::exit(1);
    }
    Ok(())
}
