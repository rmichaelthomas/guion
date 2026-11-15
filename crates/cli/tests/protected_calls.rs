//! `try_map_list`: each call is its own fault boundary. A fault in one item is that item's
//! `Err(TaskError)` and the rest still run (known issue 30: one bad request must not stop a
//! server's workers).

use std::process::Command;

static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn run(source: &str) -> (bool, String, String) {
    let directory = std::env::temp_dir().join(format!(
        "guion-protected-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("main.gu"), source).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .arg("run")
        .arg("main.gu")
        .current_dir(&directory)
        .output()
        .unwrap();
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

const PROGRAM: &str = r##"fn pick(index: Int) -> Int:
    let items = [10, 20, 30]
    return items[index]

fn main() -> Int:
    let outcomes = try_map_list([0, 7, 2, 9], pick)
    for outcome in outcomes:
        match outcome:
            Ok(value):
                print(f"ok {value}")
            Err(error):
                print(f"fault {task_code(error)}")
    print(f"{len(outcomes)} answers")
    return 0
"##;

#[test]
fn a_fault_in_one_item_does_not_stop_the_others() {
    let (ok, out, err) = run(PROGRAM);
    assert!(ok, "{err}");
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], "ok 10");
    assert!(lines[1].starts_with("fault "), "{out}");
    assert_eq!(lines[2], "ok 30");
    assert!(lines[3].starts_with("fault "), "{out}");
    assert_eq!(lines[4], "4 answers");
}

#[test]
fn it_works_with_a_closure_and_the_same_program_without_it_faults() {
    let (ok, out, err) = run(r##"fn main() -> Int:
    let limit = 2
    let outcomes = try_map_list([1, 2, 3], fn(x: Int) -> Int: [100, 200][x - limit])
    print(f"{len(outcomes)}")
    match outcomes[1]:
        Ok(value):
            print(f"{value}")
        Err(error):
            print("fault")
    return 0
"##);
    assert!(ok, "{err}");
    assert_eq!(out, "3\n100\n");
    let (ok, _, err) = run(
        "fn pick(i: Int) -> Int:\n    return [1][i]\n\nfn main() -> Int:\n    print(f\"{map_list([5], pick)[0]}\")\n    return 0\n",
    );
    assert!(!ok, "map_list must still fault, {err}");
}
