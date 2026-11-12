use guion_compiler::compile;
use guion_model::{Artifact, Op, SharedJson};
use guion_runtime::stdlib_data::{
    compare_semver, parse_semver, parse_timestamp, CsvError, Date, FileReaderRegistry,
    ReaderErrorKind, TextOptionMap,
};
use guion_runtime::stdlib_services::{resolve_local_time, LocalDateTime as ServiceLocalDateTime};
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
fn native_timezone_candidate_enumeration_covers_fractional_and_non_hour_transitions() {
    let resolve = |year, month, day, hour, minute, second, microsecond, timezone: &str| {
        resolve_local_time(
            ServiceLocalDateTime {
                year,
                month,
                day,
                hour,
                minute,
                second,
                microsecond,
            },
            timezone,
        )
    };
    let exact = resolve(2026, 3, 8, 1, 30, 0, 123456, "America/Los_Angeles").unwrap();
    assert_eq!(exact.kind, "exact");
    assert_eq!(exact.local.microsecond, 123456);
    assert_eq!(exact.unix_microseconds, [1_772_962_200_123_456]);
    assert_eq!(exact.timezone, "America/Los_Angeles");

    let gap = resolve(2026, 3, 8, 2, 30, 0, 0, "America/Los_Angeles").unwrap();
    assert_eq!(gap.kind, "gap");
    assert!(gap.unix_microseconds.is_empty());

    let fold = resolve(2026, 11, 1, 1, 30, 0, 0, "America/Los_Angeles").unwrap();
    assert_eq!(fold.kind, "fold");
    assert_eq!(
        fold.unix_microseconds,
        [1_793_521_800_000_000, 1_793_525_400_000_000]
    );

    let lord_howe_fold = resolve(2026, 4, 5, 1, 45, 0, 0, "Australia/Lord_Howe").unwrap();
    assert_eq!(lord_howe_fold.kind, "fold");
    assert_eq!(
        lord_howe_fold.unix_microseconds,
        [1_775_313_900_000_000, 1_775_315_700_000_000]
    );
    let lord_howe_gap = resolve(2026, 10, 4, 2, 15, 0, 0, "Australia/Lord_Howe").unwrap();
    assert_eq!(lord_howe_gap.kind, "gap");
    assert!(lord_howe_gap.unix_microseconds.is_empty());

    let skipped_day = resolve(2011, 12, 30, 12, 0, 0, 0, "Pacific/Apia").unwrap();
    assert_eq!(skipped_day.kind, "gap");
    assert!(skipped_day.unix_microseconds.is_empty());

    assert_eq!(
        resolve(2026, 2, 29, 1, 30, 0, 0, "America/Los_Angeles")
            .unwrap_err()
            .code,
        "E_DATE"
    );
    assert_eq!(
        resolve(2026, 3, 8, 24, 0, 0, 0, "America/Los_Angeles")
            .unwrap_err()
            .code,
        "E_TIME"
    );
    assert_eq!(
        resolve(2026, 3, 8, 1, 30, 0, 0, "Not/A_Real_Zone")
            .unwrap_err()
            .code,
        "E_TIMEZONE_UNKNOWN"
    );
}

#[test]
fn format_utc_date_uses_the_utc_calendar_day_including_pre_epoch_instants() {
    let source = r#"record Instant:
    unix_microseconds: Int
fn main() -> Int:
    let crosses_back = unwrap(time_parse_timestamp("2024-01-01T00:30:00+01:00"))
    assert(time_format_utc_date(crosses_back) == "2023-12-31")
    let crosses_forward = unwrap(time_parse_timestamp("2023-12-31T23:30:00-01:00"))
    assert(time_format_utc_date(crosses_forward) == "2024-01-01")
    let before_epoch = unwrap(time_parse_timestamp("1969-12-31T23:59:59.999999Z"))
    assert(time_format_utc_date(before_epoch) == "1969-12-31")
    return 0
"#;
    let artifact = compile("format-utc-date.gu", source).expect("UTC date formatter compiles");
    assert_eq!(artifact.format_version, Artifact::CURRENT_VERSION);
    for runtime in [Runtime::vm_only(), Runtime::new()] {
        let mut runtime = runtime
            .load(artifact.clone(), HostPolicy::default())
            .unwrap();
        runtime.invoke("main", vec![]).unwrap();
    }
}

fn field_texts(fields: &[guion_model::SharedText]) -> Vec<&str> {
    fields.iter().map(|field| &**field).collect()
}

