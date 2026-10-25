use std::process::Command;

#[test]
fn embedded_self_host_compiler_runs_shipped_data_modules_and_streams() {
    let root = std::env::temp_dir().join(format!("guion stdlib data 東京 {}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("guion.toml"),
        "[project]\nname = \"stdlib-data-check\"\nversion = \"0.1.0\"\nentry = \"src/main.gu\"\n",
    )
    .unwrap();
    std::fs::write(root.join("lines.txt"), "first\r\nsecond\n").unwrap();
    std::fs::write(root.join("rows.csv"), "a,,\n\"b,b\",\"two\nlines\"\n").unwrap();
    std::fs::write(root.join("events.jsonl"), "{\"n\":7}\nnot-json\n").unwrap();
    let source = r#"import "std:cli"
import "std:collections"
import "std:csv"
import "std:fs"
import "std:json"
import "std:jsonl"
import "std:semver"
import "std:text"
import "std:time"
fn compare_int(left: Int, right: Int) -> Int:
    return left - right
fn main() -> Int:
    let line_reader = unwrap(fs_open_lines("lines.txt"))
    let line = unwrap(fs_next_line(line_reader))
    print(unwrap(line).number)
    print(unwrap(line).text)
    unwrap(fs_close_reader(line_reader))
    let csv_reader = unwrap(csv_open_reader("rows.csv"))
    let row = unwrap(csv_next_row(csv_reader))
    print(unwrap(row).start_line)
    print(len(unwrap(row).fields))
    unwrap(fs_close_reader(csv_reader))
    let jsonl_reader = unwrap(jsonl_open("events.jsonl"))
    let event = unwrap(jsonl_next(jsonl_reader))
    print(unwrap(json_try_int(unwrap(json_try_get(unwrap(unwrap(event).value), "n")))) )
    let bad_event = unwrap(jsonl_next(jsonl_reader))
    assert(is_some(unwrap(bad_event).error))
    unwrap(jsonl_close(jsonl_reader))
    assert(text_trim("　 hello \n") == "hello")
    assert(text_join(text_split("a::b::", "::"), "|") == "a|b|")
    assert(text_replace("aba", "a", "x") == "xbx")
    assert(unwrap(text_parse_integer("-123")) == -123)
    assert(unwrap(list_get([1, 2], 1)) == 2)
    assert(not(is_some(list_get([1, 2], 8))))
    assert(sort_by([3, 1, 2], compare_int)[0] == 1)
    assert(unwrap(map_get({"x": 3}, "x")) == 3)
    let instant = unwrap(parse_timestamp("2024-01-01T01:30:00.123400+01:30"))
    print(format_utc(instant))
    assert(compare_versions(unwrap(parse_version("1.0.0-alpha")), unwrap(parse_version("1.0.0"))) < 0)
    let flags = unwrap(parse_cli(["--empty=", "pos", "--", "--literal"], [CliOption("empty", true, false, true)]))
    assert(unwrap(map_get(flags.values, "empty"))[0] == "")
    assert(flags.positionals[1] == "--literal")
    return 0
"#;
    std::fs::write(root.join("src/main.gu"), source).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .arg("run")
        .arg(&root)
        .current_dir(&root)
        .output()
        .expect("embedded Guion CLI starts");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "1\nfirst\n1\n3\n7\n2024-01-01T00:00:00.1234Z\n"
    );

    std::fs::write(
        root.join("src/main.gu"),
        r#"fn effect(value: Int) -> Int:
    print(value)
    return value
fn compare(left: Int, right: Int) -> Int:
    return effect(left) - effect(right)
fn main() -> Int:
    let values = sort_by([2, 1], compare)
    return 0
"#,
    )
    .unwrap();
    let rejected = Command::new(env!("CARGO_BIN_EXE_guion"))
        .arg("check")
        .arg(&root)
        .current_dir(&root)
        .output()
        .expect("embedded Guion CLI checks effectful comparator");
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("E_SORT_EFFECT"));
    let _ = std::fs::remove_dir_all(root);
}
