//! Whole-program record and replay: a tape holds every result the program got from outside,
//! and a replay runs with no permissions at all.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::process::{Command, Output};

fn guion(directory: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(args)
        .current_dir(directory)
        .output()
        .expect("guion starts")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn scratch(name: &str) -> std::path::PathBuf {
    let directory = std::env::temp_dir().join(format!("guion-tape-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).unwrap();
    directory
}

const READER: &str = r#"fn main() -> Int:
    match read_text("data.txt"):
        Ok(body):
            print(f"read {len(body)} bytes")
        Err(error):
            print("failed")
    print(f"clock {unix_time_ms()}")
    match read_text("absent.txt"):
        Ok(body):
            print("odd")
        Err(error):
            print("absent")
    print(f"args {len(args())}")
    return 0
"#;

#[test]
fn a_replay_reproduces_files_clock_and_arguments_with_nothing_granted() {
    let directory = scratch("basic");
    std::fs::write(directory.join("main.gu"), READER).unwrap();
    std::fs::write(directory.join("data.txt"), "alpha\nbeta\n").unwrap();

    let recorded = guion(
        &directory,
        &[
            "run",
            "--allow-clock",
            "--record",
            "run.tape",
            "main.gu",
            "one",
            "two",
        ],
    );
    assert!(recorded.status.success(), "{}", text(&recorded.stderr));
    let original = text(&recorded.stdout);
    assert!(original.contains("read 11 bytes"), "{original}");
    assert!(original.contains("args 2"), "{original}");

    // Remove the file, give no flags and no arguments: the tape alone feeds the program.
    std::fs::remove_file(directory.join("data.txt")).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(5));
    let replayed = guion(&directory, &["run", "--replay", "run.tape", "main.gu"]);
    assert!(replayed.status.success(), "{}", text(&replayed.stderr));
    assert_eq!(
        text(&replayed.stdout),
        original,
        "replay output must be identical"
    );
}

#[test]
fn a_changed_program_is_refused_and_the_first_differing_call_is_named() {
    let directory = scratch("changed");
    std::fs::write(directory.join("main.gu"), READER).unwrap();
    std::fs::write(directory.join("data.txt"), "alpha\nbeta\n").unwrap();
    let recorded = guion(
        &directory,
        &["run", "--allow-clock", "--record", "run.tape", "main.gu"],
    );
    assert!(recorded.status.success());

    std::fs::write(
        directory.join("main.gu"),
        READER.replace("absent.txt", "other.txt"),
    )
    .unwrap();
    let refused = guion(
        &directory,
        &["run", "--replay-strict", "run.tape", "main.gu"],
    );
    assert!(!refused.status.success());
    assert!(
        text(&refused.stderr).contains("program has changed"),
        "{}",
        text(&refused.stderr)
    );
    // A plain replay of a changed program checks each call instead, and names the one that differs.
    let by_calls = guion(&directory, &["run", "--replay", "run.tape", "main.gu"]);
    assert!(!by_calls.status.success());
    assert!(
        text(&by_calls.stderr).contains("R_TAPE_DIVERGED"),
        "{}",
        text(&by_calls.stderr)
    );

    let diverged = guion(
        &directory,
        &["run", "--replay-changed", "run.tape", "main.gu"],
    );
    assert!(!diverged.status.success());
    let message = text(&diverged.stderr);
    assert!(message.contains("R_TAPE_DIVERGED"), "{message}");
    assert!(message.contains("host call 3"), "{message}");
    assert!(message.contains("line 8"), "{message}");
    assert!(
        message.contains("other.txt") && message.contains("absent.txt"),
        "{message}"
    );
}

#[test]
fn replay_stops_when_the_program_makes_fewer_calls_than_the_tape() {
    let directory = scratch("fewer");
    std::fs::write(directory.join("main.gu"), READER).unwrap();
    std::fs::write(directory.join("data.txt"), "alpha\nbeta\n").unwrap();
    assert!(guion(
        &directory,
        &["run", "--allow-clock", "--record", "run.tape", "main.gu"]
    )
    .status
    .success());
    let shorter = READER.replace("    print(f\"args {len(args())}\")\n", "");
    std::fs::write(directory.join("main.gu"), shorter).unwrap();
    let diverged = guion(
        &directory,
        &["run", "--replay-changed", "run.tape", "main.gu"],
    );
    assert!(!diverged.status.success());
    assert!(
        text(&diverged.stderr).contains("R_TAPE_DIVERGED"),
        "{}",
        text(&diverged.stderr)
    );
}

#[test]
fn an_http_client_replays_without_the_network_and_the_tape_holds_no_credentials() {
    let directory = scratch("http");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buffer = [0u8; 4096];
            let _ = stream.read(&mut buffer);
            let body = "{\"greeting\":\"hello from the server\"}";
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
        }
    });
    let source = format!(
        r#"fn main() -> Int:
    let headers: Map[Text, Text] = {{"Authorization": "Bearer SECRET-TOKEN-123", "Accept": "application/json"}}
    match http_request("GET", "http://127.0.0.1:{port}/greet", headers, ""):
        Ok(response):
            print(json_stringify(response))
        Err(error):
            print("request failed")
    return 0
"#
    );
    std::fs::write(directory.join("main.gu"), source).unwrap();
    let recorded = guion(
        &directory,
        &[
            "run",
            "--allow-http-host",
            "127.0.0.1",
            "--record",
            "run.tape",
            "main.gu",
        ],
    );
    assert!(recorded.status.success(), "{}", text(&recorded.stderr));
    let original = text(&recorded.stdout);
    assert!(original.contains("hello from the server"), "{original}");
    server.join().unwrap();

    let tape = std::fs::read_to_string(directory.join("run.tape")).unwrap();
    assert!(
        !tape.contains("SECRET-TOKEN-123"),
        "credentials must never reach a tape"
    );
    assert!(
        !tape.contains("Bearer"),
        "credentials must never reach a tape"
    );

    // The server is gone and no host is allowed: the replay still answers.
    let replayed = guion(&directory, &["run", "--replay", "run.tape", "main.gu"]);
    assert!(replayed.status.success(), "{}", text(&replayed.stderr));
    assert_eq!(text(&replayed.stdout), original);

    // A rotated token does not make the replay diverge.
    let rotated = std::fs::read_to_string(directory.join("main.gu"))
        .unwrap()
        .replace("SECRET-TOKEN-123", "ANOTHER-TOKEN-456");
    std::fs::write(directory.join("main.gu"), rotated).unwrap();
    let again = guion(
        &directory,
        &["run", "--replay-changed", "run.tape", "main.gu"],
    );
    assert!(again.status.success(), "{}", text(&again.stderr));
    assert_eq!(text(&again.stdout), original);
}

