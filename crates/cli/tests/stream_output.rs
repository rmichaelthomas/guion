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

const SLOW: &str = "fn main() -> Int:\n    print(\"first\")\n    var i = 0\n    while i < 900000000:\n        set i = i + 1\n    print(\"second\")\n    return 0\n";

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
        "first arrived only {gap:?} before the process ended; the loop must run for a few seconds"
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

fn run_lines(runtime_section: &str, main: &str) -> (bool, String, String) {
    let project = project(runtime_section, main);
    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", project.0.to_str().unwrap()])
        .env("GUION_NO_PROMPT", "1")
        .output()
        .unwrap();
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

// Native code that prints and then gives up (here: the product leaves 64 bits) is replayed by the VM
// from the start. What native code printed before giving up must not be written twice.
#[test]
fn native_prints_are_not_repeated_when_the_function_falls_back_to_the_vm() {
    let main = "fn grow(n: Int) -> Int:\n    var i = 0\n    var x = 1\n    while i < n:\n        print(x)\n        set x = x * 3\n        set i = i + 1\n    return x\n\nfn main() -> Int:\n    print(grow(60))\n    return 0\n";
    let (ok, stdout, stderr) = run_lines("", main);
    assert!(ok, "{stderr}");
    let mut expected = String::new();
    let mut x = num_bigint_free_pow3(0);
    for index in 0..60u32 {
        x = num_bigint_free_pow3(index);
        expected.push_str(&format!("{x}\n"));
    }
    expected.push_str(&format!("{}\n", num_bigint_free_pow3(60)));
    let _ = x;
    assert_eq!(stdout, expected);
}

/// 3^power as a decimal string (power up to 60 overflows u64 but not u128).
fn num_bigint_free_pow3(power: u32) -> u128 {
    3u128.pow(power)
}

#[test]
fn native_stdout_and_stderr_prints_keep_their_order_and_appear_once() {
    let main = "fn both(n: Int) -> Int:\n    var i = 0\n    while i < n:\n        print(i)\n        eprint(\"e\")\n        set i = i + 1\n    return i\n\nfn main() -> Int:\n    print(both(2000))\n    return 0\n";
    let (ok, stdout, stderr) = run_lines("", main);
    assert!(ok, "{stderr}");
    let expected: String = (0..2000).map(|i| format!("{i}\n")).collect::<String>() + "2000\n";
    assert_eq!(stdout, expected);
    assert_eq!(
        stderr.matches('e').count(),
        stderr.len() - stderr.matches('\n').count()
    );
    assert_eq!(stderr.lines().count(), 2000);
}

#[test]
fn native_prints_respect_the_output_limit_and_what_fits_is_written() {
    let main = "fn many(n: Int) -> Int:\n    var i = 0\n    while i < n:\n        print(i % 10)\n        set i = i + 1\n    return i\n\nfn main() -> Int:\n    print(many(100000))\n    return 0\n";
    // Each line is two bytes; 64 bytes hold 32.
    let (ok, stdout, stderr) = run_lines("output_limit_bytes = 64\n", main);
    assert!(!ok);
    assert!(stderr.contains("R_OUTPUT_LIMIT"), "{stderr}");
    assert_eq!(stdout.lines().count(), 32, "{stdout}");
}

// Native code writes early lines before it finishes (so a long loop does not hold them back). When it
// later leaves native code at an overflow (the VM resumes mid-function), nothing is written twice, on
// either stream.
#[test]
fn lines_native_code_already_wrote_are_not_repeated_by_the_replay() {
    let main = "fn grow(n: Int) -> Int:\n    var x = 1\n    print(\"start\")\n    eprint(\"warn\")\n    var spin = 0\n    while spin < 60000000:\n        set spin = spin + 1\n    var i = 0\n    while i < n:\n        print(x)\n        set x = x * 3\n        set i = i + 1\n    return x\n\nfn main() -> Int:\n    print(grow(60))\n    return 0\n";
    let (ok, stdout, stderr) = run_lines("instruction_limit = 100000000000\n", main);
    assert!(ok, "{stderr}");
    let mut expected = String::from("start\n");
    for index in 0..60u32 {
        expected.push_str(&format!("{}\n", num_bigint_free_pow3(index)));
    }
    expected.push_str(&format!("{}\n", num_bigint_free_pow3(60)));
    assert_eq!(stdout, expected);
    assert_eq!(stderr, "warn\n");
}

// The `native_math` case: `main` is native, prints a line, and then `float_to_int` of a value past 64
// bits makes native code give up. The VM runs `main` again from its start; the first line was already
// written and must not appear twice.
#[test]
fn native_main_that_falls_back_after_printing_does_not_print_twice() {
    let main = "import \"std:math\"\nfn sum(n: Int) -> Float64:\n    var i = 0\n    var total = 0.0\n    while i < n:\n        let x = int_to_float(i) * 0.5\n        set total = total + math_sin(x) + math_floor(x) + math_pow(x, 2.0) + math_atan2(x, 3.0)\n        set i = i + 1\n    return total\n\nfn main() -> Int:\n    print(sum(5000))\n    print(float_to_int(123456.9))\n    print(float_to_int(-7.9))\n    print(float_to_int(1.0e19))\n    return 0\n";
    let (ok, stdout, stderr) = run_lines("", main);
    assert!(ok, "{stderr}");
    assert_eq!(
        stdout,
        "10419797182.625711\n123456\n-7\n10000000000000000000\n"
    );
}

// A stdio server prints a reply and then waits for the next request. A function that reads input
// never runs natively (it calls a host operation), so the reply comes from a native function that has
// returned, and its held lines are written when it returns, before main blocks on input.
fn blocking_server() -> &'static str {
    "import \"std:io\"\nfn reply(n: Int) -> Int:\n    var i = 0\n    var total = 0\n    while i < n:\n        set total = (total + i * 3) % 1000003\n        set i = i + 1\n    print(\"reply\")\n    print(\"reply2\")\n    return total\n\nfn main() -> Int:\n    print(reply(2000000))\n    match stdin_read_line():\n        Ok(got):\n            match got:\n                Some(line):\n                    print(line)\n                None:\n                    print(\"eof\")\n        Err(problem):\n            print(problem.code)\n    return 0\n"
}

