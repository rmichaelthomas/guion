//! `guion run` writes each `print` as it happens; `--json` and embedders keep buffering.

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

struct Project(PathBuf);

impl Drop for Project {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn project(runtime_section: &str, main: &str) -> Project {
    let dir = std::env::temp_dir().join(format!(
        "guion-stream-output-{}-{}",
        std::process::id(),
        NEXT_ID.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("guion.toml"),
        format!(
            "[project]\nname = \"stream-output\"\nversion = \"0.1.0\"\nentry = \"main.gu\"\n\n[runtime]\n{runtime_section}\n"
        ),
    )
    .unwrap();
    fs::write(dir.join("main.gu"), main).unwrap();
    Project(dir)
}

const SLOW: &str = "fn main() -> Int:\n    print(\"first\")\n    var i = 0\n    while i < 120000000:\n        set i = i + 1\n    print(\"second\")\n    return 0\n";

#[test]
fn prints_arrive_before_the_program_ends() {
    let project = project("instruction_limit = 100000000000\n", SLOW);
    let mut child = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", project.0.to_str().unwrap()])
        .env("GUION_NO_PROMPT", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let first = lines.next().unwrap().unwrap();
    let first_at = Instant::now();
    assert_eq!(first, "first");
    let second = lines.next().unwrap().unwrap();
    assert_eq!(second, "second");
    let status = child.wait().unwrap();
    let ended_at = Instant::now();
    assert!(status.success());
    // `first` must have been readable well before the loop ended.
    let gap = ended_at.duration_since(first_at);
    assert!(
        gap >= Duration::from_secs(1),
        "first arrived only {gap:?} before the process ended; the loop must run for about 2 s"
    );
}

#[test]
fn streamed_output_still_respects_output_limit() {
    let lines = "    print(\"0123456789\")\n".repeat(10);
    let project = project(
        "output_limit_bytes = 64\n",
        &format!("fn main() -> Int:\n{lines}    return 0\n"),
    );
    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", project.0.to_str().unwrap()])
        .env("GUION_NO_PROMPT", "1")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("R_OUTPUT_LIMIT"), "stderr: {stderr}");
    // Each line is 11 bytes; five fit in 64.
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "0123456789\n".repeat(5)
    );
}

#[test]
fn json_mode_still_buffers() {
    let project = project(
        "",
        "fn main() -> Int:\n    print(\"buffered\")\n    return 0\n",
    );
    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", "--json", project.0.to_str().unwrap()])
        .env("GUION_NO_PROMPT", "1")
        .output()
        .unwrap();
    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["program_stdout"], "buffered\n");
}

#[test]
fn a_closed_pipe_is_output_closed_not_a_panic() {
    let project = project("instruction_limit = 100000000000\n", SLOW);
    let mut child = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", project.0.to_str().unwrap()])
        .env("GUION_NO_PROMPT", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // The reader goes away while the program is still looping.
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut first = String::new();
    stdout.read_line(&mut first).unwrap();
    assert_eq!(first, "first\n");
    drop(stdout);
    let output = child.wait_with_output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(stderr.contains("R_OUTPUT_CLOSED"), "stderr: {stderr}");
    assert!(!stderr.contains("panicked"), "stderr: {stderr}");
}
