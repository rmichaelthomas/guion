//! `std:io`: the program's own standard input (artifact format 77).

use guion_model::{Artifact, Op};
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

struct Project(PathBuf);

impl Drop for Project {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn project(runtime_section: &str, main: &str) -> Project {
    let dir = std::env::temp_dir().join(format!(
        "guion-stdlib-io-{}-{}",
        std::process::id(),
        NEXT_ID.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("guion.toml"),
        format!(
            "[project]\nname = \"stdlib-io\"\nversion = \"0.1.0\"\nentry = \"main.gu\"\n\n[runtime]\n{runtime_section}\n"
        ),
    )
    .unwrap();
    fs::write(dir.join("main.gu"), main).unwrap();
    Project(dir)
}

/// Runs `guion <args> <project>` with `input` on standard input, or with it closed for `None`.
fn guion(project: &Project, args: &[&str], input: Option<&[u8]>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_guion"));
    command
        .args(args)
        .arg(project.0.to_str().unwrap())
        .env("GUION_NO_PROMPT", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    match input {
        Some(_) => command.stdin(Stdio::piped()),
        None => command.stdin(Stdio::null()),
    };
    let mut child = command.spawn().unwrap();
    if let Some(bytes) = input {
        let mut stdin = child.stdin.take().unwrap();
        // The program may stop reading early; a closed pipe is not a test failure.
        let _ = stdin.write_all(bytes);
    }
    child.wait_with_output().unwrap()
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n")
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// Prints each line, then `end`; an error prints its code and stops.
const LINES: &str = r#"import "std:io"
import "std:types"

fn main() -> Int:
    var going = true
    while going:
        match stdin_read_line():
            Ok(next):
                match next:
                    Some(line):
                        print("[" + line + "]")
                    None:
                        print("end")
                        set going = false
            Err(problem):
                print(problem.code)
                set going = false
    return 0
"#;

const ALL: &str = r#"import "std:io"
import "std:types"

fn main() -> Int:
    match stdin_read_all():
        Ok(bytes):
            print(int_to_text(len(bytes_to_ints(bytes))))
            for value in bytes_to_ints(bytes):
                print(int_to_text(value))
        Err(problem):
            print(problem.code)
    return 0
"#;

#[test]
fn reads_lines_then_end() {
    let project = project("", LINES);
    let output = guion(&project, &["run"], Some(b"a\nb\n"));
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "[a]\n[b]\nend\n");
}

#[test]
fn crlf_and_unterminated_last_line() {
    let project = project("", LINES);
    let output = guion(&project, &["run"], Some(b"x\r\ny"));
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "[x]\n[y]\nend\n");
}

#[test]
fn empty_input_is_end() {
    let project = project("", LINES);
    let output = guion(&project, &["run"], Some(b""));
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "end\n");
    let closed = guion(&project, &["run"], None);
    assert!(closed.status.success(), "{}", stderr(&closed));
    assert_eq!(stdout(&closed), "end\n");
}

#[test]
fn line_over_limit_is_typed_error() {
    let project = project("stdin_line_limit_bytes = 8\n", LINES);
    let output = guion(&project, &["run"], Some(b"123456789\nok\n"));
    assert!(output.status.success(), "{}", stderr(&output));
    // The long line is skipped whole: the next read gets the line after it.
    assert_eq!(stdout(&output), "E_STDIN_LINE_LIMIT\n");
    let at_limit = guion(&project, &["run"], Some(b"12345678\n"));
    assert_eq!(stdout(&at_limit), "[12345678]\nend\n");
}

#[test]
fn invalid_utf8_line_is_typed_error() {
    let project = project("", LINES);
    let output = guion(&project, &["run"], Some(&[0xff, 0x0a]));
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "E_STDIN_UTF8\n");
}

#[test]
fn read_all_returns_raw_bytes() {
    let project = project("", ALL);
    let output = guion(&project, &["run"], Some(&[0x00, 0xff, 0x0a]));
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "3\n0\n255\n10\n");
}

