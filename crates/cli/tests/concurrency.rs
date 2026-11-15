//! Structured concurrency for waiting work. See `docs/design-structured-concurrency.md`.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn run_project(name: &str, manifest: &str, flags: &[&str], source: &str) -> Output {
    let dir = std::env::temp_dir().join(format!("guion-conc-{name}-{}", std::process::id()));
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
        .env("GUION_NO_PROMPT", "1")
        .output()
        .expect("guion starts");
    let _ = std::fs::remove_dir_all(&dir);
    output
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

/// A local server that answers every request after `delay`, counting the most requests it ever
/// had open at once.
struct SlowServer {
    address: String,
    peak: Arc<AtomicUsize>,
}

fn slow_server(delay: Duration) -> SlowServer {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap().to_string();
    let open = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let (open_for_thread, peak_for_thread) = (open.clone(), peak.clone());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let (open, peak) = (open_for_thread.clone(), peak_for_thread.clone());
            std::thread::spawn(move || {
                let now = open.fetch_add(1, Ordering::SeqCst) + 1;
                peak.fetch_max(now, Ordering::SeqCst);
                let mut buffer = [0u8; 2048];
                let read = stream.read(&mut buffer).unwrap_or(0);
                if String::from_utf8_lossy(&buffer[..read]).starts_with("GET /hang") {
                    // A server that takes the request and never answers.
                    std::thread::sleep(Duration::from_secs(25));
                }
                std::thread::sleep(delay);
                let _ = stream.write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
                );
                open.fetch_sub(1, Ordering::SeqCst);
            });
        }
    });
    SlowServer { address, peak }
}

fn fetch_all_source(address: &str, count: usize, workers: usize) -> String {
    format!(
        "fn fetch(url: Text) -> Int:\n    if is_ok(http_get(url)):\n        return 1\n    return 0\n\nfn main() -> Int:\n    var urls: List[Text] = []\n    var i = 0\n    while i < {count}:\n        set urls = list_push(urls, \"http://{address}/\")\n        set i = i + 1\n    let mapped = parallel_map(urls, fetch, {workers})\n    if not is_ok(mapped):\n        print(task_code(error(mapped)))\n        return 1\n    var total = 0\n    for each in unwrap(mapped):\n        set total = total + each\n    print(total)\n    return 0\n"
    )
}

// ---- Part 1: fan-out is not capped by cores ----

#[test]
fn fifty_waiting_requests_run_at_once_whatever_the_core_count() {
    let server = slow_server(Duration::from_millis(600));
    let started = Instant::now();
    let output = run_project(
        "fifty",
        "",
        &["--allow-http-host", "127.0.0.1"],
        &fetch_all_source(&server.address, 50, 50),
    );
    let elapsed = started.elapsed();
    assert_eq!(stdout(&output), "50", "{}", stderr(&output));
    // Fifty requests of 0.6 s one after another would take 30 s; four at a time, 7.5 s.
    assert!(elapsed < Duration::from_secs(6), "took {elapsed:?}");
    assert!(
        server.peak.load(Ordering::SeqCst) >= 40,
        "only {} requests were open at once",
        server.peak.load(Ordering::SeqCst)
    );
}

#[test]
fn a_limit_above_the_ceiling_names_the_line_that_raises_it() {
    let output = run_project(
        "above",
        "",
        &[],
        "fn id(n: Int) -> Int:\n    return n\n\nfn main() -> Int:\n    let mapped = parallel_map([1, 2], id, 200)\n    return 0\n",
    );
    assert!(!output.status.success());
    let text = stderr(&output);
    assert!(text.contains("R_PARALLEL_LIMIT"), "{text}");
    assert!(
        text.contains("worker limit 200 is above this project's limit of 64"),
        "{text}"
    );
    assert!(text.contains("[runtime] max_parallelism = 200"), "{text}");
}

#[test]
fn a_project_can_raise_the_ceiling_up_to_1024_and_no_further() {
    let source = "fn id(n: Int) -> Int:\n    return n\n\nfn main() -> Int:\n    let mapped = parallel_map([1, 2, 3], id, 200)\n    print(len(unwrap(mapped)))\n    return 0\n";
    let raised = run_project(
        "raised",
        "[package]\nname = \"c\"\nversion = \"0.1.0\"\n[runtime]\nmax_parallelism = 200\n",
        &[],
        source,
    );
    assert_eq!(stdout(&raised), "3", "{}", stderr(&raised));

    let too_high = run_project(
        "too-high",
        "[package]\nname = \"c\"\nversion = \"0.1.0\"\n[runtime]\nmax_parallelism = 2000\n",
        &[],
        source,
    );
    assert!(!too_high.status.success());
    assert!(
        stderr(&too_high).contains("at most 1024"),
        "{}",
        stderr(&too_high)
    );
}

#[test]
fn core_count_reports_this_machines_cores() {
    let output = run_project(
        "cores",
        "",
        &[],
        "import \"std:process\"\n\nfn main() -> Int:\n    print(core_count())\n    return 0\n",
    );
    let expected = std::thread::available_parallelism().unwrap().get();
    assert_eq!(stdout(&output), expected.to_string(), "{}", stderr(&output));
}

// ---- Part 2: deadlines ----

const SUMMARY: &str = "fn show(results: List[Result[Int, TaskError]]) -> Int:\n    for each in results:\n        if is_ok(each):\n            print(\"ok \" + int_to_text(unwrap(each)))\n        else:\n            print(task_code(error(each)))\n    return 0\n";

