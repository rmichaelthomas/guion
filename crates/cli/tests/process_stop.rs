//! `process_stop_requested` (artifact format 90): true once INT, TERM or HUP has arrived in a program
//! that polls for signals; sticky, shared by every thread, and it does not take the pending signal.

#![cfg(unix)]

use std::io::{BufRead, Read};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::time::Duration;

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let directory =
            std::env::temp_dir().join(format!("guion-stop-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        Scratch(directory)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Runs `source`, waits for its `ready` line, sends `signal`, and returns what it printed after.
fn run_and_signal(name: &str, source: &str, signal: i32) -> (Output, String) {
    let scratch = Scratch::new(name);
    std::fs::write(scratch.0.join("main.gu"), source).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_guion"));
    command
        .args(["run", "main.gu"])
        .current_dir(&scratch.0)
        .env("GUION_NO_PROMPT", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // SAFETY: signal() is async-signal-safe and touches nothing else between fork and exec.
    unsafe {
        command.pre_exec(|| {
            for signal in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
                libc::signal(signal, libc::SIG_DFL);
            }
            Ok(())
        });
    }
    let mut child = command.spawn().expect("guion starts");
    let mut reader = std::io::BufReader::new(child.stdout.take().unwrap());
    let mut before = String::new();
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        if line.trim() == "ready" || line.is_empty() {
            break;
        }
        before.push_str(&line);
    }
    std::thread::sleep(Duration::from_millis(200));
    // SAFETY: the pid is the child this test started and still owns.
    unsafe {
        libc::kill(child.id() as i32, signal);
    }
    let mut after = String::new();
    reader.read_to_string(&mut after).unwrap();
    let output = child.wait_with_output().unwrap();
    (output, format!("{before}{after}"))
}

const POLLING: &str = "import \"std:process\"

fn seen(index: Int) -> Bool:
    return process_stop_requested()

fn word(flag: Bool) -> Text:
    if flag:
        return \"yes\"
    return \"no\"

fn main() -> Int:
    let first = process_signal_poll()
    print(\"before \" + word(process_stop_requested()))
    print(\"ready\")
    var waited = 0
    while waited < 100 and process_stop_requested() == false:
        sleep_ms(50)
        set waited = waited + 1
    print(\"after \" + word(process_stop_requested()))
    print(\"again \" + word(process_stop_requested()))
    print(\"poll \" + process_signal_poll())
    print(\"poll again [\" + process_signal_poll() + \"]\")
    print(\"after poll \" + word(process_stop_requested()))
    let workers = parallel_map([1, 2, 3], seen, 3)
    match workers:
        Ok(flags):
            print(\"workers \" + word(flags[0]) + word(flags[1]) + word(flags[2]))
        Err(problem):
            print(\"workers failed\")
    return 0
";

#[test]
fn stop_requested_is_false_until_a_signal_then_sticky_and_shared() {
    for (signal, name) in [
        (libc::SIGTERM, "TERM"),
        (libc::SIGINT, "INT"),
        (libc::SIGHUP, "HUP"),
    ] {
        let (output, printed) = run_and_signal(&format!("polling-{name}"), POLLING, signal);
        assert!(
            output.status.success(),
            "{printed}{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let lines: Vec<&str> = printed.lines().collect();
        assert_eq!(
            lines,
            [
                "before no",
                "after yes",
                "again yes",
                &format!("poll {name}"),
                "poll again []",
                "after poll yes",
                "workers yesyesyes",
            ],
            "{name}"
        );
    }
}

#[test]
fn a_program_that_does_not_poll_still_ends_on_the_signal() {
    let (output, printed) = run_and_signal(
        "not-polling",
        "import \"std:process\"

fn main() -> Int:
    if process_stop_requested():
        print(\"stop\")
    print(\"ready\")
    sleep_ms(10000)
    print(\"survived\")
    return 0
",
        libc::SIGTERM,
    );
    assert_eq!(output.status.signal(), Some(libc::SIGTERM), "{printed}");
    assert!(!printed.contains("survived"));
}

#[test]
fn stop_requested_replays_as_recorded() {
    let scratch = Scratch::new("replay");
    std::fs::write(
        scratch.0.join("main.gu"),
        "import \"std:process\"

fn main() -> Int:
    if process_stop_requested():
        print(\"stop\")
    else:
        print(\"go\")
    return 0
",
    )
    .unwrap();
    let guion = |args: &[&str]| -> Output {
        Command::new(env!("CARGO_BIN_EXE_guion"))
            .args(args)
            .current_dir(&scratch.0)
            .env("GUION_NO_PROMPT", "1")
            .output()
            .unwrap()
    };
    let recorded = guion(&["run", "--record", "run.tape", "main.gu"]);
    assert!(
        recorded.status.success(),
        "{}",
        String::from_utf8_lossy(&recorded.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&recorded.stdout).trim(), "go");
    let replayed = guion(&["run", "--replay", "run.tape", "main.gu"]);
    assert!(
        replayed.status.success(),
        "{}",
        String::from_utf8_lossy(&replayed.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&replayed.stdout).trim(), "go");
}
