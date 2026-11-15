//! Scoped profiles: denials that say how to fix them, `inspect --grants`, the question before a
//! run, the private `temp:` folder and `--sealed`. See `docs/design-scoped-profiles.md`.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const CLOCK_SOURCE: &str = "import \"std:time\"\n\nfn main() -> Int:\n    let current = today_local()\n    if current.year > 1970:\n        print(\"ran\")\n    return 0\n";

const COMMAND_SOURCE: &str = r#"import "std:process"
import "std:types"
fn main() -> Int:
    match process_run("echo", ["hi"], ""):
        Ok(result):
            print("ran")
        Err(problem):
            print(problem.code)
    return 0
"#;

const HTTP_SOURCE: &str = r#"import "std:types"
fn main() -> Int:
    let headers: Map[Text, Text] = {}
    match http_request_bytes("GET", "http://api.example.test/", headers, bytes_from_text(""), 1000):
        Ok(_response):
            print("ran")
        Err(problem):
            print(http_error_code(problem))
    return 0
"#;

/// A scratch project directory removed when dropped.
struct Project {
    dir: PathBuf,
}

impl Project {
    fn new(name: &str, source: &str, manifest: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("guion-grants-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("main.gu"), source).unwrap();
        if !manifest.is_empty() {
            std::fs::write(dir.join("guion.toml"), manifest).unwrap();
        }
        Self { dir }
    }

    fn path(&self) -> &Path {
        &self.dir
    }

    fn guion(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_guion"));
        command.current_dir(&self.dir);
        // The tests must never inherit a terminal-driven question from the developer's shell.
        command.env("GUION_NO_PROMPT", "1");
        command.env_remove("CI");
        command
    }

    fn run(&self, flags: &[&str]) -> Output {
        self.guion()
            .arg("run")
            .args(flags)
            .arg("main.gu")
            .output()
            .unwrap()
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn json_of(output: &Output) -> serde_json::Value {
    serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("not JSON ({error}): {}", stdout(output)))
}

// ---- Part 5: denials that say how to fix them ----

#[test]
fn a_clock_denial_names_the_manifest_line_and_the_run_flag() {
    let project = Project::new("fix-clock", CLOCK_SOURCE, "");
    let output = project.run(&[]);
    assert!(!output.status.success());
    let text = stderr(&output);
    assert!(text.contains("R_CLOCK_DENIED"), "{text}");
    assert!(
        text.contains("add to guion.toml:   [clock] enabled = true"),
        "{text}"
    );
    assert!(
        text.contains("or for one run:      guion run --allow-clock <file>"),
        "{text}"
    );
}

#[test]
fn a_json_denial_carries_the_grant_table_key_value_and_flag() {
    let project = Project::new("fix-json", CLOCK_SOURCE, "");
    let output = project.run(&["--json"]);
    let report = json_of(&output);
    assert_eq!(report["status"], "failed");
    let grant = &report["diagnostic"]["grant"];
    assert_eq!(grant["table"], "clock");
    assert_eq!(grant["key"], "enabled");
    assert_eq!(grant["value"], "true");
    assert_eq!(grant["flag"], "--allow-clock");
    // The same denial is listed in `denials`, with the same error code.
    assert_eq!(report["denials"][0]["code"], "R_CLOCK_DENIED");
    assert_eq!(report["denials"][0]["grant"], *grant);
}

#[test]
fn a_denial_the_program_handles_still_reaches_the_report() {
    let project = Project::new("fix-process", COMMAND_SOURCE, "");
    let output = project.run(&["--json"]);
    let report = json_of(&output);
    assert_eq!(report["status"], "passed");
    assert_eq!(report["program_stdout"], "E_PROCESS_DENIED\n");
    let denial = &report["denials"][0];
    assert_eq!(denial["code"], "E_PROCESS_DENIED");
    assert_eq!(denial["grant"]["table"], "process");
    assert_eq!(denial["grant"]["key"], "commands");
    assert_eq!(denial["grant"]["value"], "echo");
    assert_eq!(denial["grant"]["flag"], "--allow-command echo");
    assert!(
        denial["message"]
            .as_str()
            .unwrap()
            .contains("[process] commands = [\"echo\"]"),
        "{denial}"
    );

    let project = Project::new("fix-http", HTTP_SOURCE, "");
    let report = json_of(&project.run(&["--json"]));
    let denial = &report["denials"][0];
    assert_eq!(denial["code"], "R_NETWORK_DENIED");
    assert_eq!(denial["grant"]["table"], "network");
    assert_eq!(denial["grant"]["key"], "hosts");
    assert_eq!(denial["grant"]["value"], "api.example.test");
    assert_eq!(
        denial["grant"]["flag"],
        "--allow-http-host api.example.test"
    );
}

#[test]
fn a_run_with_every_grant_reports_no_denials() {
    let project = Project::new("fix-none", CLOCK_SOURCE, "");
    let report = json_of(&project.run(&["--json", "--allow-clock"]));
    assert_eq!(report["status"], "passed");
    assert_eq!(report["denials"], serde_json::json!([]));
}

#[test]
fn denial_codes_do_not_change() {
    let project = Project::new("fix-codes", COMMAND_SOURCE, "");
    assert_eq!(stdout(&project.run(&[])), "E_PROCESS_DENIED\n");
    let project = Project::new("fix-codes-http", HTTP_SOURCE, "");
    assert_eq!(stdout(&project.run(&[])), "R_NETWORK_DENIED\n");
}

// ---- Part 2: what a script needs versus what it has ----

const NEEDS_SOURCE: &str = r#"import "std:process"
import "std:net"
import "std:time"
import "std:types"
fn main() -> Int:
    let today = today_local()
    match process_run("git", ["--version"], ""):
        Ok(result):
            print("ok")
        Err(problem):
            print(problem.code)
    match tcp_connect("db.example.test", 5432, 500):
        Ok(conn):
            print("connected")
        Err(problem):
            print(problem.code)
    let headers: Map[Text, Text] = {}
    match http_request_bytes("GET", "https://api.github.com/x", headers, bytes_from_text(""), 1000):
        Ok(_response):
            print("ran")
        Err(problem):
            print(http_error_code(problem))
    return 0
"#;

#[test]
fn inspect_grants_compares_what_the_program_reaches_with_what_is_granted() {
    let project = Project::new("inspect", NEEDS_SOURCE, "[process]\ncommands = [\"git\"]\n");
    let output = project
        .guion()
        .args(["inspect", "--grants", "main.gu"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);
    assert!(text.contains("fetch from api.github.com"), "{text}");
    assert!(
        text.contains("[network] hosts = [\"api.github.com\"]"),
        "{text}"
    );
    assert!(text.contains("run git"), "{text}");
    assert!(text.contains("yes  (guion.toml [process])"), "{text}");
    assert!(text.contains("read the clock"), "{text}");
    assert!(text.contains("connect to db.example.test:5432"), "{text}");
}

#[test]
fn inspect_grants_json_lists_each_missing_grant_as_a_table_key_value_and_flag() {
    let project = Project::new(
        "inspect-json",
        NEEDS_SOURCE,
        "[process]\ncommands = [\"git\"]\n",
    );
    let output = project
        .guion()
        .args(["inspect", "--grants", "--json", "main.gu"])
        .output()
        .unwrap();
    let report = json_of(&output);
    assert_eq!(report["kind"], "guion_grants_report");
    let missing = report["missing"].as_array().unwrap();
    let flags = missing
        .iter()
        .map(|grant| grant["flag"].as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(
        flags,
        [
            "--allow-http-host api.github.com",
            "--allow-clock",
            "--allow-connect db.example.test:5432"
        ]
    );
    let git = report["needs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|need| need["target"] == "git")
        .unwrap();
    assert_eq!(git["granted"], true);
}

#[test]
fn inspect_grants_says_so_when_a_target_is_computed() {
    let source = r#"import "std:types"
fn main() -> Int:
    let headers: Map[Text, Text] = {}
    let url = "http://" + args()[0]
    match http_request_bytes("GET", url, headers, bytes_from_text(""), 1000):
        Ok(_response):
            print("ran")
        Err(problem):
            print(http_error_code(problem))
    return 0
"#;
    let project = Project::new("inspect-computed", source, "");
    let output = project
        .guion()
        .args(["inspect", "--grants", "main.gu"])
        .output()
        .unwrap();
    let text = stdout(&output);
    assert!(
        text.contains("fetch from a host chosen while running"),
        "{text}"
    );
    assert!(text.contains("known only while running"), "{text}");
    let output = project
        .guion()
        .args(["inspect", "--grants", "--json", "main.gu"])
        .output()
        .unwrap();
    let report = json_of(&output);
    assert_eq!(report["needs"][0]["computed"], true);
    assert_eq!(report["missing"], serde_json::json!([]));
}

#[test]
fn a_program_that_reaches_nothing_needs_no_grants() {
    let project = Project::new("inspect-none", "fn main() -> Int:\n    return 0\n", "");
    let output = project
        .guion()
        .args(["inspect", "--grants", "main.gu"])
        .output()
        .unwrap();
    assert!(
        stdout(&output).starts_with("needs no grants"),
        "{}",
        stdout(&output)
    );
}

// ---- Part 3: one question before the run ----
//
// These run guion inside a pseudo-terminal made by `script`, the only way to give it a real
// terminal from a test. Unix only.

#[cfg(unix)]
mod terminal {
    use super::*;
    use std::io::{Read, Write};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    /// Run `guion run` (or another subcommand) with a controlling terminal. `answer` is typed once
    /// the question `Allow?` appears; `None` closes the terminal's input at once (end of input).
    pub fn in_terminal(
        project: &Project,
        args: &[&str],
        env: &[(&str, &str)],
        answer: Option<&str>,
    ) -> (String, bool) {
        let binary = env!("CARGO_BIN_EXE_guion");
        let mut command = Command::new("script");
        if cfg!(target_os = "macos") {
            command.args(["-q", "/dev/null", binary]).args(args);
        } else {
            let line = format!("{binary} {}", args.join(" "));
            command.args(["-qec", &line, "/dev/null"]);
        }
        command
            .current_dir(project.path())
            .env_remove("CI")
            .env_remove("GUION_NO_PROMPT")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null());
        for (name, value) in env {
            command.env(name, value);
        }
        let mut child = command.spawn().expect("script starts");
        let mut stdin = child.stdin.take().unwrap();
        let mut stdout = child.stdout.take().unwrap();
        let captured = Arc::new(Mutex::new(Vec::<u8>::new()));
        let sink = Arc::clone(&captured);
        let reader = std::thread::spawn(move || {
            let mut chunk = [0u8; 4096];
            while let Ok(count) = stdout.read(&mut chunk) {
                if count == 0 {
                    break;
                }
                sink.lock().unwrap().extend_from_slice(&chunk[..count]);
            }
        });
        match answer {
            Some(answer) => {
                let deadline = Instant::now() + Duration::from_secs(30);
                loop {
                    let seen = String::from_utf8_lossy(&captured.lock().unwrap()).into_owned();
                    if seen.contains("Allow?") {
                        break;
                    }
                    assert!(Instant::now() < deadline, "no question appeared: {seen}");
                    std::thread::sleep(Duration::from_millis(20));
                }
                stdin.write_all(answer.as_bytes()).unwrap();
                stdin.flush().unwrap();
                let status = child.wait().unwrap();
                drop(stdin);
                reader.join().unwrap();
                (text(&captured), status.success())
            }
            None => {
                drop(stdin);
                let status = child.wait().unwrap();
                reader.join().unwrap();
                (text(&captured), status.success())
            }
        }
    }

    fn text(captured: &Arc<Mutex<Vec<u8>>>) -> String {
        String::from_utf8_lossy(&captured.lock().unwrap()).replace('\r', "")
    }

    #[test]
    fn yes_grants_this_run_only_and_a_project_less_file_is_not_offered_always() {
        let project = Project::new("ask-yes", CLOCK_SOURCE, "");
        let (out, ok) = in_terminal(&project, &["run", "main.gu"], &[], Some("y\n"));
        assert!(ok, "{out}");
        assert!(
            out.contains("main.gu wants to:\n  · read the clock"),
            "{out}"
        );
        assert!(
            out.contains("Allow? [y] this run  [N] no"),
            "no [a] without a guion.toml: {out}"
        );
        assert!(out.contains("ran"), "{out}");
        assert!(!project.path().join("guion.toml").exists());
    }

    #[test]
    fn enter_means_no_and_the_program_fails_at_the_first_denied_effect() {
        let project = Project::new("ask-no", CLOCK_SOURCE, "");
        let (out, ok) = in_terminal(&project, &["run", "main.gu"], &[], Some("\n"));
        assert!(!ok, "{out}");
        assert!(out.contains("running without them"), "{out}");
        assert!(out.contains("R_CLOCK_DENIED"), "{out}");
    }

    #[test]
    fn always_writes_a_minimal_diff_to_guion_toml_and_keeps_comments() {
        let before = "# my project\n[process]\n# only git\ncommands = [\"git\"] # keep\n";
        let project = Project::new("ask-always", CLOCK_SOURCE, before);
        let (out, ok) = in_terminal(&project, &["run", "main.gu"], &[], Some("a\n"));
        assert!(ok, "{out}");
        assert!(out.contains("(saves to guion.toml)"), "{out}");
        let after = std::fs::read_to_string(project.path().join("guion.toml")).unwrap();
        assert!(
            after.starts_with(before),
            "existing text must be untouched:\n{after}"
        );
        assert!(after.contains("[clock]\nenabled = true"), "{after}");
        // The saved grant now applies without a question.
        let again = project.run(&[]);
        assert_eq!(stdout(&again), "ran\n", "{}", stderr(&again));
    }

    #[test]
    fn ci_the_no_prompt_variable_and_the_flag_each_stop_the_question() {
        for (name, env, flag) in [
            ("ci", vec![("CI", "1")], None),
            ("env", vec![("GUION_NO_PROMPT", "1")], None),
            ("flag", vec![], Some("--no-prompt")),
        ] {
            let project = Project::new(&format!("ask-off-{name}"), CLOCK_SOURCE, "");
            let mut args = vec!["run"];
            args.extend(flag);
            args.push("main.gu");
            let (out, ok) = in_terminal(&project, &args, &env, None);
            assert!(!out.contains("wants to"), "{name}: {out}");
            assert!(!ok && out.contains("R_CLOCK_DENIED"), "{name}: {out}");
        }
    }

    #[test]
    fn json_runs_and_replays_never_ask() {
        let project = Project::new("ask-never", CLOCK_SOURCE, "");
        let (out, _) = in_terminal(&project, &["run", "--json", "main.gu"], &[], None);
        assert!(!out.contains("wants to"), "{out}");
        let recorded = project.run(&["--allow-clock", "--record", "t.tape"]);
        assert!(recorded.status.success(), "{}", stderr(&recorded));
        let (out, _) = in_terminal(
            &project,
            &["run", "--replay", "t.tape", "main.gu"],
            &[],
            None,
        );
        assert!(!out.contains("wants to"), "{out}");
    }
}

#[test]
fn piped_input_cannot_answer_the_question() {
    let project = Project::new("ask-piped", CLOCK_SOURCE, "");
    let mut child = project
        .guion()
        .env_remove("GUION_NO_PROMPT")
        .args(["run", "main.gu"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    {
        use std::io::Write;
        let mut stdin = child.stdin.take().unwrap();
        // The child may exit before reading; a closed pipe here is fine.
        let _ = stdin.write_all(&b"y\n".repeat(64));
    }
    let output = child.wait_with_output().unwrap();
    assert!(!output.status.success());
    assert!(!stdout(&output).contains("ran"));
    assert!(!stderr(&output).contains("wants to"), "{}", stderr(&output));
    assert!(stderr(&output).contains("R_CLOCK_DENIED"));
}
