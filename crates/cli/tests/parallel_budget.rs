//! A `parallel_map` whose workers run out of the instruction budget stops promptly with
//! `R_STEP_LIMIT`, however many workers there are.

use std::process::Command;
use std::time::{Duration, Instant};

const SOURCE: &str = "fn spin(count: Int) -> Int:\n    var i = 0\n    while i < count:\n        set i = i + 1\n    return i\n\nfn job(count: Int) -> Int:\n    return spin(count)\n\nfn main() -> Int:\n    let mapped = parallel_map([60000000, 60000000, 60000000, 60000000], job, WORKERS)\n    if not is_ok(mapped):\n        print(task_code(error(mapped)))\n        return 1\n    print(\"finished\")\n    return 0\n";

fn run_with(workers: usize, instruction_limit: u64) -> (String, Duration) {
    let directory = std::env::temp_dir().join(format!(
        "guion-parallel-budget-{}-{workers}-{instruction_limit}",
        std::process::id()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(
        directory.join("main.gu"),
        SOURCE.replace("WORKERS", &workers.to_string()),
    )
    .unwrap();
    std::fs::write(
        directory.join("guion.toml"),
        format!(
            "[package]\nname = \"parallel-budget\"\nversion = \"0.1.0\"\n[runtime]\ninstruction_limit = {instruction_limit}\nallocation_limit_bytes = 8589934592\n"
        ),
    )
    .unwrap();
    let started = Instant::now();
    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .arg("run")
        .arg("main.gu")
        .current_dir(&directory)
        .output()
        .expect("guion starts");
    (
        String::from_utf8_lossy(&output.stdout).trim().to_owned(),
        started.elapsed(),
    )
}

#[test]
fn workers_that_run_out_of_budget_stop_promptly() {
    for workers in [1, 4] {
        // Four jobs of 60 million iterations need about a billion instructions; allow far fewer.
        let (out, elapsed) = run_with(workers, 300_000_000);
        assert_eq!(out, "R_STEP_LIMIT", "{workers} workers");
        assert!(
            elapsed < Duration::from_secs(20),
            "{workers} workers took {elapsed:?} to stop"
        );
    }
}

#[test]
fn workers_with_enough_budget_finish() {
    let (out, _) = run_with(4, 40_000_000_000);
    assert_eq!(out, "finished");
}