#[test]
fn redacted_paths_keep_their_place_but_not_their_result() {
    let directory = scratch("redact");
    std::fs::write(
        directory.join("guion.toml"),
        "[project]\nname = \"redact\"\nversion = \"0.1.0\"\nentry = \"main.gu\"\n\n[record]\nredact = [\"secrets.txt\"]\n",
    )
    .unwrap();
    std::fs::write(directory.join("secrets.txt"), "hunter2-the-password").unwrap();
    std::fs::write(
        directory.join("main.gu"),
        "fn main() -> Int:\n    match read_text(\"secrets.txt\"):\n        Ok(body):\n            print(\"read\")\n        Err(error):\n            print(\"failed\")\n    return 0\n",
    )
    .unwrap();
    let recorded = guion(&directory, &["run", "--record", "run.tape", "main.gu"]);
    assert!(recorded.status.success(), "{}", text(&recorded.stderr));
    assert_eq!(text(&recorded.stdout), "read\n");
    let tape = std::fs::read_to_string(directory.join("run.tape")).unwrap();
    assert!(
        !tape.contains("hunter2"),
        "redacted results must not reach the tape"
    );
    let replayed = guion(&directory, &["run", "--replay", "run.tape", "main.gu"]);
    assert!(!replayed.status.success());
    assert!(
        text(&replayed.stderr).contains("R_TAPE_REDACTED"),
        "{}",
        text(&replayed.stderr)
    );
}