#[test]
fn a_native_function_writes_its_lines_before_the_program_blocks_on_stdin() {
    let main = blocking_server();
    let project = project("instruction_limit = 100000000000\n", main);
    let mut child = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", project.0.to_str().unwrap()])
        .env("GUION_NO_PROMPT", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut lines = BufReader::new(stdout).lines();
        while let Some(Ok(line)) = lines.next() {
            if sender.send(line).is_err() {
                break;
            }
        }
    });
    // Nothing has been written to stdin yet: both lines must arrive anyway.
    for expected in ["reply", "reply2", "63"] {
        let line = receiver
            .recv_timeout(Duration::from_secs(10))
            .unwrap_or_else(|_| {
                let _ = child.kill();
                panic!("{expected} did not arrive while the program waited for input")
            });
        assert_eq!(line, expected);
    }
    use std::io::Write;
    child.stdin.take().unwrap().write_all(b"next\n").unwrap();
    assert_eq!(
        receiver.recv_timeout(Duration::from_secs(10)).unwrap(),
        "next"
    );
    assert!(child.wait().unwrap().success());
}

/// Two prints back to back (the second lands within the batching interval), then a long native loop.
const BACK_TO_BACK: &str = "fn main() -> Int:\n    print(\"first\")\n    print(\"second\")\n    var i = 0\n    while i < 900000000:\n        set i = i + 1\n    print(\"third\")\n    return 0\n";

#[test]
fn a_line_printed_right_after_another_is_not_held_until_the_loop_ends() {
    let project = project("instruction_limit = 100000000000\n", BACK_TO_BACK);
    let mut child = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", project.0.to_str().unwrap()])
        .env("GUION_NO_PROMPT", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    assert_eq!(lines.next().unwrap().unwrap(), "first");
    assert_eq!(lines.next().unwrap().unwrap(), "second");
    let second_at = Instant::now();
    assert_eq!(lines.next().unwrap().unwrap(), "third");
    let third_at = Instant::now();
    assert!(child.wait().unwrap().success());
    // `second` was readable while the loop was still running, not together with `third`.
    let gap = third_at.duration_since(second_at);
    assert!(
        gap >= Duration::from_millis(500),
        "second arrived only {gap:?} before third; it was held until the loop ended"
    );
}
