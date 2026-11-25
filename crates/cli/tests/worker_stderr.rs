//! `eprint` inside a `parallel_map` worker or a `together:` task reaches standard error. Before
//! 0.9.0 the parent merged a worker's standard output and dropped its standard error.

use std::path::PathBuf;
use std::process::{Command, Output};

struct Scratch(PathBuf);
impl Scratch {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("guion-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run(name: &str, source: &str, extra: &[&str]) -> Output {
    let scratch = Scratch::new(name);
    std::fs::write(scratch.0.join("main.gu"), source).unwrap();
    let mut args = vec!["run"];
    args.extend_from_slice(extra);
    args.push("main.gu");
    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(&args)
        .current_dir(&scratch.0)
        .env("GUION_NO_PROMPT", "1")
        .output()
        .expect("guion starts");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

const PARALLEL: &str = r#"fn worker(n: Int) -> Int:
    eprint("err " + int_to_text(n))
    print("out " + int_to_text(n))
    return n

fn main() -> Unit:
    match parallel_map([1, 2, 3], worker, 3):
        Ok(xs):
            eprint("done")
        Err(e):
            eprint("failed")
"#;

#[test]
fn eprint_in_a_parallel_worker_reaches_stderr_in_item_order() {
    let output = run("worker-stderr-parallel", PARALLEL, &[]);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "out 1\nout 2\nout 3\n"
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "err 1\nerr 2\nerr 3\ndone\n"
    );
}

#[test]
fn eprint_in_a_try_parallel_worker_reaches_stderr() {
    let source = format!(
        "import \"std:process\"\n\n{}",
        PARALLEL.replace(
            "    match parallel_map([1, 2, 3], worker, 3):\n        Ok(xs):\n            eprint(\"done\")\n        Err(e):\n            eprint(\"failed\")\n",
            "    let items = try_parallel_map([1, 2, 3], worker, Workers(at_once=3, each_ms=5000, all_ms=30000))\n    eprint(\"done \" + int_to_text(len(items)))\n",
        )
    );
    let output = run("worker-stderr-try", &source, &[]);
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "err 1\nerr 2\nerr 3\ndone 3\n"
    );
}

#[test]
fn eprint_in_a_together_task_reaches_stderr() {
    let source = r#"fn work(n: Int) -> Int:
    var total = 0
    for i in range(0, 1000):
        set total = total + i
    eprint("task " + int_to_text(n))
    return total + n

fn main() -> Unit:
    together:
        let a = work(1)
        let b = work(2)
    eprint("done")
"#;
    let output = run("worker-stderr-together", source, &[]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("task 1\n"), "stderr: {stderr}");
    assert!(stderr.contains("task 2\n"), "stderr: {stderr}");
    assert!(stderr.ends_with("done\n"), "stderr: {stderr}");
}