#[test]
fn project_tests_record_and_then_replay_hermetically() {
    let directory = scratch("tests");
    std::fs::write(
        directory.join("guion.toml"),
        "[project]\nname = \"hermetic\"\nversion = \"0.1.0\"\nentry = \"main.gu\"\n",
    )
    .unwrap();
    std::fs::write(
        directory.join("main.gu"),
        "fn main() -> Int:\n    return 0\n\nfn test_reads_a_file() -> Unit:\n    match read_text(\"data.txt\"):\n        Ok(body):\n            assert(len(body) == 6)\n        Err(error):\n            assert(false)\n",
    )
    .unwrap();
    std::fs::write(directory.join("data.txt"), "alpha\n").unwrap();
    let recorded = guion(&directory, &["test", "--record", "tapes", "main.gu"]);
    assert!(recorded.status.success(), "{}", text(&recorded.stderr));
    assert!(directory.join("tapes/test_reads_a_file.tape").exists());

    std::fs::remove_file(directory.join("data.txt")).unwrap();
    let replayed = guion(&directory, &["test", "--replay", "tapes", "main.gu"]);
    assert!(
        replayed.status.success(),
        "{} {}",
        text(&replayed.stdout),
        text(&replayed.stderr)
    );
    assert!(text(&replayed.stdout).contains("ok test_reads_a_file"));
}

#[test]
fn record_and_replay_cannot_be_combined() {
    let directory = scratch("both");
    std::fs::write(
        directory.join("main.gu"),
        "fn main() -> Int:\n    return 0\n",
    )
    .unwrap();
    let output = guion(
        &directory,
        &["run", "--record", "a.tape", "--replay", "b.tape", "main.gu"],
    );
    assert!(!output.status.success());
    assert!(text(&output.stderr).contains("cannot be used together"));
}

