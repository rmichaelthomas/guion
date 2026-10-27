use guion_compiler::compile;
use guion_model::{Artifact, SharedJson};
use guion_runtime::stdlib_data::{
    compare_semver, parse_semver, parse_timestamp, CsvError, Date, FileReaderRegistry,
    ReaderErrorKind, TextOptionMap,
};
use guion_runtime::{HostPolicy, Runtime};
use std::io::Write;

fn temp_file(contents: &[u8]) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!(
        "guion-stdlib-data-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut file = std::fs::File::create(&path).unwrap();
    file.write_all(contents).unwrap();
    path
}

#[test]
fn line_reader_is_incremental_strict_utf8_and_tracks_physical_lines() {
    let large = format!("{}\r\nlast\n", "x".repeat(2 * 1024 * 1024));
    let path = temp_file(large.as_bytes());
    let mut readers = FileReaderRegistry::new(4 * 1024 * 1024, 4 * 1024 * 1024);
    let handle = readers.open_lines(&path).unwrap();
    let first = readers.next_line(handle).unwrap().unwrap();
    assert_eq!(first.number, 1);
    assert_eq!(first.text.len(), 2 * 1024 * 1024);
    assert_eq!(readers.next_line(handle).unwrap().unwrap().text, "last");
    assert_eq!(readers.next_line(handle).unwrap(), None);
    readers.close(handle).unwrap();
    assert_eq!(
        readers.next_line(handle).unwrap_err().kind,
        ReaderErrorKind::Closed
    );
    let _ = std::fs::remove_file(path);

    let invalid = temp_file(&[b'a', b'\n', 0xff]);
    let handle = readers.open_lines(&invalid).unwrap();
    assert_eq!(readers.next_line(handle).unwrap().unwrap().number, 1);
    assert_eq!(
        readers.next_line(handle).unwrap_err().kind,
        ReaderErrorKind::InvalidUtf8
    );
    let _ = std::fs::remove_file(invalid);
}

#[test]
fn csv_reader_handles_multiline_quotes_empty_fields_and_malformed_rows() {
    let path = temp_file(b"a,,\r\n\"b,b\",\"say \"\"hi\"\"\",\"two\r\nlines\"\r\n");
    let mut readers = FileReaderRegistry::new(1024, 64);
    let handle = readers.open_csv(&path).unwrap();
    let row1 = readers.next_csv(handle).unwrap().unwrap();
    assert_eq!(row1.start_line, 1);
    assert_eq!(row1.fields, ["a", "", ""]);
    let row2 = readers.next_csv(handle).unwrap().unwrap();
    assert_eq!(row2.start_line, 2);
    assert_eq!(row2.fields, ["b,b", "say \"hi\"", "two\r\nlines"]);
    assert_eq!(readers.next_csv(handle).unwrap(), None);
    readers.close(handle).unwrap();
    let _ = std::fs::remove_file(path);

    for (input, expected_line, expected_column) in [
        (b"ab\"cd\n".as_slice(), 1, 3),
        (b"\"open\n".as_slice(), 1, 6),
        (b"\"closed\"x\n".as_slice(), 1, 9),
    ] {
        let path = temp_file(input);
        let handle = readers.open_csv(&path).unwrap();
        let malformed = readers.next_csv(handle);
        assert!(
            matches!(
                malformed,
                Err(CsvError::Malformed { line, column, .. }) if line == expected_line && column == expected_column
            ),
            "unexpected CSV diagnostic: {malformed:?}"
        );
        let _ = std::fs::remove_file(path);
    }
}

#[test]
fn reader_handles_are_owned_typed_and_closed_with_registry() {
    let path = temp_file(b"a\n");
    let mut lines = FileReaderRegistry::new(1024, 32);
    let mut other = FileReaderRegistry::new(1024, 32);
    let handle = lines.open_lines(&path).unwrap();
    assert_eq!(
        other.next_line(handle).unwrap_err().kind,
        ReaderErrorKind::ForeignOwner
    );
    assert_eq!(lines.next_csv(handle).unwrap_err(), CsvError::Mode);
    lines.close(handle).unwrap();
    assert_eq!(
        lines.close(handle).unwrap_err().kind,
        ReaderErrorKind::Closed
    );
    let handle = lines.open_lines(&path).unwrap();
    lines.close_all();
    assert_eq!(
        lines.next_line(handle).unwrap_err().kind,
        ReaderErrorKind::Closed
    );
    let _ = std::fs::remove_file(path);
}

