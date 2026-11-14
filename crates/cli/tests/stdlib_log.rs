use std::path::PathBuf;
use std::process::{Command, Output};

struct Scratch(PathBuf);
impl Scratch {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("guion-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("root")).unwrap();
        Scratch(dir)
    }
    fn log(&self) -> String {
        std::fs::read_to_string(self.0.join("root").join("app.log")).unwrap_or_default()
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run(scratch: &Scratch, flags: &[&str], manifest: Option<&str>, source: &str) -> Output {
    std::fs::write(scratch.0.join("main.gu"), source).unwrap();
    if let Some(manifest) = manifest {
        std::fs::write(scratch.0.join("guion.toml"), manifest).unwrap();
    }
    let root = scratch.0.join("root");
    let mut command = Command::new(env!("CARGO_BIN_EXE_guion"));
    command.arg("run");
    command.args(["--file-root", root.to_str().unwrap()]);
    command.args(flags);
    command.arg("main.gu");
    command
        .current_dir(&scratch.0)
        .output()
        .expect("guion starts")
}

fn ok(output: &Output) {
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

const HEAD: &str = "import \"std:log\"\nimport \"std:time\"\nimport \"std:types\"\n";

fn program(body: &str) -> String {
    format!("{HEAD}\nfn main() -> Int:\n{body}    return 0\n")
}

#[test]
fn a_record_is_one_valid_line_with_sorted_keys_whatever_the_field_order() {
    let scratch = Scratch::new("log-order");
    let source = program(
        "    let logger = unwrap(log_open(\"app.log\", \"debug\"))\n    let fields: Map[Text, Json] = {\"zeta\": json_int(1), \"mid\": json_text(\"m\"), \"alpha\": json_bool(true)}\n    let done = log_info(logger, \"hello\", fields)\n",
    );
    ok(&run(&scratch, &[], None, &source));
    assert_eq!(
        scratch.log(),
        "{\"level\":\"info\",\"msg\":\"hello\",\"alpha\":true,\"mid\":\"m\",\"zeta\":1}\n"
    );
    let parsed: serde_json::Value = serde_json::from_str(scratch.log().trim_end()).unwrap();
    assert_eq!(parsed["zeta"], 1);
}

#[test]
fn no_clock_grant_gives_records_without_ts() {
    let scratch = Scratch::new("log-no-ts");
    let source = program(
        "    let logger = unwrap(log_open(\"app.log\", \"info\"))\n    let fields: Map[Text, Json] = {}\n    let a = log_warn(logger, \"w\", fields)\n    let b = log_error(logger, \"e\", fields)\n",
    );
    ok(&run(&scratch, &[], None, &source));
    assert_eq!(
        scratch.log(),
        "{\"level\":\"warn\",\"msg\":\"w\"}\n{\"level\":\"error\",\"msg\":\"e\"}\n"
    );
    assert!(!scratch.log().contains("\"ts\""));
}

#[test]
fn log_info_at_puts_ts_first_and_the_instant_comes_from_the_caller() {
    let scratch = Scratch::new("log-at");
    let source = program(
        "    let logger = unwrap(log_open(\"app.log\", \"info\"))\n    let fields: Map[Text, Json] = {\"k\": json_int(2)}\n    let at = unwrap(parse_timestamp(\"2026-01-02T03:04:05Z\"))\n    let done = log_info_at(logger, at, \"m\", fields)\n",
    );
    ok(&run(&scratch, &[], None, &source));
    assert_eq!(
        scratch.log(),
        "{\"ts\":\"2026-01-02T03:04:05Z\",\"level\":\"info\",\"msg\":\"m\",\"k\":2}\n"
    );
}

#[test]
fn log_info_now_is_refused_without_the_clock_grant_and_works_with_it() {
    let scratch = Scratch::new("log-now");
    let source = program(
        "    print(\"before\")\n    let logger = unwrap(log_open(\"app.log\", \"info\"))\n    let fields: Map[Text, Json] = {}\n    let done = log_info_now(logger, \"m\", fields)\n    print(\"after\")\n",
    );
    let denied = run(&scratch, &[], None, &source);
    assert!(!denied.status.success());
    assert!(String::from_utf8_lossy(&denied.stderr).contains("R_CLOCK_DENIED"));
    assert!(!stdout(&denied).contains("after"));
    assert_eq!(scratch.log(), "", "a refused record writes nothing");

    let granted = run(&scratch, &["--allow-clock"], None, &source);
    ok(&granted);
    let line = scratch.log();
    let parsed: serde_json::Value = serde_json::from_str(line.trim_end()).unwrap();
    assert!(parsed["ts"].as_str().unwrap().ends_with('Z'));
    assert!(line.starts_with("{\"ts\":\""));
    assert_eq!(line.matches('\n').count(), 1);
}

#[test]
fn inspect_reports_the_clock_and_output_effects_of_the_logging_functions() {
    let scratch = Scratch::new("log-inspect");
    let source = program(
        "    let logger = unwrap(log_open(\"app.log\", \"info\"))\n    let fields: Map[Text, Json] = {}\n    let done = log_info_now(logger, \"m\", fields)\n",
    );
    std::fs::write(scratch.0.join("main.gu"), &source).unwrap();
    let report = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["inspect", "--json", "main.gu"])
        .current_dir(&scratch.0)
        .output()
        .unwrap();
    ok(&report);
    let report: serde_json::Value = serde_json::from_slice(&report.stdout).unwrap();
    assert_eq!(report["clock_grant"]["required_for_entry"], true);
    let text = report.to_string();
    assert!(text.contains("output.write"), "{text}");
}

#[test]
fn an_ungranted_root_gives_a_typed_error() {
    let scratch = Scratch::new("log-ungranted");
    // A path outside the granted file root is a typed error, and nothing is created.
    let outside = run(
        &scratch,
        &[],
        None,
        &program(
            "    match log_open(\"../escape.log\", \"info\"):\n        Ok(logger):\n            print(\"opened\")\n        Err(problem):\n            print(problem.code)\n",
        ),
    );
    ok(&outside);
    assert_eq!(stdout(&outside).trim(), "R_HOST_DENIED");
    assert!(!scratch.0.join("escape.log").exists());
}

#[test]
fn an_unknown_level_is_a_typed_error() {
    let scratch = Scratch::new("log-level-name");
    let output = run(
        &scratch,
        &[],
        None,
        &program(
            "    match log_open(\"app.log\", \"verbose\"):\n        Ok(logger):\n            print(\"opened\")\n        Err(problem):\n            print(problem.code)\n",
        ),
    );
    ok(&output);
    assert_eq!(stdout(&output).trim(), "E_LOG_LEVEL");
}

#[test]
fn every_level_filters_deterministically() {
    for (level, expected) in [
        ("debug", vec!["debug", "info", "warn", "error"]),
        ("info", vec!["info", "warn", "error"]),
        ("warn", vec!["warn", "error"]),
        ("error", vec!["error"]),
    ] {
        let scratch = Scratch::new(&format!("log-filter-{level}"));
        let source = program(&format!(
            "    let logger = unwrap(log_open(\"app.log\", \"{level}\"))\n    let fields: Map[Text, Json] = {{}}\n    let a = log_debug(logger, \"d\", fields)\n    let b = log_info(logger, \"i\", fields)\n    let c = log_warn(logger, \"w\", fields)\n    let d = log_error(logger, \"e\", fields)\n"
        ));
        ok(&run(&scratch, &[], None, &source));
        let levels: Vec<String> = scratch
            .log()
            .lines()
            .map(|line| {
                let value: serde_json::Value = serde_json::from_str(line).unwrap();
                value["level"].as_str().unwrap().to_owned()
            })
            .collect();
        assert_eq!(levels, expected, "level {level}");
    }
}

#[test]
fn the_manifest_level_is_the_default_and_a_bad_one_is_rejected() {
    let scratch = Scratch::new("log-manifest");
    let source = program(
        "    let logger = unwrap(log_open(\"app.log\", \"\"))\n    let fields: Map[Text, Json] = {}\n    let a = log_info(logger, \"i\", fields)\n    let b = log_warn(logger, \"w\", fields)\n",
    );
    ok(&run(
        &scratch,
        &[],
        Some("[log]\nlevel = \"warn\"\n"),
        &source,
    ));
    assert_eq!(scratch.log(), "{\"level\":\"warn\",\"msg\":\"w\"}\n");

    let scratch = Scratch::new("log-manifest-default");
    ok(&run(&scratch, &[], None, &source));
    assert_eq!(scratch.log().lines().count(), 2, "default level is info");

    let scratch = Scratch::new("log-manifest-bad");
    let bad = run(&scratch, &[], Some("[log]\nlevel = \"loud\"\n"), &source);
    assert!(!bad.status.success());
    assert!(String::from_utf8_lossy(&bad.stderr).contains("[log]"));
    let unknown = run(&scratch, &[], Some("[log]\nsink = \"x\"\n"), &source);
    assert!(!unknown.status.success());
    assert!(String::from_utf8_lossy(&unknown.stderr).contains("[log]"));
}

#[test]
fn a_filtered_out_record_formats_nothing() {
    let scratch = Scratch::new("log-lazy");
    // Two 640-byte values fit the 1,000-byte text limit one at a time but not as one record, so
    // formatting the record fails at run time. Below the logger's level nothing formats it.
    let source = program(
        "    let logger = unwrap(log_open(\"app.log\", \"error\"))\n    var big = \"xxxxxxxxxx\"\n    var count = 0\n    while count < 6:\n        set big = big + big\n        set count = count + 1\n    let fields: Map[Text, Json] = {\"a\": json_text(big), \"b\": json_text(big)}\n    let done = log_debug(logger, \"skipped\", fields)\n    print(\"survived\")\n",
    );
    let manifest = "[runtime]\nmax_text_bytes = 1000\n";
    let filtered = run(&scratch, &[], Some(manifest), &source);
    ok(&filtered);
    assert!(stdout(&filtered).contains("survived"));
    assert_eq!(scratch.log(), "");
    // The same field at an enabled level is formatted, and the oversized value is what fails.
    let source = source.replace("log_debug", "log_error");
    let formatted = run(&scratch, &[], Some(manifest), &source);
    assert!(
        !formatted.status.success(),
        "expected the oversized field to fail when formatted: {}",
        stdout(&formatted)
    );
}

#[test]
fn newline_quote_nul_and_surrogate_pair_values_stay_on_one_valid_line() {
    let scratch = Scratch::new("log-escape");
    let source = program(
        "    let logger = unwrap(log_open(\"app.log\", \"info\"))\n    let parsed = unwrap(json_parse(\"{\\\"nul\\\":\\\"a\\\\u0000b\\\",\\\"pair\\\":\\\"\\\\ud83d\\\\ude00\\\"}\"))\n    let fields: Map[Text, Json] = {\"nl\": json_text(\"a\\nb\\r\\nc\"), \"q\": json_text(\"say \\\"hi\\\" \\\\ done\"), \"nul\": json_get(parsed, \"nul\"), \"pair\": json_get(parsed, \"pair\")}\n    let done = log_info(logger, \"line1\\nline2 \\\"quoted\\\"\", fields)\n",
    );
    ok(&run(&scratch, &[], None, &source));
    let log = scratch.log();
    assert_eq!(log.matches('\n').count(), 1, "{log:?}");
    assert!(!log.contains('\r'));
    let parsed: serde_json::Value = serde_json::from_str(log.trim_end()).unwrap();
    assert_eq!(parsed["msg"], "line1\nline2 \"quoted\"");
    assert_eq!(parsed["nl"], "a\nb\r\nc");
    assert_eq!(parsed["q"], "say \"hi\" \\ done");
    assert_eq!(parsed["nul"], "a\u{0}b");
    assert_eq!(parsed["pair"], "\u{1F600}");
}

#[test]
fn a_lone_surrogate_cannot_enter_a_field_it_is_a_typed_json_error() {
    let scratch = Scratch::new("log-surrogate");
    let source = program(
        "    match json_parse(\"\\\"x\\\\ud800y\\\"\"):\n        Ok(value):\n            print(\"parsed\")\n        Err(problem):\n            print(json_error_code(problem))\n",
    );
    let output = run(&scratch, &[], None, &source);
    ok(&output);
    assert_eq!(stdout(&output).trim(), "R_JSON_PARSE");
}

#[test]
fn a_field_named_like_a_reserved_key_is_renamed_so_keys_never_repeat() {
    let scratch = Scratch::new("log-reserved");
    let source = program(
        "    let logger = unwrap(log_open(\"app.log\", \"info\"))\n    let fields: Map[Text, Json] = {\"msg\": json_text(\"f\"), \"level\": json_int(1), \"ts\": json_int(2)}\n    let done = log_info(logger, \"m\", fields)\n",
    );
    ok(&run(&scratch, &[], None, &source));
    assert_eq!(
        scratch.log(),
        "{\"level\":\"info\",\"msg\":\"m\",\"field.level\":1,\"field.msg\":\"f\",\"field.ts\":2}\n"
    );
}

#[test]
fn records_append_across_loggers_and_runs() {
    let scratch = Scratch::new("log-append");
    let source = program(
        "    let logger = unwrap(log_open(\"app.log\", \"info\"))\n    let fields: Map[Text, Json] = {}\n    let done = log_info(logger, \"m\", fields)\n",
    );
    ok(&run(&scratch, &[], None, &source));
    ok(&run(&scratch, &[], None, &source));
    assert_eq!(scratch.log().lines().count(), 2);
}