#[test]
fn read_all_past_max_file_bytes_is_typed_error() {
    let project = project("max_file_bytes = 4\n", ALL);
    let output = guion(&project, &["run"], Some(b"12345"));
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "E_STDIN_LIMIT\n");
}

#[test]
fn a_line_read_and_read_all_share_one_stream() {
    let source = r#"import "std:io"
import "std:types"

fn main() -> Int:
    match stdin_read_line():
        Ok(first):
            match first:
                Some(line):
                    print(line)
                None:
                    print("none")
        Err(problem):
            print(problem.code)
    match stdin_read_all():
        Ok(bytes):
            print(int_to_text(len(bytes_to_ints(bytes))))
        Err(problem):
            print(problem.code)
    return 0
"#;
    let project = project("", source);
    let output = guion(&project, &["run"], Some(b"head\nrest of it\n"));
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "head\n11\n");
}

#[test]
fn bad_stdin_line_limit_is_refused() {
    let project = project("stdin_line_limit_bytes = 0\n", LINES);
    let output = guion(&project, &["run"], Some(b"a\n"));
    assert!(!output.status.success());
    assert!(stderr(&output).contains("stdin_line_limit_bytes"));
}

#[test]
fn inspect_reports_process_stdin() {
    let project = project("", LINES);
    let output = guion(&project, &["inspect", "--json"], None);
    assert!(output.status.success(), "{}", stderr(&output));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let text = report.to_string();
    let required = report["required_effects"].as_array().unwrap();
    assert!(
        required.iter().any(|effect| effect == "process.stdin"),
        "{text}"
    );
    assert!(report["functions"]["main"]["effects"]
        .as_array()
        .unwrap()
        .iter()
        .any(|effect| effect == "process.stdin"));
    // Reading stdin is an effect, not a grant: nothing is asked for.
    for grant in [
        "network_grant",
        "process_grant",
        "socket_grant",
        "clock_grant",
    ] {
        assert_eq!(
            report[grant]["required_for_entry"], false,
            "{grant}: {text}"
        );
    }
    for (name, value) in report["grant_assessment"].as_object().unwrap() {
        if name != "caller_cli_grants" {
            assert_eq!(value, "not_required", "{name}: {text}");
        }
    }
}

#[test]
fn record_and_replay_stdin() {
    let project = project("", LINES);
    let tape = project.0.join("t.tape");
    let recorded = guion(
        &project,
        &["run", "--record", tape.to_str().unwrap()],
        Some(b"q\n"),
    );
    assert!(recorded.status.success(), "{}", stderr(&recorded));
    assert_eq!(stdout(&recorded), "[q]\nend\n");
    let replayed = guion(&project, &["run", "--replay", tape.to_str().unwrap()], None);
    assert!(replayed.status.success(), "{}", stderr(&replayed));
    assert_eq!(stdout(&replayed), "[q]\nend\n");
}

#[test]
fn old_format_rejects_stdin_ops() {
    let project = project("", LINES);
    let built = project.0.join("main.gub");
    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args([
            "build",
            project.0.to_str().unwrap(),
            "--output",
            built.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    let current = fs::read(&built).unwrap();
    let mut artifact = Artifact::from_bytes(&current).unwrap();
    assert!(artifact.functions.iter().any(|function| function
        .code
        .iter()
        .any(|instruction| matches!(instruction.op, Op::StdinReadLine))));
    let previous = Artifact::CURRENT_VERSION - 1;
    artifact.format_version = previous;
    assert_eq!(artifact.to_bytes().unwrap_err().code, "E_ARTIFACT_VERSION");
    let mut old = current;
    old[4..8].copy_from_slice(&previous.to_le_bytes());
    assert_eq!(
        Artifact::from_bytes(&old).unwrap_err().code,
        "E_ARTIFACT_VERSION"
    );
    // The CLI refuses the same file.
    fs::write(&built, &old).unwrap();
    let run = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", built.to_str().unwrap()])
        .env("GUION_NO_PROMPT", "1")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!run.status.success());
    assert!(
        stderr(&run).contains("E_ARTIFACT_VERSION"),
        "{}",
        stderr(&run)
    );
}