#[test]
fn strict_timestamps_validate_gregorian_dates_and_normalize_offsets_exactly() {
    assert_eq!(
        Date::parse("2024-02-29").unwrap(),
        Date {
            year: 2024,
            month: 2,
            day: 29
        }
    );
    assert!(Date::parse("2023-02-29").is_err());
    assert_eq!(
        parse_timestamp("2024-01-01T01:30:00.123400+01:30").unwrap(),
        1_704_067_200_123_400
    );
    assert_eq!(parse_timestamp("1970-01-01T00:00:00Z").unwrap(), 0);
    for bad in [
        "2024-01-01t00:00:00Z",
        "2024-01-01T00:00:60Z",
        "2024-01-01T00:00:00",
        "2024-01-01T00:00:00.1234567Z",
        "2023-02-29T00:00:00Z",
    ] {
        assert!(parse_timestamp(bad).is_err(), "accepted {bad}");
    }
}

#[test]
fn semver_supports_huge_components_and_full_prerelease_precedence() {
    let versions = [
        "1.0.0-alpha",
        "1.0.0-alpha.1",
        "1.0.0-alpha.beta",
        "1.0.0-beta",
        "1.0.0-beta.2",
        "1.0.0-beta.11",
        "1.0.0-rc.1",
        "1.0.0",
    ];
    for pair in versions.windows(2) {
        assert_eq!(
            compare_semver(
                &parse_semver(pair[0]).unwrap(),
                &parse_semver(pair[1]).unwrap()
            ),
            std::cmp::Ordering::Less
        );
    }
    assert_eq!(
        compare_semver(
            &parse_semver("999999999999999999999999999999.0.0+one").unwrap(),
            &parse_semver("1000000000000000000000000000000.0.0+two").unwrap()
        ),
        std::cmp::Ordering::Less
    );
    assert_eq!(parse_semver("1.2.3+build.1").unwrap().build, ["build", "1"]);
    for bad in ["01.2.3", "1.2", "1.2.3-01", "1.2.3+bad..id", "1.2.3-"] {
        assert!(parse_semver(bad).is_err(), "accepted {bad}");
    }
}

#[test]
fn cli_option_map_distinguishes_missing_from_supplied_empty() {
    let parsed = TextOptionMap::parse(
        &[
            "--tag=".into(),
            "--tag=x".into(),
            "pos".into(),
            "--".into(),
            "--literal".into(),
        ],
        &[
            guion_runtime::stdlib_data::CliOption::new("tag", true, true, true),
            guion_runtime::stdlib_data::CliOption::new("flag", false, false, false),
        ],
    )
    .unwrap();
    assert_eq!(parsed.values["tag"], ["", "x"]);
    assert_eq!(parsed.positionals, ["pos", "--literal"]);
}

#[test]
fn json_safe_accessors_keep_missing_null_and_boolean_types_distinct() {
    let value =
        SharedJson::from_serde(serde_json::json!({"present": null, "truth": true, "count": 5}));
    assert!(guion_runtime::stdlib_data::json_try_get(&value, "present")
        .unwrap()
        .is_null());
    assert_eq!(
        guion_runtime::stdlib_data::json_try_get(&value, "missing")
            .unwrap_err()
            .code,
        "E_JSON_MISSING"
    );
    assert!(guion_runtime::stdlib_data::json_try_bool(&SharedJson::Bool(true)).unwrap());
    assert_eq!(
        guion_runtime::stdlib_data::json_try_int(&SharedJson::Bool(true))
            .unwrap_err()
            .code,
        "E_JSON_TYPE"
    );
    assert_eq!(
        guion_runtime::stdlib_data::json_try_int(&SharedJson::Number(serde_json::Number::from(5)))
            .unwrap()
            .to_string(),
        "5"
    );
    let scalar = SharedJson::from_serde(serde_json::Value::Null);
    assert_eq!(
        guion_runtime::stdlib_data::json_try_get(&scalar, "x")
            .unwrap_err()
            .code,
        "E_JSON_SHAPE"
    );
}