#[test]
fn csv_reader_plain_records_match_the_general_parser() {
    let input = b"a,b\n,\n\r\n\nx,\r\r\ny\rz,1\r\n\"q,1\",2\n\"m\nn\",3\nsnow\xc3\xa9,4\nlast,5";
    let path = temp_file(input);
    let mut readers = FileReaderRegistry::new(1024, 1024);
    let handle = readers.open_csv(&path).unwrap();
    let mut rows = Vec::new();
    while let Some(row) = readers.next_csv(handle).unwrap() {
        let fields: Vec<String> = row.fields.iter().map(|field| field.to_string()).collect();
        rows.push((row.start_line, fields));
    }
    let expected: Vec<(usize, Vec<&str>)> = vec![
        (1, vec!["a", "b"]),
        (2, vec!["", ""]),
        (3, vec![""]),
        (4, vec![""]),
        (5, vec!["x", ""]),
        (6, vec!["y\rz", "1"]),
        (7, vec!["q,1", "2"]),
        (8, vec!["m\nn", "3"]),
        (10, vec!["snowé", "4"]),
        (11, vec!["last", "5"]),
    ];
    let rows: Vec<(usize, Vec<&str>)> = rows
        .iter()
        .map(|(line, fields)| (*line, fields.iter().map(String::as_str).collect()))
        .collect();
    assert_eq!(rows, expected);
    let _ = std::fs::remove_file(path);

    let invalid = temp_file(&[b'a', b'\n', 0xff, b',', b'b', b'\n']);
    let handle = readers.open_csv(&invalid).unwrap();
    assert_eq!(readers.next_csv(handle).unwrap().unwrap().start_line, 1);
    assert!(readers.next_csv(handle).is_err());
    let _ = std::fs::remove_file(invalid);
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
    assert_eq!(&*readers.next_line(handle).unwrap().unwrap().text, "last");
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
    assert_eq!(field_texts(&row1.fields), ["a", "", ""]);
    let row2 = readers.next_csv(handle).unwrap().unwrap();
    assert_eq!(row2.start_line, 2);
    assert_eq!(
        field_texts(&row2.fields),
        ["b,b", "say \"hi\"", "two\r\nlines"]
    );
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
fn local_time_resolver_reports_exact_gap_fold_and_typed_errors_without_capabilities() {
    let root = std::env::temp_dir().join(format!(
        "guion-local-time-resolution-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let source = r#"import "std:time"
fn compare_local(left: Int, right: Int) -> Int:
    let exact = unwrap(resolve_local(LocalDateTime(2026, 3, 8, 1, 30, 0, 0), "America/Los_Angeles"))
    if exact.kind == "exact" and exact.instants[0].unix_microseconds == 1772962200000000:
        return left - right
    return 1
fn main() -> Int:
    assert(time_zone_data_version() == "2026c")
    let exact = unwrap(resolve_local(LocalDateTime(2026, 3, 8, 1, 30, 0, 0), "America/Los_Angeles"))
    assert(exact.kind == "exact")
    assert(exact.local.hour == 1 and exact.timezone == "America/Los_Angeles")
    assert(len(exact.instants) == 1)
    assert(format_utc(exact.instants[0]) == "2026-03-08T09:30:00Z")
    let gap = unwrap(resolve_local(LocalDateTime(2026, 3, 8, 2, 30, 0, 0), "America/Los_Angeles"))
    assert(gap.kind == "gap" and len(gap.instants) == 0)
    let fold = unwrap(resolve_local(LocalDateTime(2026, 11, 1, 1, 30, 0, 0), "America/Los_Angeles"))
    assert(fold.kind == "fold" and len(fold.instants) == 2)
    assert(format_utc(fold.instants[0]) == "2026-11-01T08:30:00Z")
    assert(format_utc(fold.instants[1]) == "2026-11-01T09:30:00Z")
    let invalid_date = resolve_local(LocalDateTime(2026, 2, 29, 1, 30, 0, 0), "America/Los_Angeles")
    assert(not(is_ok(invalid_date)) and error(invalid_date).code == "E_DATE")
    let invalid_time = resolve_local(LocalDateTime(2026, 3, 8, 24, 0, 0, 0), "America/Los_Angeles")
    assert(not(is_ok(invalid_time)) and error(invalid_time).code == "E_TIME")
    let unknown = resolve_local(LocalDateTime(2026, 11, 1, 1, 30, 0, 0), "Not/A_Real_Zone")
    assert(not(is_ok(unknown)) and error(unknown).code == "E_TIMEZONE_UNKNOWN")
    assert(sort_by([2, 1], compare_local) == [1, 2])
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
    .expect("temporal resolver program compiles");
    for vm_only in [true, false] {
        let mut runtime = if vm_only {
            Runtime::vm_only()
        } else {
            Runtime::new()
        }
        .load(artifact.clone(), HostPolicy::default())
        .expect("pure temporal program runs without file or clock grants");
        assert_eq!(
            runtime.invoke("main", vec![]).unwrap(),
            guion_model::Value::Int(guion_model::BigInt::from_i64(0))
        );
        assert_eq!(runtime.take_output(), "");
        assert!(runtime.effect_report().is_empty());
    }
    let _ = std::fs::remove_dir_all(root);
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
    assert(text_join([], "|") == "")
    assert(text_join(["α", "β"], "🧬") == "α🧬β")
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
    assert(format_utc_date(instant) == "2024-01-01")
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
    assert_eq!(artifact.format_version, Artifact::CURRENT_VERSION);
    assert!(artifact.functions.iter().any(|function| {
        function
            .code
            .iter()
            .any(|instruction| instruction.op == Op::TextJoin)
    }));
    let artifact_bytes = artifact.to_bytes().expect("artifact encoding succeeds");
    let decoded = Artifact::from_bytes(&artifact_bytes).expect("artifact decoding succeeds");
    assert!(decoded.functions.iter().any(|function| {
        function
            .code
            .iter()
            .any(|instruction| instruction.op == Op::TextJoin)
    }));
    let mut previous_format = artifact.clone();
    previous_format.format_version = 42;
    assert_eq!(
        previous_format.to_bytes().unwrap_err().code,
        "E_ARTIFACT_VERSION"
    );
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

#[test]
fn sort_by_large_reverse_list_fits_log_linear_instruction_budget() {
    let source = r#"fn compare(left: Int, right: Int) -> Int:
    if left < right:
        return -1
    if left > right:
        return 1
    return 0
fn main() -> Int:
    var values: List[Int] = []
    var value = 2048
    while value > 0:
        set value = value - 1
        set values = list_push(values, value)
    let sorted = sort_by(values, compare)
    assert(len(sorted) == 2048)
    assert(sorted[0] == 0)
    assert(sorted[2047] == 2047)
    return 0
"#;
    let artifact = compile("sort-by-large.gu", source).expect("sort program compiles");
    let policy = HostPolicy {
        instruction_limit: 1_000_000,
        ..HostPolicy::default()
    };
    let mut runtime = Runtime::vm_only().load(artifact, policy).unwrap();
    runtime
        .invoke("main", vec![])
        .expect("a stable log-linear sort should fit this instruction budget");
}

#[test]
fn sort_by_large_list_is_stable_and_preserves_original_values() {
    let source = r#"record Item:
    key: Int
    order: Int
fn compare_key(left: Item, right: Item) -> Int:
    return left.key - right.key
fn main() -> Int:
    var items: List[Item] = []
    var index = 0
    while index < 96:
        set items = list_push(items, Item((index * 7) % 5, index))
        set index = index + 1
    let sorted = sort_by(items, compare_key)
    assert(len(sorted) == 96)
    assert(items[0].order == 0)
    var previous = sorted[0]
    var position = 1
    while position < len(sorted):
        let current = sorted[position]
        assert(previous.key <= current.key)
        if previous.key == current.key:
            assert(previous.order < current.order)
        set previous = current
        set position = position + 1
    return 0
"#;
    let artifact = compile("sort-by-stable-large.gu", source).expect("sort program compiles");
    for runtime in [Runtime::vm_only(), Runtime::new()] {
        let mut instance = runtime
            .load(artifact.clone(), HostPolicy::default())
            .unwrap();
        instance.invoke("main", vec![]).unwrap();
    }
}

#[test]
fn sort_by_int_key_is_stable_and_preserves_original_values() {
    let source = r#"record Item:
    key: Int
    order: Int
fn item_key(item: Item) -> Int:
    return item.key
fn main() -> Int:
    let items = [Item(1, 1), Item(0, 2), Item(1, 3)]
    let sorted = sort_by_int_key(items, item_key)
    assert(sorted[0].order == 2)
    assert(sorted[1].order == 1)
    assert(sorted[2].order == 3)
    assert(items[0].order == 1)
    return 0
"#;
    let artifact = compile("sort-by-int-key.gu", source).expect("named key function compiles");
    assert_eq!(artifact.format_version, Artifact::CURRENT_VERSION);
    let mut too_old = artifact.clone();
    too_old.format_version = 39;
    assert_eq!(too_old.to_bytes().unwrap_err().code, "E_ARTIFACT_VERSION");
    for runtime in [Runtime::vm_only(), Runtime::new()] {
        let mut instance = runtime
            .load(artifact.clone(), HostPolicy::default())
            .unwrap();
        instance.invoke("main", vec![]).unwrap();
    }
}

#[test]
fn sort_by_int_key_large_reverse_list_fits_log_linear_instruction_budget() {
    let source = r#"fn int_key(value: Int) -> Int:
    return value
fn main() -> Int:
    var values: List[Int] = []
    var value = 2048
    while value > 0:
        set value = value - 1
        set values = list_push(values, value)
    let sorted = sort_by_int_key(values, int_key)
    assert(len(sorted) == 2048)
    assert(sorted[0] == 0)
    assert(sorted[2047] == 2047)
    return 0
"#;
    let artifact = compile("sort-by-int-key-large.gu", source).expect("sort program compiles");
    let policy = HostPolicy {
        instruction_limit: 1_000_000,
        ..HostPolicy::default()
    };
    let mut runtime = Runtime::vm_only().load(artifact, policy).unwrap();
    runtime
        .invoke("main", vec![])
        .expect("key extraction and native merge sort fit the logarithmic instruction budget");
}

#[test]
fn sort_by_int_key_rejects_transitive_effects() {
    let effectful = r#"fn helper(value: Int) -> Int:
    print(value)
    return value
fn int_key(value: Int) -> Int:
    return helper(value)
fn main() -> Int:
    let values = sort_by_int_key([2, 1], int_key)
    return values[0]
"#;
    let error = compile("sort-by-int-key-effect.gu", effectful).unwrap_err();
    assert_eq!(error.code, "E_SORT_EFFECT");
}

#[test]
fn merge_sorted_by_int_key_is_stable_across_feeds() {
    let source = r#"record Item:
    key: Int
    order: Int
fn item_key(item: Item) -> Int:
    return item.key
fn main() -> Int:
    let first = [Item(1, 1), Item(3, 2)]
    let second = [Item(1, 3), Item(2, 4)]
    let feeds = [first, second]
    let merged = merge_sorted_by_int_key(feeds, item_key)
    assert(merged[0].order == 1)
    assert(merged[1].order == 3)
    assert(merged[2].order == 4)
    assert(merged[3].order == 2)
    assert(first[0].order == 1)
    assert(second[0].order == 3)
    return 0
"#;
    let artifact = compile("merge-sorted-by-int-key.gu", source)
        .expect("named key function compiles for sorted feed merge");
    assert_eq!(artifact.format_version, Artifact::CURRENT_VERSION);
    let mut too_old = artifact.clone();
    too_old.format_version = 39;
    assert_eq!(too_old.to_bytes().unwrap_err().code, "E_ARTIFACT_VERSION");
    for runtime in [Runtime::vm_only(), Runtime::new()] {
        let mut instance = runtime
            .load(artifact.clone(), HostPolicy::default())
            .unwrap();
        instance.invoke("main", vec![]).unwrap();
    }
}

#[test]
fn merge_sorted_by_int_key_rejects_unsorted_feeds() {
    let source = r#"fn int_key(value: Int) -> Int:
    return value
fn main() -> Int:
    let merged = merge_sorted_by_int_key([[2, 1]], int_key)
    return merged[0]
"#;
    let artifact = compile("merge-sorted-by-int-key-unsorted.gu", source)
        .expect("merge source compiles before runtime order validation");
    let mut runtime = Runtime::vm_only()
        .load(artifact, HostPolicy::default())
        .unwrap();
    let error = runtime.invoke("main", vec![]).unwrap_err();
    assert_eq!(error.code, "R_SORT_ORDER");
}

#[test]
fn native_text_list_parameters_match_the_vm() {
    let root = std::env::temp_dir().join(format!("guion-native-text-list-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let source = r#"import "std:text"
fn first_two(values: List[Text]) -> Text:
    var result = ""
    var index = 0
    while index < 2:
        set result = result + values[index]
        set index = index + 1
    return result
fn main() -> Int:
    let parts = text_split("a::b::", "::")
    return len(first_two(text_chars("a::b::"))) * 100 + len(parts)
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
    .expect("fixture compiles");
    let mut vm = Runtime::vm_only()
        .load(artifact.clone(), HostPolicy::default())
        .unwrap();
    let expected = vm.invoke("main", vec![]).unwrap();
    let mut native = Runtime::new()
        .load(artifact, HostPolicy::default())
        .unwrap();
    assert_eq!(native.invoke("main", vec![]).unwrap(), expected);
    let _ = std::fs::remove_dir_all(root);
}

// ---- csv_read_columns -------------------------------------------------------------------

const CSV_COLUMN_RECORDS: &str = r#"record FileReader:
    owner: Int
    id: Int
record CsvRow:
    start_line: Int
    fields: List[Text]
record CsvColumn:
    name: Text
    kind: Text
    ints: Array[i64]
    floats: Array[f64]
    flags: Array[bool]
    codes: Array[i64]
    dictionary: List[Text]
    valid: Array[bool]
"#;

/// Runs `main` of a program that may call `csv_read_columns`, with the given files in a fresh
/// project root, once with machine code off and once with it on. The two outputs must match.
fn run_csv_program(
    tag: &str,
    files: &[(&str, &[u8])],
    source: &str,
    adjust: impl Fn(&mut HostPolicy),
) -> String {
    let root = std::env::temp_dir().join(format!("guion-csv-columns-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    for (name, bytes) in files {
        std::fs::write(root.join(name), bytes).unwrap();
    }
    let artifact = compile("columns.gu", &format!("{CSV_COLUMN_RECORDS}{source}"))
        .unwrap_or_else(|error| panic!("{tag}: {}", error.message));
    let mut outputs = Vec::new();
    for vm_only in [true, false] {
        let mut policy = HostPolicy::default().with_file_access(&root, true, false);
        adjust(&mut policy);
        let mut runtime = if vm_only {
            Runtime::vm_only()
        } else {
            Runtime::new()
        }
        .load(artifact.clone(), policy)
        .unwrap();
        if let Err(error) = runtime.invoke("main", vec![]) {
            panic!("{tag}: {} {}", error.code, error.message);
        }
        outputs.push(runtime.take_output());
        assert_eq!(runtime.open_file_reader_count(), 0);
    }
    let _ = std::fs::remove_dir_all(&root);
    assert_eq!(
        outputs[0], outputs[1],
        "{tag}: machine code changed the result"
    );
    outputs.remove(0)
}

const NO_KINDS: &str = r#"fn no_kinds() -> Map[Text, Text]:
    let none: Map[Text, Text] = {}
    return none
"#;

#[test]
fn csv_read_columns_infers_kinds_at_the_boundaries() {
    let csv = "i,big,sci,dot,flag,cap,gap,holes,word,weird\n\
               1,1,1e5,.5,true,true,,1,a,1\n\
               2,9223372036854775808,2,1,false,True,,,b,inf\n\
               -0,3,3,2,true,false,,3,a,2\n\
               9223372036854775807,4,4,3,false,true,,,c,nan\n";
    let source = format!(
        r#"{NO_KINDS}
fn column(columns: List[CsvColumn], name: Text) -> CsvColumn:
    for c in columns:
        if c.name == name:
            return c
    return columns[0]

fn only_storage(c: CsvColumn, ints: Bool, floats: Bool, flags: Bool, text: Bool) -> Bool:
    var want_ints = 0
    var want_floats = 0
    var want_flags = 0
    var want_codes = 0
    if ints:
        set want_ints = 4
    if floats:
        set want_floats = 4
    if flags:
        set want_flags = 4
    if text:
        set want_codes = 4
    return array_size(c.ints) == want_ints and array_size(c.floats) == want_floats and array_size(c.flags) == want_flags and array_size(c.codes) == want_codes

fn main() -> Unit:
    let columns = unwrap(csv_read_columns("t.csv", no_kinds()))
    assert(len(columns) == 10)
    let i = column(columns, "i")
    assert(i.kind == "i64")
    assert(i.ints == array_i64([1, 2, 0, 9223372036854775807]))
    assert(only_storage(i, true, false, false, false))
    assert(len(i.dictionary) == 0)
    assert(i.valid == array_bool([true, true, true, true]))
    let big = column(columns, "big")
    assert(big.kind == "f64")
    assert(big.floats == array_f64([1.0, 9223372036854775808.0, 3.0, 4.0]))
    assert(only_storage(big, false, true, false, false))
    let sci = column(columns, "sci")
    assert(sci.kind == "f64")
    assert(sci.floats == array_f64([100000.0, 2.0, 3.0, 4.0]))
    let dot = column(columns, "dot")
    assert(dot.kind == "f64")
    assert(dot.floats == array_f64([0.5, 1.0, 2.0, 3.0]))
    let flag = column(columns, "flag")
    assert(flag.kind == "bool")
    assert(flag.flags == array_bool([true, false, true, false]))
    assert(only_storage(flag, false, false, true, false))
    let cap = column(columns, "cap")
    assert(cap.kind == "text")
    assert(cap.codes == array_i64([0, 1, 2, 0]))
    assert(cap.dictionary == ["true", "True", "false"])
    assert(only_storage(cap, false, false, false, true))
    let gap = column(columns, "gap")
    assert(gap.kind == "text")
    assert(gap.codes == array_i64([0, 0, 0, 0]))
    assert(len(gap.dictionary) == 0)
    assert(gap.valid == array_bool([false, false, false, false]))
    let holes = column(columns, "holes")
    assert(holes.kind == "i64")
    assert(holes.ints == array_i64([1, 0, 3, 0]))
    assert(holes.valid == array_bool([true, false, true, false]))
    let word = column(columns, "word")
    assert(word.codes == array_i64([0, 1, 0, 2]))
    assert(word.dictionary == ["a", "b", "c"])
    let weird = column(columns, "weird")
    assert(weird.kind == "text")
    assert(weird.dictionary == ["1", "inf", "2", "nan"])
    assert(weird.codes == array_i64([0, 1, 2, 3]))
    print("ok")
"#
    );
    let out = run_csv_program("infer", &[("t.csv", csv.as_bytes())], &source, |_| {});
    assert_eq!(out, "ok\n");
}

#[test]
fn csv_read_columns_splits_quoted_fields_like_csv_next() {
    let csv = "a,b,c\n\"x,y\",\"he said \"\"hi\"\"\",\"line1\nline2\"\nplain,\"\",tail\n\"x,y\",z,\"\"\"\"\n";
    let source = format!(
        r#"{NO_KINDS}
fn main() -> Unit:
    let columns = unwrap(csv_read_columns("q.csv", no_kinds()))
    let reader = unwrap(csv_open("q.csv"))
    let header = unwrap(unwrap(csv_next(reader)))
    assert(len(header.fields) == len(columns))
    var row = 0
    var running = true
    while running:
        let next = unwrap(csv_next(reader))
        if is_some(next):
            let fields = unwrap(next).fields
            var j = 0
            while j < len(columns):
                let c = columns[j]
                assert(c.kind == "text")
                assert(c.dictionary[array_get(c.codes, row)] == fields[j])
                set j = j + 1
            set row = row + 1
        else:
            set running = false
    unwrap(file_close(reader))
    assert(row == 3)
    assert(columns[0].dictionary == ["x,y", "plain"])
    assert(columns[1].dictionary == ["he said \"hi\"", "", "z"])
    assert(columns[2].dictionary == ["line1\nline2", "tail", "\""])
    print(row)
"#
    );
    let out = run_csv_program("quoted", &[("q.csv", csv.as_bytes())], &source, |_| {});
    assert_eq!(out, "3\n");
}

#[test]
fn csv_read_columns_drops_a_bom_and_skips_blank_lines() {
    let csv = b"\xEF\xBB\xBFn,v\r\n\r\n1,2\r\n\n3,4\r\n";
    let source = format!(
        r#"{NO_KINDS}
fn main() -> Unit:
    let columns = unwrap(csv_read_columns("b.csv", no_kinds()))
    assert(columns[0].name == "n")
    assert(columns[1].name == "v")
    assert(columns[0].ints == array_i64([1, 3]))
    assert(columns[1].ints == array_i64([2, 4]))
    print("ok")
"#
    );
    let out = run_csv_program("bom", &[("b.csv", csv)], &source, |_| {});
    assert_eq!(out, "ok\n");
}

#[test]
fn csv_read_columns_reads_a_blank_line_as_a_missing_value_in_a_one_column_file() {
    let csv = b"n\n1\n\n3\r\n\r\n5\n";
    let wide = b"n,v\n1,2\n\n3,4\n";
    let source = format!(
        r#"{NO_KINDS}
fn main() -> Unit:
    let columns = unwrap(csv_read_columns("one.csv", no_kinds()))
    assert(columns[0].kind == "i64")
    assert(columns[0].ints == array_i64([1, 0, 3, 0, 5]))
    assert(columns[0].valid == array_bool([true, false, true, false, true]))
    let two = unwrap(csv_read_columns("two.csv", no_kinds()))
    assert(two[0].ints == array_i64([1, 3]))
    print("ok")
"#
    );
    let out = run_csv_program(
        "one-column",
        &[("one.csv", csv), ("two.csv", wide)],
        &source,
        |_| {},
    );
    assert_eq!(out, "ok\n");
}

#[test]
fn csv_read_columns_treats_quoted_empty_as_text_in_text_columns_only() {
    let csv = "t,n,q,m\n\"\",\"\",\"\",1\nx,5,\"\",\"\"\n,6,\"\",x\n";
    let source = format!(
        r#"{NO_KINDS}
fn main() -> Unit:
    let columns = unwrap(csv_read_columns("e.csv", no_kinds()))
    let t = columns[0]
    assert(t.kind == "text")
    assert(t.dictionary == ["", "x"])
    assert(t.codes == array_i64([0, 1, 0]))
    assert(t.valid == array_bool([true, true, false]))
    let n = columns[1]
    assert(n.kind == "i64")
    assert(n.ints == array_i64([0, 5, 6]))
    assert(n.valid == array_bool([false, true, true]))
    let q = columns[2]
    assert(q.kind == "text")
    assert(q.dictionary == [""])
    assert(q.codes == array_i64([0, 0, 0]))
    assert(q.valid == array_bool([true, true, true]))
    let m = columns[3]
    assert(m.kind == "text")
    assert(m.dictionary == ["1", "", "x"])
    assert(m.codes == array_i64([0, 1, 2]))
    assert(m.valid == array_bool([true, true, true]))
    print("ok")
"#
    );
    let out = run_csv_program("empty", &[("e.csv", csv.as_bytes())], &source, |_| {});
    assert_eq!(out, "ok\n");
}

#[test]
fn csv_read_columns_fixes_kinds_by_name_and_reports_bad_values() {
    let source = format!(
        r#"{NO_KINDS}
fn fixed(a: Text, b: Text, c: Text) -> Map[Text, Text]:
    return {{"a": a, "b": b, "c": c}}

fn report(path: Text, kinds: Map[Text, Text]) -> Unit:
    let result = csv_read_columns(path, kinds)
    assert(not(is_ok(result)))
    let problem = error(result)
    print(file_error_code(problem) + " " + int_to_text(file_error_line(problem)) + ":" + int_to_text(file_error_column(problem)) + " " + file_error_message(problem))

fn main() -> Unit:
    let columns = unwrap(csv_read_columns("k.csv", fixed("f64", "text", "bool")))
    assert(columns[0].kind == "f64")
    assert(columns[0].floats == array_f64([1.0, 0.0]))
    assert(columns[0].valid == array_bool([true, false]))
    assert(columns[1].kind == "text")
    assert(columns[1].dictionary == ["007", "x"])
    assert(columns[2].kind == "bool")
    assert(columns[2].flags == array_bool([true, false]))
    assert(columns[2].valid == array_bool([true, false]))
    report("bad.csv", {{"a": "i64"}})
    report("k.csv", {{"c": "i64"}})
    report("bool.csv", {{"c": "bool"}})
    report("k.csv", {{"zzz": "i64"}})
    report("k.csv", {{"a": "decimal"}})
    report("ragged.csv", no_kinds())
"#
    );
    let out = run_csv_program(
        "fixed",
        &[
            ("k.csv", b"a,b,c\n1,007,true\n,x,\n"),
            ("bad.csv", b"a,b\n1,2\n1.5,3\n"),
            ("bool.csv", b"a,b,c\n1,2,true\n1,2,\n1,2,True\n"),
            ("ragged.csv", b"a,b\n1,2\n3\n"),
        ],
        &source,
        |_| {},
    );
    assert_eq!(
        out,
        "E_CSV_VALUE 3:1 column `a`: `1.5` is not i64\n\
         E_CSV_VALUE 2:3 column `c`: `true` is not i64\n\
         E_CSV_VALUE 4:3 column `c`: `True` is not bool\n\
         E_CSV_VALUE 1:0 no column named `zzz`\n\
         E_CSV_VALUE 1:0 unknown kind `decimal`\n\
         R_CSV_MALFORMED 3:0 CSV record has 1 fields, expected 2\n"
    );
}

#[test]
fn csv_read_columns_needs_the_file_read_capability() {
    let source = format!(
        r#"{NO_KINDS}
fn main() -> Unit:
    let columns = csv_read_columns("t.csv", no_kinds())
    let reader = csv_open("t.csv")
    assert(not(is_ok(columns)))
    assert(not(is_ok(reader)))
    assert(file_error_code(error(columns)) == file_error_code(error(reader)))
    print(file_error_code(error(columns)))
"#
    );
    let artifact = compile("columns.gu", &format!("{CSV_COLUMN_RECORDS}{source}")).unwrap();
    let mut runtime = Runtime::new()
        .load(artifact, HostPolicy::default())
        .unwrap();
    runtime.invoke("main", vec![]).unwrap();
    assert_eq!(runtime.take_output(), "R_HOST_DENIED\n");
}

#[test]
fn streaming_reads_ignore_max_file_bytes_but_whole_file_reads_keep_it() {
    let mut csv = String::from("id,name\n");
    for row in 0..400 {
        csv.push_str(&format!("{row},row{row}\n"));
    }
    assert!(csv.len() > 3000);
    let source = format!(
        r#"{NO_KINDS}
fn main() -> Unit:
    let columns = unwrap(csv_read_columns("big.csv", no_kinds()))
    print(array_size(columns[0].ints))
    let reader = unwrap(csv_open("big.csv"))
    var count = 0
    var running = true
    while running:
        let next = unwrap(csv_next(reader))
        if is_some(next):
            set count = count + 1
        else:
            set running = false
    unwrap(file_close(reader))
    print(count)
    let lines = unwrap(file_open_lines("big.csv"))
    var seen = 0
    var more = true
    while more:
        let line = unwrap(file_next_line(lines))
        if is_some(line):
            set seen = seen + 1
        else:
            set more = false
    unwrap(file_close(lines))
    print(seen)
    print(unwrap(read_text("big.csv")))
"#
    );
    let root = std::env::temp_dir().join(format!("guion-csv-limit-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("big.csv"), &csv).unwrap();
    let artifact = compile("columns.gu", &format!("{CSV_COLUMN_RECORDS}{source}")).unwrap();
    let mut policy = HostPolicy::default().with_file_access(&root, true, false);
    policy.max_file_bytes = 1024;
    let mut runtime = Runtime::new().load(artifact, policy).unwrap();
    let error = runtime.invoke("main", vec![]).unwrap_err();
    assert_eq!(error.code, "R_FILE_LIMIT");
    assert_eq!(runtime.take_output(), "400\n401\n401\n");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn csv_read_columns_charges_the_allocation_budget_and_names_the_setting() {
    let mut csv = String::from("id,score,name\n");
    for row in 0..20000 {
        csv.push_str(&format!("{row},{row}.5,row{row}\n"));
    }
    let source = format!(
        r#"{NO_KINDS}
fn main() -> Unit:
    let columns = unwrap(csv_read_columns("big.csv", no_kinds()))
    print(len(columns))
"#
    );
    let root = std::env::temp_dir().join(format!("guion-csv-alloc-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("big.csv"), &csv).unwrap();
    let artifact = compile("columns.gu", &format!("{CSV_COLUMN_RECORDS}{source}")).unwrap();
    for vm_only in [true, false] {
        let mut policy = HostPolicy::default().with_file_access(&root, true, false);
        policy.allocation_limit_bytes = 200_000;
        let mut runtime = if vm_only {
            Runtime::vm_only()
        } else {
            Runtime::new()
        }
        .load(artifact.clone(), policy)
        .unwrap();
        let error = runtime.invoke("main", vec![]).unwrap_err();
        assert_eq!(error.code, "R_ALLOCATION_LIMIT");
        assert!(
            error
                .message
                .ends_with("; raise `allocation_limit_bytes` under `[runtime]` in guion.toml"),
            "{}",
            error.message
        );
    }
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn csv_read_columns_reads_files_larger_than_its_buffer_and_rereads_late_text() {
    let rows = 20000;
    let mut csv = String::from("n,m,q,late\n");
    for row in 0..rows {
        let late = if row == rows - 1 {
            "z".to_string()
        } else {
            row.to_string()
        };
        csv.push_str(&format!("{row},{},\"a,{}\",{late}\n", row % 7, row % 5));
    }
    assert!(csv.len() > 3 * 64 * 1024 / 2);
    let source = format!(
        r#"{NO_KINDS}
fn main() -> Unit:
    let columns = unwrap(csv_read_columns("long.csv", no_kinds()))
    assert(columns[0].kind == "i64")
    assert(array_size(columns[0].ints) == 20000)
    assert(array_sum(columns[0].ints) == 199990000)
    assert(columns[1].kind == "i64")
    assert(array_sum(columns[1].ints) == 59997)
    assert(columns[2].kind == "text")
    assert(len(columns[2].dictionary) == 5)
    assert(columns[2].dictionary[3] == "a,3")
    assert(columns[3].kind == "text")
    assert(len(columns[3].dictionary) == 20000)
    assert(columns[3].dictionary[19999] == "z")
    assert(array_size(columns[3].ints) == 0)
    print("ok")
"#
    );
    let out = run_csv_program("long", &[("long.csv", csv.as_bytes())], &source, |_| {});
    assert_eq!(out, "ok\n");
}

#[test]
fn csv_read_columns_charges_the_text_pass_in_addition_to_the_numeric_pass() {
    let rows = 20000;
    let mut csv = String::from("a,b,c,d,e,f,late\n");
    for row in 0..rows {
        let late = if row == rows - 1 {
            "z".to_string()
        } else {
            format!("{row}")
        };
        csv.push_str(&format!("{row},{row},{row},{row},{row},{row},{late}\n"));
    }
    let source = format!(
        r#"{NO_KINDS}
fn main() -> Unit:
    let columns = unwrap(csv_read_columns("late.csv", no_kinds()))
    print(len(columns))
"#
    );
    let root = std::env::temp_dir().join(format!("guion-csv-twopass-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("late.csv"), &csv).unwrap();
    let artifact = compile("columns.gu", &format!("{CSV_COLUMN_RECORDS}{source}")).unwrap();
    let run = |limit: usize| {
        let mut policy = HostPolicy::default().with_file_access(&root, true, false);
        policy.allocation_limit_bytes = limit;
        let mut runtime = Runtime::vm_only().load(artifact.clone(), policy).unwrap();
        runtime.invoke("main", vec![]).map(|_| ())
    };
    // Six numeric columns stay charged while the text column is read again, so the two passes' buffers add up to more than either alone (about 2.4 MB for the second pass by itself).
    assert!(run(8_000_000).is_ok());
    let error = run(3_500_000).unwrap_err();
    assert_eq!(error.code, "R_ALLOCATION_LIMIT");
    let _ = std::fs::remove_dir_all(&root);
}

// A read that succeeds but whose result then breaks a limit must still close its effect record.
#[test]
fn csv_read_columns_closes_its_effect_when_the_result_breaks_a_limit() {
    let source = format!(
        r#"{NO_KINDS}
fn main() -> Unit:
    let columns = unwrap(csv_read_columns("words.csv", no_kinds()))
    print(len(columns))
"#
    );
    let root = std::env::temp_dir().join(format!("guion-csv-effect-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("words.csv"), "w\na\nb\nc\nd\n").unwrap();
    let artifact = compile("columns.gu", &format!("{CSV_COLUMN_RECORDS}{source}")).unwrap();
    let mut policy = HostPolicy::default().with_file_access(&root, true, false);
    // The dictionary of four words is a list longer than this.
    policy.max_collection_items = 2;
    let mut runtime = Runtime::vm_only().load(artifact, policy).unwrap();
    let error = runtime.invoke("main", vec![]).unwrap_err();
    assert_eq!(error.code, "R_COLLECTION_LIMIT", "{}", error.message);
    let effects = runtime.effect_report();
    assert_eq!(effects.len(), 1);
    assert_eq!(
        effects[0].outcome,
        guion_runtime::EffectOutcome::ReturnedError
    );
    let _ = std::fs::remove_dir_all(&root);
}

// The read's last charge covers the names and records of the result. When the budget refuses it,
// the error is raised and the effect record is still closed.
#[test]
fn csv_read_columns_closes_its_effect_when_the_result_does_not_fit_the_budget() {
    let columns = 3000usize;
    let header: Vec<String> = (0..columns).map(|index| format!("c{index:04}")).collect();
    let row = vec!["1"; columns];
    let source = format!(
        r#"{NO_KINDS}
fn main() -> Unit:
    let columns = unwrap(csv_read_columns("wide.csv", no_kinds()))
    print(len(columns))
"#
    );
    let root = std::env::temp_dir().join(format!("guion-csv-produced-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("wide.csv"),
        format!("{}\n{}\n", header.join(","), row.join(",")),
    )
    .unwrap();
    let artifact = compile("columns.gu", &format!("{CSV_COLUMN_RECORDS}{source}")).unwrap();
    // 256 bytes and the name for each column's record.
    let produced = columns * (256 + 5);
    let mut refused_at_the_result = 0;
    for limit in (1..=40).map(|step| step * 50_000) {
        let mut policy = HostPolicy::default().with_file_access(&root, true, false);
        policy.allocation_limit_bytes = limit;
        let mut runtime = Runtime::vm_only().load(artifact.clone(), policy).unwrap();
        let outcome = runtime.invoke("main", vec![]);
        let effects: Vec<_> = runtime
            .effect_report()
            .into_iter()
            .filter(|effect| effect.kind == guion_runtime::EffectKind::FileRead)
            .collect();
        assert_eq!(effects.len(), 1, "limit {limit}: {effects:?} {outcome:?}");
        match outcome {
            Ok(_) => assert_eq!(effects[0].outcome, guion_runtime::EffectOutcome::Returned),
            Err(error) => {
                assert_eq!(error.code, "R_ALLOCATION_LIMIT", "limit {limit}");
                assert_eq!(
                    effects[0].outcome,
                    guion_runtime::EffectOutcome::ReturnedError,
                    "limit {limit}: the effect record was left open"
                );
                if error.message.contains(&format!("requested {produced} ")) {
                    refused_at_the_result += 1;
                }
            }
        }
    }
    assert!(
        refused_at_the_result > 0,
        "no limit refused the result's own charge"
    );
    let _ = std::fs::remove_dir_all(&root);
}
