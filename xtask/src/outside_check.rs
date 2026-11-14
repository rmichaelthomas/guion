//! The outside-program guardrail: `evaluation/outside/jsonl-summary/run.py` must produce
//! byte-identical output to its Python twin, and no Guion median may be more than the noise
//! allowance (5%) slower than `baseline-0.2.0.json`, which sits next to `run.py`.
use serde_json::Value;
use std::fs;
use std::path::Path;
use std::process::Command;

/// Percent a median may exceed the baseline before the check fails. Repeated runs of the same
/// build on the development machine moved by up to this much, so a smaller figure would fail
/// on noise.
pub const NOISE_ALLOWANCE_PERCENT: f64 = 5.0;

const DIRECTORY: &str = "evaluation/outside/jsonl-summary";

/// Compare measured medians with the baseline; one line per case, and the cases over the
/// allowance. Only the Guion cases the baseline lists are guarded.
pub fn evaluate(
    baseline: &Value,
    measured: &Value,
    allowance_percent: f64,
) -> (Vec<String>, Vec<String>) {
    let mut report = Vec::new();
    let mut failures = Vec::new();
    let mut cases = vec![(
        "hello world".to_owned(),
        baseline["hello_ms"].as_f64(),
        measured["hello_ms"].as_f64(),
    )];
    if let Some(rows) = baseline["rows"].as_object() {
        for (count, entry) in rows {
            cases.push((
                format!("JSONL summary, {count} rows, Guion"),
                entry["guion_ms"].as_f64(),
                measured["rows"][count]["guion_ms"].as_f64(),
            ));
        }
    }
    for (name, base, now) in cases {
        let (Some(base), Some(now)) = (base, now) else {
            failures.push(format!(
                "{name}: missing from the baseline or the measurement"
            ));
            continue;
        };
        let change = (now - base) / base * 100.0;
        let line = format!("{name}: {now:.1} ms against baseline {base:.1} ms ({change:+.1}%)");
        if change > allowance_percent {
            failures.push(format!("{line}, over the {allowance_percent}% allowance"));
        }
        report.push(line);
    }
    (report, failures)
}

/// `cargo xtask outside-check`: run the harness on the current release build and compare. The
/// release build is not made here: a `cargo build` from inside `cargo run` rebuilds the
/// binary, and a few-millisecond hello world measures slow for a while after a build.
pub fn run(root: &Path, _args: &[String]) -> i32 {
    let binary = root.join("target").join("release").join("guion");
    if !binary.is_file() {
        eprintln!(
            "outside-check: {} not found; run `cargo build --locked --release -p guion-cli` first",
            binary.display()
        );
        return 2;
    }
    let directory = root.join(DIRECTORY);
    let output =
        std::env::temp_dir().join(format!("guion-outside-check-{}.json", std::process::id()));
    let status = Command::new("python3")
        .current_dir(root)
        .arg(directory.join("run.py"))
        .arg("--json")
        .arg(&output)
        .status();
    let measured = fs::read_to_string(&output)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok());
    let _ = fs::remove_file(&output);
    if !status.is_ok_and(|status| status.success()) {
        eprintln!("outside-check: run.py failed (output mismatch between Guion and Python, or a run failed)");
        return 1;
    }
    let Some(measured) = measured else {
        eprintln!("outside-check: run.py wrote no measurements");
        return 1;
    };
    let baseline = match fs::read_to_string(directory.join("baseline-0.2.0.json"))
        .map_err(|error| error.to_string())
        .and_then(|text| serde_json::from_str::<Value>(&text).map_err(|error| error.to_string()))
    {
        Ok(value) => value,
        Err(error) => {
            eprintln!("outside-check: cannot read baseline-0.2.0.json: {error}");
            return 2;
        }
    };
    let (report, failures) = evaluate(&baseline, &measured, NOISE_ALLOWANCE_PERCENT);
    for line in &report {
        println!("{line}");
    }
    if failures.is_empty() {
        println!("outside-check passed (output identical, no median over {NOISE_ALLOWANCE_PERCENT}% slower than baseline-0.2.0)");
        0
    } else {
        for line in &failures {
            eprintln!("{line}");
        }
        eprintln!("outside-check failed");
        1
    }
}
