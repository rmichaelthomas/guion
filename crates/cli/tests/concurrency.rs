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
                if String::from_utf8_lossy(&buffer[..read]).starts_with("GET /slow") {
                    std::thread::sleep(Duration::from_millis(700));
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
            "Workers(at_once=3, each_ms=700, all_ms=0)",
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
            "Workers(all_ms=900, at_once=2, each_ms=0)",
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
        "import \"std:process\"\n\nfn spin(count: Int) -> Int:\n    var i = 0\n    while i < count:\n        set i = i + 1\n    return i\n\n{SUMMARY}\nfn main() -> Int:\n    let results = try_parallel_map([10, 4000000000, 20], spin, Workers(at_once=3, each_ms=400, all_ms=0))\n    return show(results)\n"
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

// ---- Part 3: together:, start, wait, wait_within ----

fn project_failure(name: &str, source: &str) -> String {
    let output = run_project(name, "", &[], source);
    assert!(!output.status.success(), "{}", stdout(&output));
    stderr(&output)
}

#[test]
fn tasks_run_in_the_background_and_wait_gives_their_results() {
    let output = run_project(
        "tg-basic",
        "",
        &[],
        "fn double(n: Int) -> Int:\n    return n * 2\n\nfn triple(n: Int) -> Int:\n    return n * 3\n\nfn main() -> Int:\n    var total = 0\n    together:\n        let a = start(double, 21)\n        let b = start(triple, 5)\n        let x = wait(a)\n        let y = wait(b)\n        if is_ok(x) and is_ok(y):\n            set total = unwrap(x) + unwrap(y)\n    print(total)\n    return 0\n",
    );
    assert_eq!(stdout(&output), "57", "{}", stderr(&output));
}

#[test]
fn a_fault_in_a_task_is_its_own_err_and_siblings_are_untouched() {
    let output = run_project(
        "tg-fault",
        "",
        &[],
        "fn pick(n: Int) -> Int:\n    let items = [10, 20]\n    return items[n]\n\nfn main() -> Int:\n    together:\n        let a = start(pick, 1)\n        let b = start(pick, 9)\n        let c = start(pick, 0)\n        let x = wait(a)\n        let y = wait(b)\n        let z = wait(c)\n        print(is_ok(x))\n        print(is_ok(y))\n        print(is_ok(z))\n    return 0\n",
    );
    assert_eq!(stdout(&output), "true\nfalse\ntrue", "{}", stderr(&output));
}

#[test]
fn a_task_not_waited_for_is_stopped_when_its_block_ends() {
    // The task would spin for far longer than the test allows.
    let started = Instant::now();
    let output = run_project(
        "tg-unwaited",
        "[package]\nname = \"c\"\nversion = \"0.1.0\"\n[runtime]\ninstruction_limit = 100000000000\n",
        &[],
        "fn spin(n: Int) -> Int:\n    var i = 0\n    while i < n:\n        set i = i + 1\n    return i\n\nfn main() -> Int:\n    together:\n        let watcher = start(spin, 4000000000)\n        print(\"working\")\n    print(\"done\")\n    return 0\n",
    );
    assert_eq!(stdout(&output), "working\ndone", "{}", stderr(&output));
    assert!(
        started.elapsed() < Duration::from_secs(8),
        "{:?}",
        started.elapsed()
    );
}

#[test]
fn wait_within_gives_up_with_r_deadline_and_the_block_still_ends_promptly() {
    let server = slow_server(Duration::from_millis(10));
    let started = Instant::now();
    let output = run_project(
        "tg-within",
        "",
        &["--allow-http-host", "127.0.0.1"],
        &format!(
            "fn fetch(url: Text) -> Int:\n    if is_ok(http_get(url)):\n        return 1\n    return 0\n\nfn main() -> Int:\n    together:\n        let fast = start(fetch, \"http://{0}/a\")\n        let slow = start(fetch, \"http://{0}/hang\")\n        let first = wait_within(fast, 3000)\n        let second = wait_within(slow, 400)\n        print(is_ok(first))\n        if is_ok(second):\n            print(\"slow finished\")\n        else:\n            print(task_code(error(second)))\n    return 0\n",
            server.address
        ),
    );
    assert_eq!(stdout(&output), "true\nR_DEADLINE", "{}", stderr(&output));
    assert!(
        started.elapsed() < Duration::from_secs(6),
        "{:?}",
        started.elapsed()
    );
}

#[test]
fn a_task_has_the_parents_grants_and_no_others() {
    let server = slow_server(Duration::from_millis(10));
    let source = format!(
        "fn fetch(url: Text) -> Int:\n    if is_ok(http_get(url)):\n        return 1\n    return 0\n\nfn main() -> Int:\n    together:\n        let t = start(fetch, \"http://{}/a\")\n        let r = wait(t)\n        print(unwrap(r))\n    return 0\n",
        server.address
    );
    let denied = run_project("tg-grant-no", "", &[], &source);
    assert_eq!(stdout(&denied), "0", "{}", stderr(&denied));
    let allowed = run_project(
        "tg-grant-yes",
        "",
        &["--allow-http-host", "127.0.0.1"],
        &source,
    );
    assert_eq!(stdout(&allowed), "1", "{}", stderr(&allowed));
}

#[test]
fn start_outside_a_block_and_leaving_a_block_early_are_refused_when_the_code_is_checked() {
    let outside = project_failure(
        "tg-outside",
        "fn one(n: Int) -> Int:\n    return n\n\nfn main() -> Int:\n    let t = start(one, 1)\n    return 0\n",
    );
    assert!(outside.contains("E_START_OUTSIDE_TOGETHER"), "{outside}");

    let early = project_failure(
        "tg-return",
        "fn one(n: Int) -> Int:\n    return n\n\nfn main() -> Int:\n    together:\n        let t = start(one, 1)\n        return 0\n    return 0\n",
    );
    assert!(early.contains("E_TOGETHER_EXIT"), "{early}");
}

#[test]
fn a_task_cannot_leave_its_block() {
    let printed = project_failure(
        "tg-print",
        "fn one(n: Int) -> Int:\n    return n\n\nfn main() -> Int:\n    together:\n        let t = start(one, 1)\n        print(t)\n    return 0\n",
    );
    assert!(printed.contains("E_SCOPE_ESCAPE"), "{printed}");

    let outer = project_failure(
        "tg-outer",
        "fn one(n: Int) -> Int:\n    return n\n\nfn main() -> Int:\n    together:\n        var held = start(one, 1)\n        together:\n            set held = start(one, 2)\n    return 0\n",
    );
    assert!(outer.contains("E_SCOPE_ESCAPE"), "{outer}");

    let after = project_failure(
        "tg-after",
        "fn one(n: Int) -> Int:\n    return n\n\nfn main() -> Int:\n    together:\n        let t = start(one, 1)\n    let r = wait(t)\n    return 0\n",
    );
    assert!(after.contains("E_NAME_UNKNOWN"), "{after}");
}

#[test]
fn an_error_that_unwinds_past_a_block_still_stops_its_tasks() {
    let server = slow_server(Duration::from_millis(10));
    let started = Instant::now();
    let output = run_project(
        "tg-unwind",
        "",
        &["--allow-http-host", "127.0.0.1"],
        &format!(
            "fn fetch(url: Text) -> Int:\n    if is_ok(http_get(url)):\n        return 1\n    return 0\n\nfn main() -> Int:\n    together:\n        let slow = start(fetch, \"http://{}/hang\")\n        let items = [1]\n        print(items[5])\n    return 0\n",
            server.address
        ),
    );
    assert!(!output.status.success());
    assert!(
        started.elapsed() < Duration::from_secs(8),
        "{:?}",
        started.elapsed()
    );
}

#[test]
fn a_recorded_wait_within_replays_the_same_way_with_nothing_granted() {
    let server = slow_server(Duration::from_millis(10));
    let dir = std::env::temp_dir().join(format!("guion-conc-tg-replay-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("main.gu"),
        format!(
            "fn fetch(url: Text) -> Int:\n    if is_ok(http_get(url)):\n        print(\"fetched\")\n        return 1\n    return 0\n\nfn main() -> Int:\n    together:\n        let fast = start(fetch, \"http://{0}/a\")\n        let slow = start(fetch, \"http://{0}/hang\")\n        let first = wait_within(fast, 3000)\n        let second = wait_within(slow, 400)\n        print(is_ok(first))\n        print(is_ok(second))\n    return 0\n",
            server.address
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
    let expected = "fetched\ntrue\nfalse";
    assert_eq!(stdout(&recorded), expected, "{}", stderr(&recorded));
    for _ in 0..10 {
        let replayed = guion(&["run", "--replay", "t.tape", "main.gu"]);
        assert_eq!(stdout(&replayed), expected, "{}", stderr(&replayed));
    }
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- Part 4: channels and whichever comes first ----

const PRODUCER: &str = "fn produce(out: Channel[Int]) -> Int:\n    var i = 1\n    while i <= LIMIT:\n        let sent = send(out, i)\n        set i = i + 1\n    close(out)\n    return LIMIT\n\n";

#[test]
fn a_producer_and_a_consumer_pass_every_value_in_order_through_a_small_channel() {
    let source = format!(
        "{}fn main() -> Int:\n    var total = 0\n    var count = 0\n    together:\n        let results: Channel[Int] = channel(2)\n        let producer = start(produce, results)\n        var more = true\n        while more:\n            match receive(results):\n                Some(value):\n                    set total = total + value\n                    set count = count + 1\n                None:\n                    set more = false\n        let done = wait(producer)\n    print(count)\n    print(total)\n    return 0\n",
        PRODUCER.replace("LIMIT", "200")
    );
    let output = run_project("ch-pass", "", &[], &source);
    assert_eq!(stdout(&output), "200\n20100", "{}", stderr(&output));
}

#[test]
fn a_full_channel_blocks_its_sender() {
    // Ten values into a channel of three, with nobody receiving: the producer cannot finish, and
    // when it is given up on it has put in only what the channel holds.
    let source = format!(
        "{}fn main() -> Int:\n    together:\n        let results: Channel[Int] = channel(3)\n        let producer = start(produce, results)\n        let early = wait_within(producer, 400)\n        print(is_ok(early))\n        var taken = 0\n        while taken < 3:\n            match receive(results):\n                Some(value):\n                    print(value)\n                    set taken = taken + 1\n                None:\n                    set taken = 99\n    return 0\n",
        PRODUCER.replace("LIMIT", "10")
    );
    let output = run_project("ch-full", "", &[], &source);
    assert_eq!(stdout(&output), "false\n1\n2\n3", "{}", stderr(&output));
}

#[test]
fn first_ready_names_the_task_that_finished_first_and_first_value_the_channel() {
    let server = slow_server(Duration::from_millis(10));
    let source = format!(
        "import \"std:process\"\n\nfn fetch(url: Text) -> Int:\n    if is_ok(http_get(url)):\n        return 1\n    return 0\n\nfn say(out: Channel[Int]) -> Int:\n    let sent = send(out, 7)\n    return 0\n\nfn main() -> Int:\n    together:\n        let slow = start(fetch, \"http://{0}/hang\")\n        let fast = start(fetch, \"http://{0}/a\")\n        let won = first_ready([slow, fast])\n        print(won.index)\n        print(unwrap(won.value))\n        let still = first_ready_within([slow], 300)\n        print(is_some(still))\n        let quiet: Channel[Int] = channel(1)\n        let loud: Channel[Int] = channel(1)\n        let speaker = start(say, loud)\n        let heard = first_value([quiet, loud])\n        print(heard.index)\n        print(unwrap(heard.value))\n    return 0\n",
        server.address
    );
    let output = run_project("ch-first", "", &["--allow-http-host", "127.0.0.1"], &source);
    assert_eq!(stdout(&output), "1\n1\nfalse\n1\n7", "{}", stderr(&output));
}

#[test]
fn channels_and_tasks_cannot_leave_their_block_and_a_channel_needs_its_type() {
    let returned = project_failure(
        "ch-return",
        "fn make() -> Channel[Int]:\n    together:\n        let c: Channel[Int] = channel(1)\n    return 0\n\nfn main() -> Int:\n    return 0\n",
    );
    assert!(returned.contains("E_SCOPE_ESCAPE"), "{returned}");
    let printed = project_failure(
        "ch-print",
        "fn main() -> Int:\n    together:\n        let c: Channel[Int] = channel(1)\n        print(c)\n    return 0\n",
    );
    assert!(printed.contains("E_SCOPE_ESCAPE"), "{printed}");
    let untyped = project_failure(
        "ch-untyped",
        "fn main() -> Int:\n    together:\n        let c = channel(1)\n    return 0\n",
    );
    assert!(untyped.contains("element type"), "{untyped}");
    let wrong = project_failure(
        "ch-wrong",
        "fn main() -> Int:\n    together:\n        let c: Channel[Int] = channel(1)\n        let r = send(c, \"text\")\n    return 0\n",
    );
    assert!(wrong.contains("E_TYPE_ARGUMENT"), "{wrong}");
    let outside = project_failure(
        "ch-outside",
        "fn main() -> Int:\n    let c: Channel[Int] = channel(1)\n    return 0\n",
    );
    assert!(outside.contains("E_CHANNEL_OUTSIDE_TOGETHER"), "{outside}");
}

#[test]
fn a_recorded_race_replays_with_the_same_winner_every_time() {
    let server = slow_server(Duration::from_millis(10));
    let dir = std::env::temp_dir().join(format!("guion-conc-race-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("main.gu"),
        format!(
            "import \"std:process\"\n\nfn fetch(url: Text) -> Int:\n    if is_ok(http_get(url)):\n        return 1\n    return 0\n\nfn report(job: Text, out: Channel[Text]) -> Int:\n    let n = fetch(job)\n    let sent = send(out, job)\n    return n\n\nfn first(job: Text) -> Int:\n    return fetch(job)\n\nfn main() -> Int:\n    together:\n        let slow = start(first, \"http://{0}/slow\")\n        let fast = start(first, \"http://{0}/a\")\n        let won = first_ready([slow, fast])\n        print(won.index)\n        let arrivals: Channel[Int] = channel(4)\n        let one = start(deliver_slow, arrivals)\n        let two = start(deliver_fast, arrivals)\n        let a = receive(arrivals)\n        let b = receive(arrivals)\n        print(unwrap(a))\n        print(unwrap(b))\n    return 0\n\nfn deliver_slow(out: Channel[Int]) -> Int:\n    let n = first(\"http://{0}/slow\")\n    let sent = send(out, 1)\n    return n\n\nfn deliver_fast(out: Channel[Int]) -> Int:\n    let n = first(\"http://{0}/a\")\n    let sent = send(out, 2)\n    return n\n",
            server.address
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
        "race.tape",
        "main.gu",
    ]);
    assert_eq!(stdout(&recorded), "1\n2\n1", "{}", stderr(&recorded));
    for _ in 0..30 {
        let replayed = guion(&["run", "--replay", "race.tape", "main.gu"]);
        assert_eq!(stdout(&replayed), "1\n2\n1", "{}", stderr(&replayed));
    }
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- Part 5: commands in the background ----

#[cfg(unix)]
mod background {
    use super::*;

    fn pid_alive(pid: &str) -> bool {
        Command::new("kill")
            .args(["-0", pid])
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    fn wait_until(limit: Duration, mut done: impl FnMut() -> bool) -> bool {
        let started = Instant::now();
        while started.elapsed() < limit {
            if done() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        done()
    }

    #[test]
    fn a_started_command_is_read_a_line_at_a_time_and_waited_for() {
        let output = run_project(
            "bg-lines",
            "",
            &["--allow-command", "sh"],
            "import \"std:process\"\nimport \"std:types\"\n\nfn main() -> Int:\n    together:\n        match start_process(\"sh\", [\"-c\", \"echo one; echo two; exit 3\"]):\n            Ok(p):\n                var more = true\n                while more:\n                    match process_line(p):\n                        Some(line):\n                            print(line)\n                        None:\n                            set more = false\n                match process_wait(p):\n                    Ok(done):\n                        print(done.exit_code)\n                    Err(problem):\n                        print(problem.code)\n            Err(problem):\n                print(problem.code)\n    return 0\n",
        );
        assert_eq!(stdout(&output), "one\ntwo\n3", "{}", stderr(&output));
    }

    #[test]
    fn input_can_be_written_and_closed() {
        let output = run_project(
            "bg-input",
            "",
            &["--allow-command", "cat"],
            "import \"std:process\"\nimport \"std:types\"\n\nfn main() -> Int:\n    together:\n        match start_process(\"cat\", []):\n            Ok(p):\n                let wrote = process_write(p, \"hello\\nthere\\n\")\n                let closed = process_close_input(p)\n                match process_line(p):\n                    Some(line):\n                        print(line)\n                    None:\n                        print(\"none\")\n                match process_wait(p):\n                    Ok(done):\n                        print(done.stdout)\n                    Err(problem):\n                        print(problem.code)\n            Err(problem):\n                print(problem.code)\n    return 0\n",
        );
        assert_eq!(stdout(&output), "hello\nthere", "{}", stderr(&output));
    }

    #[test]
    fn a_command_without_a_grant_is_refused_with_the_usual_fix() {
        let output = run_project(
            "bg-denied",
            "",
            &[],
            "import \"std:process\"\nimport \"std:types\"\n\nfn main() -> Int:\n    together:\n        match start_process(\"sh\", [\"-c\", \"true\"]):\n            Ok(p):\n                print(\"started\")\n            Err(problem):\n                print(problem.code)\n                print(problem.message)\n    return 0\n",
        );
        let text = stdout(&output);
        assert!(
            text.starts_with("E_PROCESS_DENIED"),
            "{text}{}",
            stderr(&output)
        );
        assert!(text.contains("--allow-command sh"), "{text}");
    }

    #[test]
    fn a_command_not_waited_for_is_gone_with_everything_it_started_when_the_block_ends() {
        let dir = std::env::temp_dir().join(format!("guion-conc-bg-group-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("main.gu"),
            "import \"std:process\"\nimport \"std:types\"\n\nfn main() -> Int:\n    together:\n        match start_process(\"sh\", [\"-c\", \"sleep 300 & echo $! > child.pid; echo ready; wait\"]):\n            Ok(p):\n                print(process_line(p))\n            Err(problem):\n                print(problem.code)\n    print(\"block ended\")\n    return 0\n",
        )
        .unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_guion"))
            .args(["run", "--allow-command", "sh", "main.gu"])
            .current_dir(&dir)
            .env("GUION_NO_PROMPT", "1")
            .output()
            .unwrap();
        assert!(
            stdout(&output).ends_with("block ended"),
            "{}{}",
            stdout(&output),
            stderr(&output)
        );
        let pid = std::fs::read_to_string(dir.join("child.pid"))
            .unwrap()
            .trim()
            .to_owned();
        assert!(!pid.is_empty());
        assert!(
            wait_until(Duration::from_secs(5), || !pid_alive(&pid)),
            "the command's own child {pid} was left running"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_interrupt_leaves_no_child_processes() {
        let dir = std::env::temp_dir().join(format!("guion-conc-bg-int-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("main.gu"),
            "import \"std:process\"\nimport \"std:types\"\n\nfn main() -> Int:\n    together:\n        match start_process(\"sh\", [\"-c\", \"sleep 300 & echo $! > child.pid; wait\"]):\n            Ok(p):\n                match process_wait(p):\n                    Ok(done):\n                        print(done.exit_code)\n                    Err(problem):\n                        print(problem.code)\n            Err(problem):\n                print(problem.code)\n    return 0\n",
        )
        .unwrap();
        let mut guion = Command::new(env!("CARGO_BIN_EXE_guion"))
            .args(["run", "--allow-command", "sh", "main.gu"])
            .current_dir(&dir)
            .env("GUION_NO_PROMPT", "1")
            .spawn()
            .unwrap();
        let pid_file = dir.join("child.pid");
        assert!(wait_until(Duration::from_secs(15), || {
            std::fs::read_to_string(&pid_file)
                .map(|text| !text.trim().is_empty())
                .unwrap_or(false)
        }));
        let child_pid = std::fs::read_to_string(&pid_file)
            .unwrap()
            .trim()
            .to_owned();
        assert!(pid_alive(&child_pid));
        Command::new("kill")
            .args(["-INT", &guion.id().to_string()])
            .status()
            .unwrap();
        let status = guion.wait().unwrap();
        assert_eq!(status.code(), Some(130), "{status:?}");
        assert!(
            wait_until(Duration::from_secs(5), || !pid_alive(&child_pid)),
            "the command's child {child_pid} survived the interrupt"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_process_cannot_leave_its_block() {
        let returned = project_failure(
            "bg-return",
            "import \"std:process\"\nimport \"std:types\"\n\nfn make() -> Process:\n    return Process(0)\n\nfn main() -> Int:\n    return 0\n",
        );
        assert!(returned.contains("E_SCOPE_ESCAPE"), "{returned}");
    }

    #[test]
    fn a_recorded_session_with_a_command_replays_with_nothing_granted() {
        let dir = std::env::temp_dir().join(format!("guion-conc-bg-replay-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("main.gu"),
            "import \"std:process\"\nimport \"std:types\"\n\nfn main() -> Int:\n    together:\n        match start_process(\"sh\", [\"-c\", \"echo alpha; echo beta\"]):\n            Ok(p):\n                var more = true\n                while more:\n                    match process_line(p):\n                        Some(line):\n                            print(line)\n                        None:\n                            set more = false\n                match process_wait(p):\n                    Ok(done):\n                        print(done.exit_code)\n                    Err(problem):\n                        print(problem.code)\n            Err(problem):\n                print(problem.code)\n    return 0\n",
        )
        .unwrap();
        let guion = |args: &[&str]| {
            Command::new(env!("CARGO_BIN_EXE_guion"))
                .args(args)
                .current_dir(&dir)
                .env("GUION_NO_PROMPT", "1")
                .output()
                .unwrap()
        };
        let recorded = guion(&[
            "run",
            "--allow-command",
            "sh",
            "--record",
            "t.tape",
            "main.gu",
        ]);
        assert_eq!(stdout(&recorded), "alpha\nbeta\n0", "{}", stderr(&recorded));
        let replayed = guion(&["run", "--replay", "t.tape", "main.gu"]);
        assert_eq!(stdout(&replayed), "alpha\nbeta\n0", "{}", stderr(&replayed));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn record_arguments_can_be_named_in_any_order() {
    let source = "record Box:\n    width: Int\n    height: Int\n    label: Text\n\nfn line(b: Box) -> Text:\n    return int_to_text(b.width) + \" \" + int_to_text(b.height) + \" \" + b.label\n\nfn main() -> Int:\n    print(line(Box(1, 2, \"a\")))\n    print(line(Box(label=\"b\", height=20, width=10)))\n    print(line(Box(5, label=\"c\", height=6)))\n    return 0\n";
    let output = run_project("named-ok", "", &[], source);
    assert_eq!(
        stdout(&output),
        "1 2 a\n10 20 b\n5 6 c",
        "{}",
        stderr(&output)
    );
}

#[test]
fn record_argument_mistakes_are_named() {
    let base = "record Box:\n    width: Int\n    height: Int\n\nfn main() -> Int:\n    let b = BODY\n    return b.width\n";
    for (body, code, words) in [
        (
            "Box(width=1, depth=2)",
            "E_CALL_KEYWORD",
            "no field `depth`",
        ),
        ("Box(1, width=2)", "E_CALL_KEYWORD", "given twice"),
        ("Box(1, 2, 3, height=4)", "E_CALL_ARITY", "positional"),
        ("Box(width=1)", "E_CALL_ARITY", "missing height"),
    ] {
        let output = run_project("named-bad", "", &[], &base.replace("BODY", body));
        assert!(!output.status.success(), "{body}");
        let text = stderr(&output);
        assert!(
            text.contains(code) && text.contains(words),
            "{body}: {text}"
        );
    }
}

#[test]
fn first_value_takes_the_value_itself_and_a_closed_channel_gives_none() {
    let source = "import \"std:process\"\n\nfn main() -> Int:\n    together:\n        let a: Channel[Int] = channel(2)\n        let b: Channel[Int] = channel(2)\n        let sent = send(b, 41)\n        let first = first_value([a, b])\n        print(first.index)\n        print(unwrap(first.value))\n        close(a)\n        let closed = first_value([a])\n        print(closed.index)\n        print(is_some(closed.value))\n    return 0\n";
    let output = run_project("ready-value", "", &[], source);
    assert_eq!(stdout(&output), "1\n41\n0\nfalse", "{}", stderr(&output));
}

#[test]
fn first_ready_needs_std_process_for_the_ready_record() {
    let text = project_failure(
        "ready-import",
        "fn id(n: Int) -> Int:\n    return n\n\nfn main() -> Int:\n    together:\n        let t = start(id, 1)\n        let won = first_ready([t])\n    return 0\n",
    );
    assert!(
        text.contains("E_IMPORT_REQUIRED") && text.contains("std:process"),
        "{text}"
    );
}

#[test]
fn two_consumers_sharing_a_channel_never_lose_or_double_a_value() {
    let source = "import \"std:process\"\n\nfn drain(c: Channel[Int]) -> Int:\n    var total = 0\n    var open = true\n    while open:\n        let got = first_value([c])\n        match got.value:\n            Some(n):\n                set total = total + n\n            None:\n                set open = false\n    return total\n\nfn main() -> Int:\n    together:\n        let c: Channel[Int] = channel(4)\n        let one = start(drain, c)\n        let two = start(drain, c)\n        var i = 1\n        while i <= 200:\n            let sent = send(c, i)\n            set i = i + 1\n        close(c)\n        let a = wait(one)\n        let b = wait(two)\n        print(unwrap(a) + unwrap(b))\n    return 0\n";
    let output = run_project("ready-two", "", &[], source);
    assert_eq!(stdout(&output), "20100", "{}", stderr(&output));
}

#[cfg(windows)]
mod windows {
    use super::*;

    fn pings_running() -> usize {
        let listing = Command::new("tasklist")
            .args(["/FI", "IMAGENAME eq ping.exe", "/FO", "CSV", "/NH"])
            .output()
            .expect("tasklist runs");
        String::from_utf8_lossy(&listing.stdout)
            .lines()
            .filter(|line| line.to_ascii_lowercase().contains("ping.exe"))
            .count()
    }

    #[test]
    fn a_command_and_what_it_started_are_gone_when_the_block_ends() {
        assert_eq!(pings_running(), 0, "a ping was already running");
        // Reading two lines of ping's output proves ping, a child of cmd, is running.
        let source = "import \"std:process\"\n\nfn main() -> Int:\n    together:\n        let started = start_process(\"cmd\", [\"/c\", \"ping -n 120 127.0.0.1\"])\n        if is_ok(started):\n            let p = unwrap(started)\n            let first = process_line(p)\n            let second = process_line(p)\n            print(is_some(first) and is_some(second))\n    return 0\n";
        let output = run_project(
            "win-job",
            "[environment]\nvariables = [\"SystemRoot\", \"ComSpec\", \"windir\"]\n",
            &["--allow-command", "cmd"],
            source,
        );
        assert_eq!(stdout(&output), "true", "{}", stderr(&output));
        let started = Instant::now();
        while pings_running() > 0 && started.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(200));
        }
        assert_eq!(pings_running(), 0, "the grandchild outlived the block");
    }
}

#[cfg(feature = "python-interop")]
#[test]
fn a_python_call_is_cut_off_at_the_item_deadline() {
    let python = Command::new("python3")
        .args(["-c", "import sys; print(sys.executable)"])
        .output()
        .expect("python3 runs");
    let python = String::from_utf8(python.stdout).unwrap().trim().to_owned();
    let dir = std::env::temp_dir().join(format!("guion-conc-py-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("napper.py"),
        "import time\ndef nap(seconds):\n    time.sleep(seconds)\n    return seconds\n",
    )
    .unwrap();
    let source = format!(
        "import \"std:process\"\n\nextern python \"napper:nap\" fn nap(seconds: Int) -> Int\n\nfn work(seconds: Int) -> Int:\n    return nap(seconds)\n\n{SUMMARY}\nfn main() -> Int:\n    let results = try_parallel_map([0, 30, 0], work, Workers(at_once=3, each_ms=800, all_ms=0))\n    return show(results)\n"
    );
    std::fs::write(dir.join("main.gu"), source).unwrap();
    let started = Instant::now();
    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", "main.gu"])
        .current_dir(&dir)
        .env("GUION_PYTHON", python)
        .env("GUION_NO_PROMPT", "1")
        .output()
        .expect("guion starts");
    let elapsed = started.elapsed();
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(
        stdout(&output),
        "ok 0\nR_DEADLINE\nok 0",
        "{}",
        stderr(&output)
    );
    // The call would sleep 30 s and the bridge's own limit is 15 s.
    assert!(elapsed < Duration::from_secs(8), "took {elapsed:?}");
}
