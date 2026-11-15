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