fn try_source(address: &str, urls: &[&str], limits: &str) -> String {
    let list = urls
        .iter()
        .map(|path| format!("\"http://{address}{path}\""))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "import \"std:process\"\n\nfn fetch(url: Text) -> Int:\n    if is_ok(http_get(url)):\n        return 1\n    return 0\n\n{SUMMARY}\nfn main() -> Int:\n    let urls: List[Text] = [{list}]\n    let results = try_parallel_map(urls, fetch, {limits})\n    return show(results)\n"
    )
}

#[test]
fn an_item_past_each_ms_is_r_deadline_and_a_hung_call_still_returns_on_time() {
    let server = slow_server(Duration::from_millis(10));
    let started = Instant::now();
    let output = run_project(
        "each",
        "",
        &["--allow-http-host", "127.0.0.1"],
        &try_source(
            &server.address,
            &["/a", "/hang", "/b"],
            "Workers(3, 700, 0)",
        ),
    );
    let elapsed = started.elapsed();
    assert_eq!(
        stdout(&output),
        "ok 1\nR_DEADLINE\nok 1",
        "{}",
        stderr(&output)
    );
    // The hung request would hold the item for 15 s (the HTTP timeout) or 25 s (the server).
    assert!(elapsed < Duration::from_secs(6), "took {elapsed:?}");
}

#[test]
fn all_ms_keeps_finished_results_and_stops_the_rest() {
    let server = slow_server(Duration::from_millis(10));
    let output = run_project(
        "all",
        "",
        &["--allow-http-host", "127.0.0.1"],
        &try_source(
            &server.address,
            &["/a", "/b", "/hang", "/hang", "/c"],
            "Workers(2, 0, 900)",
        ),
    );
    // Two finish; two are cut off while running; the last never starts.
    assert_eq!(
        stdout(&output),
        "ok 1\nok 1\nR_DEADLINE\nR_DEADLINE\nR_DEADLINE",
        "{}",
        stderr(&output)
    );
}

#[test]
fn an_item_that_only_computes_is_stopped_at_its_deadline_too() {
    let source = format!(
        "import \"std:process\"\n\nfn spin(count: Int) -> Int:\n    var i = 0\n    while i < count:\n        set i = i + 1\n    return i\n\n{SUMMARY}\nfn main() -> Int:\n    let results = try_parallel_map([10, 4000000000, 20], spin, Workers(3, 400, 0))\n    return show(results)\n"
    );
    let started = Instant::now();
    let output = run_project(
        "compute",
        "[package]\nname = \"c\"\nversion = \"0.1.0\"\n[runtime]\ninstruction_limit = 100000000000\n",
        &[],
        &source,
    );
    assert_eq!(
        stdout(&output),
        "ok 10\nR_DEADLINE\nok 20",
        "{}",
        stderr(&output)
    );
    assert!(started.elapsed() < Duration::from_secs(8));
}

#[test]
fn a_fault_in_one_item_is_that_items_error_and_siblings_carry_on() {
    let source = format!(
        "import \"std:process\"\n\nfn pick(n: Int) -> Int:\n    let items = [10, 20, 30]\n    return items[n]\n\n{SUMMARY}\nfn main() -> Int:\n    let results = try_parallel_map([0, 7, 2, 1], pick, Workers(2, 0, 0))\n    return show(results)\n"
    );
    let output = run_project("fault", "", &[], &source);
    let text = stdout(&output);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 4, "{text}{}", stderr(&output));
    assert_eq!(lines[0], "ok 10");
    assert!(lines[1].starts_with("R_"), "{text}");
    assert_eq!(lines[2], "ok 30");
    assert_eq!(lines[3], "ok 20");
}

#[test]
fn workers_above_the_ceiling_get_the_same_fix_it_error() {
    let source = format!(
        "import \"std:process\"\n\nfn id(n: Int) -> Int:\n    return n\n\n{SUMMARY}\nfn main() -> Int:\n    return show(try_parallel_map([1], id, Workers(200, 0, 0)))\n"
    );
    let output = run_project("try-ceiling", "", &[], &source);
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("[runtime] max_parallelism = 200"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn a_recorded_deadline_replays_with_the_same_item_timing_out_and_nothing_granted() {
    let server = slow_server(Duration::from_millis(10));
    let dir = std::env::temp_dir().join(format!("guion-conc-replay-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("main.gu"),
        try_source(
            &server.address,
            &["/a", "/hang", "/b", "/hang"],
            "Workers(2, 600, 0)",
        ),
    )
    .unwrap();
    let guion = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_guion"))
            .args(args)
            .current_dir(&dir)
            .env("GUION_NO_PROMPT", "1")
            .output()
            .expect("guion starts")
    };
    let recorded = guion(&[
        "run",
        "--allow-http-host",
        "127.0.0.1",
        "--record",
        "t.tape",
        "main.gu",
    ]);
    assert_eq!(
        stdout(&recorded),
        "ok 1\nR_DEADLINE\nok 1\nR_DEADLINE",
        "{}",
        stderr(&recorded)
    );
    for _ in 0..20 {
        let replayed = guion(&["run", "--replay", "t.tape", "main.gu"]);
        assert_eq!(
            stdout(&replayed),
            "ok 1\nR_DEADLINE\nok 1\nR_DEADLINE",
            "{}",
            stderr(&replayed)
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}
