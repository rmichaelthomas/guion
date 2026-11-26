//! A program that reads a large CSV file over and over (the parallel reader hands the work to the
//! shared worker pool, whose threads allocate and free on their own lists) must not creep toward
//! the allocation limit (the test sets 32 MiB): what a worker frees goes back to a pool the next
//! read can use, and nothing a read allocated stays counted after its columns are dropped.

use std::process::Command;

const PROGRAM: &str = r#"record CsvColumn:
    name: Text
    kind: Text
    ints: Array[i64]
    floats: Array[f64]
    flags: Array[bool]
    codes: Array[i64]
    dictionary: List[Text]
    valid: Array[bool]

fn main() -> Int:
    let none: Map[Text, Text] = {}
    var round = 0
    var total = 0
    while round < 300:
        let columns = unwrap(csv_read_columns("data.csv", none))
        set total = total + len(columns)
        set round = round + 1
    print(total)
    return 0
"#;

#[test]
fn three_hundred_large_csv_reads_complete_under_a_32_mib_allocation_limit() {
    let dir = std::env::temp_dir().join(format!("guion-alloc-threads-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("guion.toml"),
        "[package]\nname = \"alloc-threads\"\nversion = \"0.1.0\"\n\n[runtime]\nallocation_limit_bytes = 33554432\n\n[capabilities]\nfiles = [\"read\"]\n",
    )
    .unwrap();
    let mut csv = String::from("a,b,c\n");
    let mut row = 0u64;
    while csv.len() < 2 * 1024 * 1024 + 4096 {
        csv.push_str(&format!("{row},{}.5,name{}\n", row * 3, row % 97));
        row += 1;
    }
    std::fs::write(dir.join("data.csv"), csv).unwrap();
    std::fs::write(dir.join("main.gu"), PROGRAM).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", "main.gu"])
        .current_dir(&dir)
        .output()
        .expect("guion starts");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stdout}{stderr}");
    assert_eq!(stdout.trim(), "900", "{stderr}");
    let _ = std::fs::remove_dir_all(&dir);
}