#[test]
fn a_server_replays_its_connections_without_a_socket() {
    let directory = scratch("server");
    let port = {
        let probe = TcpListener::bind("127.0.0.1:0").unwrap();
        probe.local_addr().unwrap().port()
    };
    std::fs::write(
        directory.join("main.gu"),
        format!(
            "import \"std:http_server\"\n\nfn main() -> Int:\n    let listening = http_server_listen(\"127.0.0.1\", {port})\n    if not is_ok(listening):\n        print(\"listen failed\")\n        return 1\n    let accepted = http_server_accept(unwrap(listening), 8000)\n    if not is_ok(accepted):\n        print(\"no connection\")\n        return 1\n    let request = http_server_read_request(unwrap(accepted), 5000, 1048576)\n    if is_ok(request):\n        let sent = http_server_respond_text(unwrap(request), 200, \"hello client\")\n        if is_ok(sent):\n            print(\"served\")\n    return 0\n"
        ),
    )
    .unwrap();
    let allow = format!("127.0.0.1:{port}");
    let child = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args([
            "run",
            "--allow-bind",
            &allow,
            "--record",
            "server.tape",
            "main.gu",
        ])
        .current_dir(&directory)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut answer = String::new();
    for _ in 0..40 {
        std::thread::sleep(std::time::Duration::from_millis(250));
        if let Ok(mut stream) = std::net::TcpStream::connect(("127.0.0.1", port)) {
            stream
                .write_all(b"GET /hi HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
                .unwrap();
            let _ = stream.read_to_string(&mut answer);
            break;
        }
    }
    assert!(answer.contains("hello client"), "client got: {answer:?}");
    let recorded = child.wait_with_output().unwrap();
    assert!(recorded.status.success(), "{}", text(&recorded.stderr));
    let original = text(&recorded.stdout);
    assert_eq!(original, "served\n");

    let replayed = guion(&directory, &["run", "--replay", "server.tape", "main.gu"]);
    assert!(replayed.status.success(), "{}", text(&replayed.stderr));
    assert_eq!(text(&replayed.stdout), original);
}

const FAN_OUT: &str = r#"fn load(n: Int) -> Int:
    var size = 0 - 1
    match read_text(f"item{n}.txt"):
        Ok(body):
            set size = len(body) + n
        Err(error):
            set size = 0 - 1
    return size

fn main() -> Int:
    let sizes = parallel_map([1, 2, 3, 4, 5, 6], load, 4)
    match sizes:
        Ok(values):
            for value in values:
                print(f"{value}")
        Err(error):
            print("failed")
    return 0
"#;

fn fan_out_directory(name: &str, program: &str) -> std::path::PathBuf {
    let directory = scratch(name);
    std::fs::write(directory.join("main.gu"), program).unwrap();
    for n in 1..=6 {
        std::fs::write(directory.join(format!("item{n}.txt")), "x".repeat(n * 3)).unwrap();
    }
    directory
}

#[test]
fn parallel_map_items_keep_their_own_lanes_on_the_tape_and_replay_with_any_worker_count() {
    let directory = fan_out_directory("lanes", FAN_OUT);
    let recorded = guion(&directory, &["run", "--record", "fan.tape", "main.gu"]);
    assert!(recorded.status.success(), "{}", text(&recorded.stderr));
    let original = text(&recorded.stdout);
    assert_eq!(original, "4\n8\n12\n16\n20\n24\n");

    let tape = std::fs::read_to_string(directory.join("fan.tape")).unwrap();
    assert!(tape.starts_with("{\"guion_tape\":2"), "{tape}");
    for item in 0..6 {
        assert!(tape.contains(&format!("\"lane\":\"0.{item}\"")), "{tape}");
    }

    for _ in 0..5 {
        for n in 1..=6 {
            let _ = std::fs::remove_file(directory.join(format!("item{n}.txt")));
        }
        let replayed = guion(&directory, &["run", "--replay", "fan.tape", "main.gu"]);
        assert!(replayed.status.success(), "{}", text(&replayed.stderr));
        assert_eq!(text(&replayed.stdout), original);
    }
}

#[test]
fn a_program_without_parallel_work_still_writes_the_first_tape_format() {
    let directory = scratch("plain-version");
    std::fs::write(directory.join("main.gu"), READER).unwrap();
    std::fs::write(directory.join("data.txt"), "alpha").unwrap();
    let recorded = guion(
        &directory,
        &["run", "--allow-clock", "--record", "plain.tape", "main.gu"],
    );
    assert!(recorded.status.success(), "{}", text(&recorded.stderr));
    let tape = std::fs::read_to_string(directory.join("plain.tape")).unwrap();
    assert!(tape.starts_with("{\"guion_tape\":1"), "{tape}");
    assert!(!tape.contains("\"lane\""), "{tape}");
}

#[test]
fn a_divergence_inside_a_parallel_item_names_its_lane_even_though_the_program_sees_an_err() {
    let directory = fan_out_directory("lane-divergence", FAN_OUT);
    let recorded = guion(&directory, &["run", "--record", "fan.tape", "main.gu"]);
    assert!(recorded.status.success(), "{}", text(&recorded.stderr));
    // The same program reading another file name in each item.
    std::fs::write(
        directory.join("main.gu"),
        FAN_OUT.replace("item{n}.txt", "other{n}.txt"),
    )
    .unwrap();
    let replayed = guion(
        &directory,
        &["run", "--replay-changed", "fan.tape", "main.gu"],
    );
    assert!(!replayed.status.success());
    let error = text(&replayed.stderr);
    assert!(error.contains("R_TAPE_DIVERGED"), "{error}");
    assert!(error.contains("parallel_map lane 0."), "{error}");
}

#[test]
fn an_item_that_fails_replays_to_the_same_failure_whatever_its_siblings_did() {
    let program = FAN_OUT.replace(
        "    return size\n",
        "    if n == 3:\n        let boom = 1 / (n - 3)\n    return size\n",
    );
    let directory = fan_out_directory("lane-failure", &program);
    let recorded = guion(&directory, &["run", "--record", "fan.tape", "main.gu"]);
    assert!(recorded.status.success(), "{}", text(&recorded.stderr));
    let original = text(&recorded.stdout);
    assert_eq!(original, "failed\n");
    for _ in 0..10 {
        let replayed = guion(&directory, &["run", "--replay", "fan.tape", "main.gu"]);
        assert!(replayed.status.success(), "{}", text(&replayed.stderr));
        assert_eq!(text(&replayed.stdout), original);
    }
}

const TWO_READS: &str = r#"fn first() -> Text:
    match read_text("a.txt"):
        Ok(body):
            return body
        Err(error):
            return "none"

fn main() -> Int:
    let a = first()
    match read_text("b.txt"):
        Ok(body):
            print(f"{a} {body}")
        Err(error):
            print("failed")
    return 0
"#;

fn record_two_reads(name: &str) -> std::path::PathBuf {
    let directory = scratch(name);
    std::fs::write(directory.join("main.gu"), TWO_READS).unwrap();
    std::fs::write(directory.join("a.txt"), "A").unwrap();
    std::fs::write(directory.join("b.txt"), "B").unwrap();
    let recorded = guion(&directory, &["run", "--record", "run.tape", "main.gu"]);
    assert!(recorded.status.success(), "{}", text(&recorded.stderr));
    directory
}

#[test]
fn a_tape_replays_after_an_edit_that_leaves_every_outside_call_alone() {
    let directory = record_two_reads("edit-safe");
    // Comments and a new local move every line and change the stack, but not one call.
    let edited = format!("# a note\n# another note\n{}", TWO_READS)
        .replace("let a = first()", "let extra = 41\n    let a = first()")
        .replace("print(f\"{a} {body}\")", "print(f\"{a} {body} {extra}\")");
    std::fs::write(directory.join("main.gu"), edited).unwrap();
    let replayed = guion(&directory, &["run", "--replay", "run.tape", "main.gu"]);
    let stderr = text(&replayed.stderr);
    assert!(replayed.status.success(), "{stderr}");
    assert_eq!(text(&replayed.stdout), "A B 41\n");
    assert!(stderr.contains("program hash differs"), "{stderr}");
    assert!(stderr.contains("--replay-strict"), "{stderr}");

    let strict = guion(
        &directory,
        &["run", "--replay-strict", "run.tape", "main.gu"],
    );
    assert!(!strict.status.success());
    assert!(text(&strict.stderr).contains("program has changed"));
}

#[test]
fn a_json_run_report_carries_the_relaxed_replay_notice() {
    let directory = record_two_reads("edit-json");
    // An exact replay has no notice.
    let exact = guion(
        &directory,
        &["run", "--json", "--replay", "run.tape", "main.gu"],
    );
    let report: serde_json::Value = serde_json::from_str(&text(&exact.stdout)).unwrap();
    assert_eq!(report["status"], "passed", "{report}");
    assert!(report["tape_notice"].is_null(), "{report}");
    // Edit the program without touching any outside call: the replay is relaxed, and says so.
    let edited = format!("# a note\n{TWO_READS}");
    std::fs::write(directory.join("main.gu"), edited).unwrap();
    let relaxed = guion(
        &directory,
        &["run", "--json", "--replay", "run.tape", "main.gu"],
    );
    let report: serde_json::Value = serde_json::from_str(&text(&relaxed.stdout)).unwrap();
    assert_eq!(report["status"], "passed", "{report}");
    let notice = report["tape_notice"]
        .as_str()
        .expect("a tape_notice string");
    assert!(notice.contains("program hash differs"), "{notice}");
    assert!(notice.contains("--replay-strict"), "{notice}");
    // JSON mode keeps stderr clean of the note.
    assert!(!text(&relaxed.stderr).contains("program hash differs"));
}

#[test]
fn an_edit_that_changes_what_a_call_is_given_still_diverges() {
    let directory = record_two_reads("edit-arg");
    std::fs::write(
        directory.join("main.gu"),
        TWO_READS.replace("b.txt", "c.txt"),
    )
    .unwrap();
    let replayed = guion(&directory, &["run", "--replay", "run.tape", "main.gu"]);
    assert!(!replayed.status.success());
    let stderr = text(&replayed.stderr);
    assert!(stderr.contains("R_TAPE_DIVERGED"), "{stderr}");
    assert!(stderr.contains("c.txt"), "{stderr}");
}

#[test]
fn an_edit_that_reorders_calls_still_diverges() {
    let directory = record_two_reads("edit-order");
    let swapped = TWO_READS
        .replace("a.txt", "TMP")
        .replace("b.txt", "a.txt")
        .replace("TMP", "b.txt");
    std::fs::write(directory.join("main.gu"), swapped).unwrap();
    let replayed = guion(&directory, &["run", "--replay", "run.tape", "main.gu"]);
    assert!(!replayed.status.success());
    assert!(text(&replayed.stderr).contains("R_TAPE_DIVERGED"));
}

#[test]
fn an_edit_that_drops_or_adds_a_call_still_diverges() {
    let directory = record_two_reads("edit-count");
    std::fs::write(
        directory.join("main.gu"),
        TWO_READS.replace("let a = first()", "let a = \"x\""),
    )
    .unwrap();
    let fewer = guion(&directory, &["run", "--replay", "run.tape", "main.gu"]);
    assert!(!fewer.status.success());
    assert!(text(&fewer.stderr).contains("R_TAPE_DIVERGED"));
}

#[test]
fn an_unchanged_program_replays_without_a_notice() {
    let directory = record_two_reads("edit-none");
    let replayed = guion(&directory, &["run", "--replay", "run.tape", "main.gu"]);
    assert!(replayed.status.success());
    assert!(!text(&replayed.stderr).contains("program hash differs"));
}