#[test]
fn fs_walk_is_sorted_bounded_and_does_not_follow_symlink_directories() {
    let root = std::env::temp_dir().join(format!("guion-walk-{}", std::process::id()));
    let outside = root.with_extension("outside");
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&outside);
    std::fs::create_dir_all(root.join("z-dir")).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(root.join("z-dir/a.txt"), "a").unwrap();
    std::fs::write(outside.join("secret.txt"), "secret").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, root.join("a-link")).unwrap();
    let entries = guion_runtime::stdlib_data::fs_walk(&root, 8).unwrap();
    assert_eq!(
        entries
            .iter()
            .map(|entry| (entry.relative_path.as_str(), entry.kind.as_str()))
            .collect::<Vec<_>>(),
        {
            #[cfg(unix)]
            {
                vec![
                    ("a-link", "symlink"),
                    ("z-dir", "directory"),
                    ("z-dir/a.txt", "file"),
                ]
            }
            #[cfg(not(unix))]
            {
                vec![("z-dir", "directory"), ("z-dir/a.txt", "file")]
            }
        }
    );
    assert!(guion_runtime::stdlib_data::fs_walk(&root, 1).is_err());
    #[cfg(unix)]
    {
        let root_link = root.with_extension("link");
        std::os::unix::fs::symlink(&root, &root_link).unwrap();
        assert_eq!(
            guion_runtime::stdlib_data::fs_walk(&root_link, 8)
                .unwrap_err()
                .code,
            "E_FS_WALK_ROOT"
        );
        let _ = std::fs::remove_file(root_link);
    }
    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_dir_all(outside);
}

