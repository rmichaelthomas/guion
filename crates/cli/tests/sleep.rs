//! `sleep_ms` (artifact format 88): a blocking sleep in slices that a stop, a deadline or a
//! signal ends promptly, that a tape does not record, and that a replay does not wait for.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let directory =
            std::env::temp_dir().join(format!("guion-sleep-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        Scratch(directory)
    }

    fn write(&self, source: &str) {
        std::fs::write(self.0.join("main.gu"), source).unwrap();
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn guion(directory: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(args)
        .current_dir(directory)
        .env("GUION_NO_PROMPT", "1")
        .output()
        .expect("guion starts")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn run(name: &str, flags: &[&str], source: &str) -> Output {
    let scratch = Scratch::new(name);
    scratch.write(source);
    let mut args = vec!["run"];
    args.extend_from_slice(flags);
    args.push("main.gu");
    guion(&scratch.0, &args)
}

#[test]
fn sleeps_at_least_requested() {
    let output = run(
        "at-least",
        &["--allow-clock"],
        "fn main() -> Int:\n    let before = unix_time_ms()\n    sleep_ms(50)\n    let after = unix_time_ms()\n    print(after - before)\n    return 0\n",
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let gap: i64 = stdout(&output).parse().unwrap();
    assert!(gap >= 50, "slept {gap} ms");
    assert!(gap < 2_000, "slept {gap} ms");
}

#[test]
fn zero_returns() {
    let output = run(
        "zero",
        &[],
        "fn main() -> Int:\n    sleep_ms(0)\n    print(\"done\")\n    return 0\n",
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "done");
}

#[test]
fn the_longest_allowed_sleep_is_accepted() {
    // A day is the limit. Run it in a task that is stopped at its deadline: the sleep starts
    // (it is not a range error) and ends with the task.
    let started = Instant::now();
    let output = run(
        "limit",
        &[],
        "fn nap(ms: Int) -> Int:\n    sleep_ms(ms)\n    return 1\n\nfn main() -> Int:\n    together:\n        let task = start(nap, 86400000)\n        let result = wait_within(task, 100)\n        print(task_code(error(result)))\n    return 0\n",
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "R_DEADLINE");
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[test]
fn range_errors() {
    for (ms, label) in [("-1", "below"), ("86400001", "above")] {
        let output = run(
            &format!("range-{label}"),
            &[],
            &format!("fn main() -> Int:\n    sleep_ms({ms})\n    return 0\n"),
        );
        assert!(!output.status.success(), "{ms}");
        assert!(
            stderr(&output).contains("R_SLEEP_RANGE"),
            "{ms}: {}",
            stderr(&output)
        );
    }
}

#[test]
fn deadline_stops_sleeping_task() {
    let started = Instant::now();
    let output = run(
        "deadline",
        &[],
        "fn nap(ms: Int) -> Int:\n    sleep_ms(ms)\n    return 1\n\nfn main() -> Int:\n    together:\n        let task = start(nap, 10000)\n        let result = wait_within(task, 100)\n        if is_ok(result):\n            print(\"finished\")\n        else:\n            print(task_code(error(result)))\n    return 0\n",
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "R_DEADLINE");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "took {:?}",
        started.elapsed()
    );
}

#[cfg(unix)]
fn spawn_and_interrupt(name: &str, source: &str) -> (Output, Duration) {
    let scratch = Scratch::new(name);
    scratch.write(source);
    let child = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", "main.gu"])
        .current_dir(&scratch.0)
        .env("GUION_NO_PROMPT", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("guion starts");
    std::thread::sleep(Duration::from_millis(400));
    let interrupted = Instant::now();
    // SAFETY: the pid is the child this test started and still owns.
    unsafe {
        libc::kill(child.id() as i32, libc::SIGINT);
    }
    let output = child.wait_with_output().unwrap();
    (output, interrupted.elapsed())
}

#[cfg(unix)]
#[test]
fn ctrl_c_interrupts_sleep() {
    use std::os::unix::process::ExitStatusExt;
    // Nothing handles the signal, so it ends the process as it does anywhere else.
    let (output, took) = spawn_and_interrupt(
        "ctrl-c",
        "fn main() -> Int:\n    sleep_ms(10000)\n    return 0\n",
    );
    assert!(took < Duration::from_secs(1), "took {took:?}");
    assert_eq!(output.status.signal(), Some(libc::SIGINT));

    // A program that polls for signals owns them: the sleep ends at once and the program reads it.
    let (output, took) = spawn_and_interrupt(
        "ctrl-c-polling",
        "import \"std:process\"\n\nfn main() -> Int:\n    let first = process_signal_poll()\n    sleep_ms(10000)\n    print(process_signal_poll())\n    return 0\n",
    );
    assert!(took < Duration::from_secs(1), "took {took:?}");
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "INT");
}

#[test]
fn replay_does_not_wait() {
    let scratch = Scratch::new("replay");
    scratch.write(
        "fn main() -> Int:\n    sleep_ms(2000)\n    print(\"rested\")\n    print(len(args()))\n    return 0\n",
    );
    let recorded_at = Instant::now();
    let recorded = guion(&scratch.0, &["run", "--record", "run.tape", "main.gu"]);
    assert!(recorded.status.success(), "{}", stderr(&recorded));
    assert!(recorded_at.elapsed() >= Duration::from_millis(2000));
    let tape = std::fs::read_to_string(scratch.0.join("run.tape")).unwrap();
    assert!(
        !tape.contains("Sleep"),
        "the sleep is not on the tape: {tape}"
    );

    let replayed_at = Instant::now();
    let replayed = guion(&scratch.0, &["run", "--replay", "run.tape", "main.gu"]);
    let took = replayed_at.elapsed();
    assert!(replayed.status.success(), "{}", stderr(&replayed));
    assert!(took < Duration::from_millis(500), "replay took {took:?}");
    assert_eq!(stdout(&replayed), stdout(&recorded));
}

#[test]
fn inspect_reports_clock_sleep() {
    let scratch = Scratch::new("inspect");
    scratch.write(
        "fn pause() -> Int:\n    sleep_ms(1)\n    return 0\n\nfn main() -> Int:\n    return pause()\n",
    );
    let output = guion(&scratch.0, &["inspect", "--json", "main.gu"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        report["functions"]["pause"]["effects"],
        serde_json::json!(["clock.sleep"])
    );
    assert_eq!(
        report["functions"]["main"]["effects"],
        serde_json::json!(["clock.sleep"])
    );
    let human = guion(&scratch.0, &["inspect", "main.gu"]);
    assert!(stdout(&human).contains("clock.sleep"), "{}", stdout(&human));
    let docs = guion(&scratch.0, &["doc", "--json", "main.gu"]);
    assert!(docs.status.success(), "{}", stderr(&docs));
    assert!(stdout(&docs).contains("clock.sleep"), "{}", stdout(&docs));
}

#[test]
fn old_format_rejects_sleep() {
    use guion_model::{Artifact, Op};
    const SLEEP_FIRST_FORMAT: u32 = 88;
    let scratch = Scratch::new("old-format");
    scratch.write("fn main() -> Int:\n    sleep_ms(1)\n    return 0\n");
    let built = scratch.0.join("main.gub");
    let output = guion(
        &scratch.0,
        &["build", "main.gu", "--output", built.to_str().unwrap()],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let current = std::fs::read(&built).unwrap();
    let mut artifact = Artifact::from_bytes(&current).unwrap();
    assert!(artifact.format_version >= SLEEP_FIRST_FORMAT);
    assert!(artifact.format_version == Artifact::CURRENT_VERSION);
    assert!(artifact
        .functions
        .iter()
        .any(|function| function.code.iter().any(|i| matches!(i.op, Op::Sleep))));
    let previous = SLEEP_FIRST_FORMAT - 1;
    artifact.format_version = previous;
    assert_eq!(artifact.to_bytes().unwrap_err().code, "E_ARTIFACT_VERSION");
    let mut old = current;
    old[4..8].copy_from_slice(&previous.to_le_bytes());
    assert_eq!(
        Artifact::from_bytes(&old).unwrap_err().code,
        "E_ARTIFACT_VERSION"
    );
}

#[test]
fn sleeping_function_runs_in_vm() {
    // The hot loop is compiled to machine code; the function that sleeps, and the function that
    // calls it, stay in the VM.
    let scratch = Scratch::new("vm-only");
    scratch.write(
        "fn nap(n: Int) -> Int:\n    var i = 0\n    while i < n:\n        sleep_ms(0)\n        set i = i + 1\n    return i\n\nfn spin(n: Int) -> Int:\n    var total = 0\n    var i = 0\n    while i < n:\n        set total = total + i\n        set i = i + 1\n    return total\n\nfn caller() -> Int:\n    return nap(3)\n\nfn main() -> Int:\n    print(spin(100000))\n    print(nap(5000))\n    print(caller())\n    return 0\n",
    );
    let output = guion(&scratch.0, &["run", "--json", "main.gu"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["program_stdout"], "4999950000\n5000\n3\n");
    let compiled: Vec<String> = report["native_backend"]["compiled_functions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|name| name.as_str().unwrap().to_owned())
        .collect();
    assert!(compiled.contains(&"spin".to_owned()), "{compiled:?}");
    assert!(!compiled.contains(&"nap".to_owned()), "{compiled:?}");
    assert!(!compiled.contains(&"caller".to_owned()), "{compiled:?}");
}
