use std::process::Command;

fn run_project(name: &str, manifest: &str, flags: &[&str], source: &str) -> std::process::Output {
    let dir = std::env::temp_dir().join(format!("guion-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("main.gu"), source).unwrap();
    if !manifest.is_empty() {
        std::fs::write(dir.join("guion.toml"), manifest).unwrap();
    }
    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .arg("run")
        .args(flags)
        .arg("main.gu")
        .current_dir(&dir)
        .env("GUION_TEST_SECRET", "hunter2")
        .output()
        .expect("guion starts");
    let _ = std::fs::remove_dir_all(&dir);
    output
}

fn assert_ok(output: &std::process::Output) {
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn commands_are_denied_without_a_grant_and_the_error_names_the_grant() {
    let source = r#"import "std:process"
import "std:types"
fn main() -> Int:
    match process_run("echo", ["hi"], ""):
        Ok(result):
            return 1
        Err(problem):
            assert(problem.code == "E_PROCESS_DENIED")
            assert(text_find(problem.message, "--allow-command", 0) >= 0)
    return 0
"#;
    assert_ok(&run_project("proc-deny", "", &[], source));
    // A grant for one command does not extend to another.
    let source = r#"import "std:process"
import "std:types"
fn main() -> Int:
    assert(is_ok(process_run("echo", ["hi"], "")))
    assert(not is_ok(process_run("cat", [], "")))
    return 0
"#;
    assert_ok(&run_project(
        "proc-exact",
        "",
        &["--allow-command", "echo"],
        source,
    ));
}

#[test]
fn granted_commands_pass_arguments_without_a_shell_and_capture_output() {
    let source = r#"import "std:process"
fn main() -> Int:
    let echoed = unwrap(process_run("echo", ["a b", "$HOME", ";", "x"], ""))
    assert(echoed.exit_code == 0)
    assert(echoed.stdout == "a b $HOME ; x\n")
    let piped = unwrap(process_run("cat", [], "from stdin"))
    assert(piped.stdout == "from stdin")
    let failed = unwrap(process_run("sh", ["-c", "echo oops >&2; exit 3"], ""))
    assert(failed.exit_code == 3)
    assert(failed.stderr == "oops\n")
    assert(not is_ok(process_run("definitely-not-installed", [], "")))
    assert(process_pid() > 0)
    return 0
"#;
    let manifest =
        "[process]\ncommands = [\"echo\", \"cat\", \"sh\", \"definitely-not-installed\"]\n";
    assert_ok(&run_project("proc-run", manifest, &[], source));
}

#[test]
fn children_get_a_cleared_environment_unless_a_variable_is_granted() {
    let source = r#"import "std:process"
fn main() -> Int:
    let seen = unwrap(process_run("sh", ["-c", "printf %s \"$GUION_TEST_SECRET\""], ""))
    assert(seen.stdout == "SECRET_VALUE")
    return 0
"#;
    let hidden = source.replace("SECRET_VALUE", "");
    assert_ok(&run_project(
        "proc-env-hidden",
        "[process]\ncommands = [\"sh\"]\n",
        &[],
        &hidden,
    ));
    let shown = source.replace("SECRET_VALUE", "hunter2");
    assert_ok(&run_project(
        "proc-env-granted",
        "[process]\ncommands = [\"sh\"]\n[environment]\nvariables = [\"GUION_TEST_SECRET\"]\n",
        &[],
        &shown,
    ));
}

#[test]
fn timeouts_and_output_caps_kill_the_child_and_return_stable_errors() {
    let source = r#"import "std:process"
import "std:types"
fn code_of(result: Result[ProcessResult, StdError]) -> Text:
    match result:
        Ok(value):
            return "ok"
        Err(problem):
            return problem.code
fn main() -> Int:
    assert(code_of(process_run("sleep", ["5"], "")) == "E_PROCESS_TIMEOUT")
    assert(code_of(process_run("yes", [], "")) == "E_PROCESS_OUTPUT_LIMIT")
    return 0
"#;
    let manifest =
        "[process]\ncommands = [\"sleep\", \"yes\"]\ntimeout_ms = 300\nmax_output_bytes = 4096\n";
    let started = std::time::Instant::now();
    assert_ok(&run_project("proc-limits", manifest, &[], source));
    assert!(
        started.elapsed() < std::time::Duration::from_secs(4),
        "the child should be killed, not waited for"
    );
}

#[cfg(unix)]
#[test]
fn signals_are_polled_and_cleared() {
    let source = r#"import "std:process"
fn main() -> Int:
    assert(process_signal_poll() == "")
    let pid = int_to_text(process_pid())
    assert(is_ok(process_run("kill", ["-INT", pid], "")))
    assert(process_signal_poll() == "INT")
    assert(process_signal_poll() == "")
    assert(is_ok(process_run("kill", ["-TERM", pid], "")))
    assert(process_signal_poll() == "TERM")
    return 0
"#;
    assert_ok(&run_project(
        "proc-signal",
        "[process]\ncommands = [\"kill\"]\n",
        &[],
        source,
    ));
}

#[test]
fn inspection_reports_the_process_effect_and_grant() {
    let dir = std::env::temp_dir().join(format!("guion-proc-inspect-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("main.gu"),
        "import \"std:process\"\nfn main() -> Int:\n    let ignored = process_run(\"echo\", [], \"\")\n    return 0\n",
    )
    .unwrap();
    std::fs::write(dir.join("guion.toml"), "[process]\ncommands = [\"echo\"]\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["inspect", "--json", "main.gu"])
        .current_dir(&dir)
        .output()
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert_ok(&output);
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let effects = report["required_effects"].to_string();
    assert!(effects.contains("process.spawn"), "{effects}");
    assert_eq!(report["process_grant"]["required_for_entry"], true);
    assert_eq!(
        report["manifest_grants"]["process_commands"],
        serde_json::json!(["echo"])
    );
}
