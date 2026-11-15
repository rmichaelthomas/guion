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
                let _ = stream.read(&mut buffer);
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