#[test]
fn guion_line_and_csv_builtins_stream_records_through_owned_handles() {
    let root = std::env::temp_dir().join(format!("guion-stdlib-vm-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("lines.txt"), "alpha\r\nbeta\n").unwrap();
    std::fs::write(root.join("rows.csv"), "a,,\n\"b,b\",\"two\nlines\"\n").unwrap();
    let outside = root.with_extension("outside");
    std::fs::create_dir_all(&outside).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, root.join("root-link")).unwrap();
    let source = r#"record FileReader:
    owner: Int
    id: Int
record SourceLine:
    number: Int
    text: Text
record CsvRow:
    start_line: Int
    fields: List[Text]
record Date:
    year: Int
    month: Int
    day: Int
record Instant:
    unix_microseconds: Int
record SemVer:
    major: Text
    minor: Text
    patch: Text
    prerelease: List[Text]
    build: List[Text]
    original: Text
record CliOption:
    name: Text
    takes_value: Bool
    repeatable: Bool
    required: Bool
record CliArgs:
    values: Map[Text, List[Text]]
    positionals: List[Text]
record DirectoryEntry:
    relative_path: Text
    kind: Text
fn main() -> Int:
    let file = unwrap(file_open_lines("lines.txt"))
    let first = unwrap(file_next_line(file))
    assert(is_some(first))
    print(unwrap(first).number)
    print(unwrap(first).text)
    unwrap(file_close(file))
    let csv = unwrap(csv_open("rows.csv"))
    let row1 = unwrap(csv_next(csv))
    print(unwrap(row1).start_line)
    print(len(unwrap(row1).fields))
    let row2 = unwrap(csv_next(csv))
    print(unwrap(row2).fields[1])
    unwrap(file_close(csv))
    print(unwrap(csv_encode([["a,b", "say \"hi\""], ["", "line\nbreak"]])))
    let json = unwrap(json_parse("{\"present\":null,\"truth\":true,\"count\":7}"))
    assert(not(is_ok(json_try_get(json, "absent"))))
    assert(json_is_null(unwrap(json_try_get(json, "present"))))
    print(unwrap(json_try_int(unwrap(json_try_get(json, "count")))))
    assert(not(is_ok(json_try_int(unwrap(json_try_get(json, "truth"))))))
    print(unwrap(time_parse_date("2024-02-29")).month)
    let instant = unwrap(time_parse_timestamp("2024-01-01T01:30:00.123400+01:30"))
    print(time_format_utc(instant))
    let prerelease = unwrap(semver_parse("1.0.0-beta.2+build.1"))
    let release = unwrap(semver_parse("1.0.0"))
    print(semver_compare(prerelease, release))
    let arguments = unwrap(cli_parse(["--tag=", "pos", "--", "--literal"], [CliOption("tag", true, true, true)]))
    print(arguments.values["tag"][0])
    print(arguments.positionals[1])
    assert(len(unwrap(fs_walk("."))) > 0)
    assert(not(is_ok(fs_walk("root-link"))))
    return 0
"#;
    let artifact = compile("stdlib-data.gu", source).expect("data builtins compile");
    assert_eq!(artifact.format_version, Artifact::CURRENT_VERSION);
    let mut old_artifact = artifact.clone();
    old_artifact.format_version = 34;
    assert_eq!(
        old_artifact.to_bytes().unwrap_err().code,
        "E_ARTIFACT_VERSION"
    );
    for vm_only in [true, false] {
        let mut runtime = if vm_only {
            Runtime::vm_only()
        } else {
            Runtime::new()
        }
        .load(
            artifact.clone(),
            HostPolicy::default().with_file_access(&root, true, false),
        )
        .unwrap();
        runtime.invoke("main", vec![]).unwrap();
        assert_eq!(runtime.take_output(), "1\nalpha\n1\n3\ntwo\nlines\n\"a,b\",\"say \"\"hi\"\"\"\n,\"line\nbreak\"\n7\n2\n2024-01-01T00:00:00.1234Z\n-1\n\n--literal\n");
        assert_eq!(runtime.open_file_reader_count(), 0);
    }
    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_dir_all(outside);
}

#[test]
fn every_shipped_data_module_compiles_as_an_independent_std_import() {
    let root = std::env::temp_dir().join(format!("guion-stdlib-modules-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let source = r#"import "std:cli"
import "std:collections"
import "std:csv"
import "std:fs"
import "std:json"
import "std:jsonl"
import "std:semver"
import "std:text"
import "std:time"
fn main() -> Int:
    assert(text_trim("　 hello \n") == "hello")
    assert(text_join(text_split("a::b::", "::"), "|") == "a|b|")
    assert(text_join(text_split("a,b,,c,", ","), "|") == "a|b||c|")
    assert(text_join(text_split("α🧬β🧬", "🧬"), "|") == "α|β|")
    assert(len(text_split("", ",")) == 1)
    assert(text_join(text_split("abc", ""), "|") == "abc")
    assert(text_replace("aba", "a", "x") == "xbx")
    assert(text_replace("αββ", "β", "X") == "αXX")
    assert(unwrap(text_parse_integer("-123")) == -123)
    assert(unwrap(list_get([1, 2], 1)) == 2)
    assert(not(is_some(list_get([1, 2], 8))))
    assert(unwrap(map_get({"x": 3}, "x")) == 3)
    assert(text_compare_scalar("é", "😀") < 0)
    assert(text_compare_bytes("é", "😀") < 0)
    let instant = unwrap(parse_timestamp("2024-01-01T00:00:00Z"))
    assert(format_utc(instant) == "2024-01-01T00:00:00Z")
    assert(compare_versions(unwrap(parse_version("1.0.0-alpha")), unwrap(parse_version("1.0.0"))) < 0)
    return 0
"#;
    let entry = root.join("main.gu");
    std::fs::write(&entry, source).unwrap();
    let stdlib = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../language/stdlib");
    let artifact = guion_compiler::compile_project_with_roots(
        &entry,
        &root,
        Some(&stdlib),
        &std::collections::HashMap::new(),
    )
    .expect("shipped stdlib imports compile");
    for vm_only in [true, false] {
        let mut runtime = if vm_only {
            Runtime::vm_only()
        } else {
            Runtime::new()
        }
        .load(artifact.clone(), HostPolicy::default())
        .unwrap();
        runtime.invoke("main", vec![]).unwrap();
        assert_eq!(runtime.take_output(), "");
    }
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn fs_read_text_returns_content_and_preserves_missing_path_in_std_error() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!(
        "../../target/perf-session/fs-read-text-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("present.txt"), "hello from std:fs").unwrap();
    let source = r#"
import "std:fs"
import "std:types"
fn main() -> Int:
    match fs_read_text("present.txt"):
        Ok(text):
            print(text)
        Err(_problem):
            return 1
    match fs_read_text("missing.txt"):
        Ok(_text):
            return 2
        Err(problem):
            print(problem.code)
            print(problem.path)
            return 0
"#;
    let entry = root.join("main.gu");
    std::fs::write(&entry, source).unwrap();
    let stdlib = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../language/stdlib");
    let artifact = guion_compiler::compile_project_with_roots(
        &entry,
        &root,
        Some(&stdlib),
        &std::collections::HashMap::new(),
    )
    .expect("filesystem helper compiles with std imports");

    for runtime in [Runtime::vm_only(), Runtime::new()] {
        let mut instance = runtime
            .load(
                artifact.clone(),
                HostPolicy::default().with_file_access(&root, true, false),
            )
            .expect("runtime loads filesystem helper");
        instance
            .invoke("main", vec![])
            .expect("helper handles file result");
        assert_eq!(
            instance.take_output(),
            "hello from std:fs\nR_FILE_NOT_FOUND\nmissing.txt\n"
        );
    }
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn invalid_data_keeps_existing_output_untouched() {
    let root = std::env::temp_dir().join(format!("guion-invalid-data-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("report.txt"), "existing report\n").unwrap();
    let source = r#"fn main() -> Int:
    let parsed = time_parse_date("2023-02-29")
    if is_ok(parsed):
        unwrap(write_text_atomic("report.txt", "invalid output\n"))
    return 0
"#;
    let artifact = compile("invalid-data.gu", source).unwrap();
    let mut runtime = Runtime::vm_only()
        .load(
            artifact,
            HostPolicy::default().with_file_access(&root, true, true),
        )
        .unwrap();
    runtime.invoke("main", vec![]).unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("report.txt")).unwrap(),
        "existing report\n"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn sort_by_is_stable_preserves_aliases_and_rejects_transitive_effects() {
    let source = r#"record Item:
    key: Int
    order: Int
fn compare_key(left: Item, right: Item) -> Int:
    return left.key - right.key
fn main() -> Int:
    let items = [Item(1, 1), Item(0, 2), Item(1, 3)]
    let sorted = sort_by(items, compare_key)
    assert(sorted[0].order == 2)
    assert(sorted[1].order == 1)
    assert(sorted[2].order == 3)
    assert(items[0].order == 1)
    return 0
"#;
    let artifact = compile("sort-by.gu", source).expect("named comparator compiles");
    assert_eq!(artifact.format_version, Artifact::CURRENT_VERSION);
    let mut too_old = artifact.clone();
    too_old.format_version = 35;
    assert_eq!(too_old.to_bytes().unwrap_err().code, "E_ARTIFACT_VERSION");
    for vm_only in [true, false] {
        let mut runtime = if vm_only {
            Runtime::vm_only()
        } else {
            Runtime::new()
        }
        .load(artifact.clone(), HostPolicy::default())
        .unwrap();
        runtime.invoke("main", vec![]).unwrap();
    }

    let effectful = r#"fn helper(value: Int) -> Int:
    print(value)
    return value
fn compare(left: Int, right: Int) -> Int:
    return helper(left) - helper(right)
fn main() -> Int:
    let values = sort_by([2, 1], compare)
    return values[0]
"#;
    let error = compile("sort-effect.gu", effectful).unwrap_err();
    assert_eq!(error.code, "E_SORT_EFFECT");
}
