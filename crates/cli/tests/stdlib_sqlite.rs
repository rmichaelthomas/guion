//! `std:sqlite` under the worker-pool pattern: `sqlite_begin_read`, and write transactions that
//! serialize without failing.

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
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run(scratch: &Scratch, source: &str) -> Output {
    std::fs::write(scratch.0.join("main.gu"), source).unwrap();
    Command::new(env!("CARGO_BIN_EXE_guion"))
        .args([
            "run",
            "--file-root",
            scratch.0.join("root").to_str().unwrap(),
            "main.gu",
        ])
        .current_dir(&scratch.0)
        .output()
        .expect("guion starts")
}

fn assert_success(output: &Output) -> String {
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn sixteen_hundred_write_transactions_across_workers_all_commit() {
    let scratch = Scratch::new("sqlite-1600");
    let output = run(
        &scratch,
        r#"import "std:sqlite"
import "std:types"

record Job:
    id: Int
    count: Int

fn writer(job: Job) -> Int:
    let opened = sqlite_open("many.db")
    if not is_ok(opened):
        return -1
    let db = unwrap(opened)
    var committed = 0
    var step = 0
    while step < job.count:
        let begun = sqlite_begin(db)
        if is_ok(begun):
            let tx = unwrap(begun)
            let inserted = sqlite_execute(tx, "INSERT INTO w (worker, seq) VALUES (?, ?)", [sql_integer(job.id), sql_integer(step)])
            if is_ok(inserted):
                if is_ok(sqlite_commit(tx)):
                    set committed = committed + 1
            else:
                let rolled = sqlite_rollback(tx)
        set step = step + 1
    let closed = sqlite_close(db)
    return committed

fn main() -> Int:
    let opened = unwrap(sqlite_open("many.db"))
    let tx = unwrap(sqlite_begin(opened))
    unwrap(sqlite_execute(tx, "CREATE TABLE w (id INTEGER PRIMARY KEY, worker INTEGER, seq INTEGER)", []))
    unwrap(sqlite_commit(tx))
    unwrap(sqlite_close(opened))
    let jobs: List[Job] = [Job(0, 400), Job(1, 400), Job(2, 400), Job(3, 400)]
    let mapped = unwrap(parallel_map(jobs, writer, 4))
    var total = 0
    for done in mapped:
        set total = total + done
    print("committed " + int_to_text(total))
    let check = unwrap(sqlite_open("many.db"))
    let reading = unwrap(sqlite_begin_read(check))
    let rows = unwrap(sqlite_query(reading, "SELECT COUNT(*) AS n FROM w", []))
    print("rows " + int_to_text(len(rows)))
    unwrap(sqlite_commit(reading))
    unwrap(sqlite_close(check))
    return 0
"#,
    );
    let stdout = assert_success(&output);
    assert!(stdout.contains("committed 1600"), "{stdout}");
}

#[test]
fn write_in_a_read_transaction_is_read_only_and_a_nested_begin_read_is_refused() {
    let scratch = Scratch::new("sqlite-read-only");
    let output = run(
        &scratch,
        r#"import "std:sqlite"
import "std:types"

fn main() -> Int:
    let db = unwrap(sqlite_open("ro.db"))
    let setup = unwrap(sqlite_begin(db))
    unwrap(sqlite_execute(setup, "CREATE TABLE t (x INTEGER)", []))
    unwrap(sqlite_execute(setup, "INSERT INTO t (x) VALUES (1)", []))
    let nested = sqlite_begin_read(db)
    assert(not is_ok(nested))
    print("nested " + error(nested).code)
    unwrap(sqlite_commit(setup))
    let reading = unwrap(sqlite_begin_read(db))
    let refused = sqlite_execute(reading, "INSERT INTO t (x) VALUES (2)", [])
    assert(not is_ok(refused))
    print("write " + error(refused).code)
    let rows = unwrap(sqlite_query(reading, "SELECT COUNT(*) AS n FROM t", []))
    print("rows " + int_to_text(len(rows)))
    unwrap(sqlite_commit(reading))
    unwrap(sqlite_close(db))
    return 0
"#,
    );
    let stdout = assert_success(&output);
    assert!(
        stdout.contains("nested E_SQLITE_NESTED_TRANSACTION"),
        "{stdout}"
    );
    assert!(stdout.contains("write E_SQLITE_READ_ONLY"), "{stdout}");
    assert!(stdout.contains("rows 1"), "{stdout}");
}

#[test]
fn read_transaction_left_open_at_worker_exit_does_not_lock_the_database() {
    let scratch = Scratch::new("sqlite-left-open");
    let output = run(
        &scratch,
        r#"import "std:sqlite"
import "std:types"

fn leaves_it_open(id: Int) -> Int:
    let db = unwrap(sqlite_open("left.db"))
    let reading = unwrap(sqlite_begin_read(db))
    let rows = unwrap(sqlite_query(reading, "SELECT COUNT(*) AS n FROM t", []))
    return len(rows)

fn main() -> Int:
    let db = unwrap(sqlite_open("left.db"))
    let setup = unwrap(sqlite_begin(db))
    unwrap(sqlite_execute(setup, "CREATE TABLE t (x INTEGER)", []))
    unwrap(sqlite_commit(setup))
    let mapped = unwrap(parallel_map([1, 2, 3, 4], leaves_it_open, 4))
    print("workers " + int_to_text(len(mapped)))
    # A leaked shared lock would make this commit wait out the busy deadline and fail.
    let writing = unwrap(sqlite_begin(db))
    unwrap(sqlite_execute(writing, "INSERT INTO t (x) VALUES (1)", []))
    let committed = sqlite_commit(writing)
    assert(is_ok(committed))
    print("committed")
    unwrap(sqlite_close(db))
    return 0
"#,
    );
    let stdout = assert_success(&output);
    assert!(stdout.contains("workers 4"), "{stdout}");
    assert!(stdout.contains("committed"), "{stdout}");
}
