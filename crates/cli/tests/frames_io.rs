//! The `frames` package reads CSV and JSONL files, writes CSV, and prints frames as tables.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct App(PathBuf);

impl Drop for App {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn guion(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(args)
        .current_dir(dir)
        .output()
        .expect("guion starts")
}

/// A project that depends on the package, with `files` written into it and `body` as `main`.
fn app(name: &str, files: &[(&str, &str)], body: &str) -> App {
    let dir = std::env::temp_dir().join(format!("guion-frames-io-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("guion.toml"),
        "[project]\nname = \"app\"\nversion = \"0.1.0\"\nentry = \"src/main.gu\"\n",
    )
    .unwrap();
    for (file, contents) in files {
        std::fs::write(dir.join(file), contents).unwrap();
    }
    let mut source = String::from(
        "import \"pkg:frames/src/lib.gu\"\n\nfn kinds(frame: Frame) -> Text:\n    var out = \"\"\n    for column in frame_columns(frame):\n        set out = out + column_name(column) + \":\" + column_kind(column) + \":\" + int_to_text(column_missing(column)) + \" \"\n    return out\n\nfn code(result: Result[Frame, StdError]) -> Text:\n    match result:\n        Ok(frame):\n            return \"ok\"\n        Err(problem):\n            return problem.code + \" \" + problem.message\n\nfn main() -> Unit:\n",
    );
    for line in body.lines() {
        source.push_str("    ");
        source.push_str(line);
        source.push('\n');
    }
    std::fs::write(dir.join("src/main.gu"), source).unwrap();
    let package = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packages/frames");
    let added = guion(
        &dir,
        &["add", "frames", "--path", package.to_str().unwrap()],
    );
    assert!(
        added.status.success(),
        "{}",
        String::from_utf8_lossy(&added.stderr)
    );
    App(dir)
}

fn stdout(app: &App) -> String {
    let ran = guion(&app.0, &["run", "src/main.gu"]);
    assert!(
        ran.status.success(),
        "{}{}",
        String::from_utf8_lossy(&ran.stdout),
        String::from_utf8_lossy(&ran.stderr)
    );
    String::from_utf8_lossy(&ran.stdout).into_owned()
}

const PEOPLE: &str = "id,score,flag,name,big,mixed,blank,n\n1,0.5,true,ann,99999999999999999999,1,,1\n2,,false,bob,1,true,,2.5\n3,2,true,,2,x,,3\n";

#[test]
fn csv_kinds_are_inferred_from_every_value() {
    let app = app(
        "infer",
        &[("people.csv", PEOPLE)],
        "let frame = unwrap(frame_read_csv(\"people.csv\"))\nprint(frame_rows(frame))\nprint(kinds(frame))\nlet mixed = unwrap(frame_column(frame, \"mixed\"))\nprint(column_cell_text(mixed, 0) + \"|\" + column_cell_text(mixed, 1) + \"|\" + column_cell_text(mixed, 2))\nprint(column_cell_text(unwrap(frame_column(frame, \"score\")), 1))\nprint(column_cell_text(unwrap(frame_column(frame, \"n\")), 0))\nprint(column_cell_text(unwrap(frame_column(frame, \"big\")), 1))",
    );
    assert_eq!(
        stdout(&app),
        "3\nid:i64:0 score:f64:1 flag:bool:0 name:text:1 big:f64:0 mixed:text:0 blank:text:3 n:f64:0 \n1|true|x\nNA\n1.0\n1.0\n"
    );
}

#[test]
fn csv_errors_name_the_line_and_column() {
    let app = app(
        "errors",
        &[
            ("people.csv", PEOPLE),
            ("ragged.csv", "a,b\n1,2\n3\n"),
            ("empty.csv", ""),
            ("dupes.csv", "a,a\n1,2\n"),
        ],
        "print(code(frame_read_csv(\"ragged.csv\")))\nprint(code(frame_read_csv(\"empty.csv\")))\nprint(code(frame_read_csv(\"dupes.csv\")))\nprint(code(frame_read_csv(\"absent.csv\")))\nprint(code(frame_read_csv_with(\"people.csv\", {\"name\": \"i64\"})))\nprint(code(frame_read_csv_with(\"people.csv\", {\"nope\": \"i64\"})))\nprint(code(frame_read_csv_with(\"people.csv\", {\"id\": \"date\"})))\nprint(kinds(unwrap(frame_read_csv_with(\"people.csv\", {\"id\": \"text\", \"n\": \"f64\", \"blank\": \"bool\"}))))",
    );
    let out = stdout(&app);
    let lines: Vec<&str> = out.lines().collect();
    assert!(
        lines[0].starts_with("E_FRAME_PARSE") && lines[0].contains("line 3"),
        "{out}"
    );
    assert!(lines[1].starts_with("E_FRAME_PARSE"), "{out}");
    assert!(lines[2].starts_with("E_FRAME_COLUMN"), "{out}");
    assert!(lines[3].starts_with("R_FILE_NOT_FOUND"), "{out}");
    assert!(
        lines[4].starts_with("E_FRAME_PARSE")
            && lines[4].contains("line 2")
            && lines[4].contains("`name`"),
        "{out}"
    );
    assert!(lines[5].starts_with("E_FRAME_COLUMN"), "{out}");
    assert!(lines[6].starts_with("E_FRAME_KIND"), "{out}");
    assert_eq!(
        lines[7],
        "id:text:0 score:f64:1 flag:bool:0 name:text:1 big:f64:0 mixed:text:0 blank:bool:3 n:f64:0 "
    );
}

#[test]
fn csv_round_trips_kinds_values_and_missing_values() {
    let app = app(
        "round",
        &[("people.csv", PEOPLE)],
        "let frame = unwrap(frame_read_csv(\"people.csv\"))\nprint(unwrap(frame_write_csv(frame, \"out.csv\")))\nlet again = unwrap(frame_read_csv(\"out.csv\"))\nprint(kinds(again))\nprint(frame_show(again, 10) == frame_show(frame, 10))",
    );
    assert_eq!(
        stdout(&app),
        "3\nid:i64:0 score:f64:1 flag:bool:0 name:text:1 big:f64:0 mixed:text:0 blank:text:3 n:f64:0 \ntrue\n"
    );
    let written = std::fs::read_to_string(app.0.join("out.csv")).unwrap();
    assert_eq!(
        written.lines().next().unwrap(),
        "id,score,flag,name,big,mixed,blank,n"
    );
    assert_eq!(
        written.lines().nth(2).unwrap(),
        "2,,false,bob,1.0,true,,2.5"
    );
}

#[test]
fn many_rows_cross_reading_chunks() {
    let mut csv = String::from("k,v,t\n");
    for row in 0..70_000 {
        if row == 69_999 {
            csv.push_str(&format!("{row},0.5,late\n"));
        } else {
            csv.push_str(&format!(
                "{row},{},{}\n",
                row % 7,
                if row % 3 == 0 { "" } else { "x" }
            ));
        }
    }
    let app = app(
        "chunks",
        &[("big.csv", &csv)],
        "let frame = unwrap(frame_read_csv(\"big.csv\"))\nprint(kinds(frame))\nprint(column_sum(unwrap(frame_column(frame, \"v\"))))\nprint(column_cell_text(unwrap(frame_column(frame, \"t\")), 69999))",
    );
    let expected_sum: f64 = (0..69_999).map(|row| f64::from(row % 7)).sum::<f64>() + 0.5;
    assert_eq!(
        stdout(&app),
        format!("k:i64:0 v:f64:0 t:text:23333 \n{expected_sum}\nlate\n")
    );
}

#[test]
fn jsonl_columns_are_the_keys_of_every_object() {
    let app = app(
        "jsonl",
        &[(
            "rows.jsonl",
            "{\"b\": \"x\", \"a\": 1, \"c\": 1.5}\n{\"a\": 2.5, \"b\": null, \"d\": true}\n\n{\"a\": 3, \"c\": 2, \"b\": \"y\"}\n",
        ), ("bad.jsonl", "{\"a\": 1}\n{\"a\": \n")],
        "let frame = unwrap(frame_read_jsonl(\"rows.jsonl\"))\nprint(kinds(frame))\nprint(column_cell_text(unwrap(frame_column(frame, \"a\")), 0))\nprint(column_cell_text(unwrap(frame_column(frame, \"b\")), 2))\nprint(code(frame_read_jsonl(\"bad.jsonl\")))",
    );
    let out = stdout(&app);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], "a:f64:0 b:text:1 c:f64:1 d:bool:2 ");
    assert_eq!(lines[1], "1.0");
    assert_eq!(lines[2], "y");
    assert!(
        lines[3].starts_with("E_FRAME_PARSE") && lines[3].contains("line 2"),
        "{out}"
    );
}

#[test]
fn show_prints_an_aligned_table() {
    let app = app(
        "show",
        &[(
            "t.csv",
            "id,x,name,ok\n1,0.5,a,true\n2,,bb,false\n3,10.25,a,true\n4,2.5,a,\n5,-1,ccc,true\n",
        )],
        "let frame = unwrap(frame_read_csv(\"t.csv\"))\nprint(frame_show(frame, 10))\nprint(frame_show(frame, 4))",
    );
    assert_eq!(
        stdout(&app),
        concat!(
            "id      x  name  ok\n",
            " 1    0.5  a     true\n",
            " 2     NA  bb    false\n",
            " 3  10.25  a     true\n",
            " 4    2.5  a     NA\n",
            " 5   -1.0  ccc   true\n",
            "[5 rows x 4 columns]\n",
            "id     x  name  ok\n",
            " 1   0.5  a     true\n",
            " 2    NA  bb    false\n",
            " …     …  …     …\n",
            " 4   2.5  a     NA\n",
            " 5  -1.0  ccc   true\n",
            "[5 rows x 4 columns]\n",
        )
    );
}

#[test]
fn jsonl_line_that_is_not_an_object_is_a_parse_error() {
    let app = app(
        "jsonl-shape",
        &[
            ("later.jsonl", "{\"a\": 1}\n5\n"),
            ("first.jsonl", "[\"a\", \"b\"]\n{\"a\": 1}\n"),
        ],
        "print(code(frame_read_jsonl(\"later.jsonl\")))\nprint(code(frame_read_jsonl(\"first.jsonl\")))",
    );
    let out = stdout(&app);
    let lines: Vec<&str> = out.lines().collect();
    assert!(
        lines[0].starts_with("E_FRAME_PARSE") && lines[0].contains("line 2"),
        "{out}"
    );
    assert!(
        lines[1].starts_with("E_FRAME_PARSE") && lines[1].contains("line 1"),
        "{out}"
    );
}

#[test]
fn csv_blank_lines_are_skipped_except_in_a_one_column_file() {
    let app = app(
        "blank",
        &[("wide.csv", "a,b\n1,2\n\n3,4\n\n"), ("narrow.csv", "a\n1\n\n2\n")],
        "print(frame_rows(unwrap(frame_read_csv(\"wide.csv\"))))\nprint(frame_rows(unwrap(frame_read_csv(\"narrow.csv\"))))",
    );
    assert_eq!(stdout(&app), "2\n3\n");
}

#[test]
fn csv_byte_order_mark_is_not_part_of_the_first_name() {
    let app = app(
        "bom",
        &[("bom.csv", "\u{feff}a,b\n1,2\n")],
        "print(text_join_native(frame_names(unwrap(frame_read_csv(\"bom.csv\"))), \"|\"))",
    );
    assert_eq!(stdout(&app), "a|b\n");
}

#[test]
fn csv_empty_text_round_trips_distinct_from_missing() {
    let app = app(
        "emptytext",
        &[("in.csv", "name,n\nann,1\n\"\",2\n,3\nbob,\n")],
        "let frame = unwrap(frame_read_csv(\"in.csv\"))\nlet name = unwrap(frame_column(frame, \"name\"))\nprint(column_missing(name))\nprint(unwrap(frame_write_csv(frame, \"out.csv\")))\nlet again = unwrap(frame_read_csv(\"out.csv\"))\nlet back = unwrap(frame_column(again, \"name\"))\nprint(column_missing(back))\nprint(array_to_list(column_valid(back)) == array_to_list(column_valid(name)))\nprint(frame_show(again, 10) == frame_show(frame, 10))",
    );
    assert_eq!(stdout(&app), "1\n4\n1\ntrue\ntrue\n");
    let written = std::fs::read_to_string(app.0.join("out.csv")).unwrap();
    assert_eq!(written, "name,n\nann,1\n\"\",2\n,3\nbob,");
}

#[test]
fn csv_reads_files_above_a_small_max_file_bytes() {
    let mut csv = String::from("k,t\n");
    for row in 0..2_000 {
        csv.push_str(&format!("{row},value-{}\n", row % 13));
    }
    let app = app(
        "smalllimit",
        &[("big.csv", &csv)],
        "let frame = unwrap(frame_read_csv(\"big.csv\"))\nprint(frame_rows(frame))\nprint(column_sum(unwrap(frame_column(frame, \"k\"))))",
    );
    let manifest = app.0.join("guion.toml");
    let mut toml = std::fs::read_to_string(&manifest).unwrap();
    toml.push_str("\n[runtime]\nmax_file_bytes = 4096\n");
    std::fs::write(&manifest, toml).unwrap();
    assert!(csv.len() > 4096);
    assert_eq!(stdout(&app), "2000\n1999000\n");
}

#[test]
fn show_aligns_wide_characters_by_display_width() {
    let app = app(
        "show-width",
        &[],
        "let frame = unwrap(frame_new([column_text(\"t\", [\"日本\", \"ab\", \"e\\u{301}\", \"a\\nb\", \"c\\td\"]), column_i64(\"n\", [1, 2, 3, 4, 5])]))\nprint(frame_show(frame, 10))",
    );
    assert_eq!(
        stdout(&app),
        concat!(
            "t     n\n",
            "日本  1\n",
            "ab    2\n",
            "e\u{301}     3\n",
            "a\\nb  4\n",
            "c\\td  5\n",
            "[5 rows x 2 columns]\n",
        )
    );
}

#[test]
fn csv_writes_extreme_floats_in_exponent_form_that_reads_back() {
    let app = app(
        "exponent",
        &[],
        "let values = [1e20, 1.5e-7, 0.0, 1e16, 0.0001, 0.00001, -2.5e-10, 1e300, 123456789012345680.0, 1.0, 1234.5, -1e22]\nlet frame = unwrap(frame_new([column_f64(\"x\", values)]))\nprint(unwrap(frame_write_csv(frame, \"out.csv\")))\nlet again = unwrap(frame_read_csv(\"out.csv\"))\nlet x = unwrap(frame_column(again, \"x\"))\nprint(column_kind(x))\nprint(array_to_list(column_floats(x)) == values)",
    );
    assert_eq!(stdout(&app), "12\nf64\ntrue\n");
    let written = std::fs::read_to_string(app.0.join("out.csv")).unwrap();
    assert_eq!(
        written,
        "x\n1e+20\n1.5e-07\n0.0\n1e+16\n0.0001\n1e-05\n-2.5e-10\n1e+300\n1.2345678901234568e+17\n1.0\n1234.5\n-1e+22"
    );
}

#[test]
fn jsonl_keys_that_first_appear_later_become_columns() {
    let app = app(
        "jsonl-union",
        &[(
            "rows.jsonl",
            "{\"b\": 1, \"a\": 2}\n{\"a\": 3, \"z\": \"s\", \"c\": true}\n{\"y\": 1.5, \"b\": 4}\n",
        )],
        "let frame = unwrap(frame_read_jsonl(\"rows.jsonl\"))\nprint(text_join_native(frame_names(frame), \"|\"))\nprint(kinds(frame))\nprint(column_cell_text(unwrap(frame_column(frame, \"z\")), 0) + \"|\" + column_cell_text(unwrap(frame_column(frame, \"z\")), 1))",
    );
    assert_eq!(
        stdout(&app),
        "a|b|c|z|y\na:i64:1 b:i64:1 c:bool:2 z:text:2 y:f64:2 \nNA|s\n"
    );
}

#[test]
fn a_one_column_csv_keeps_a_missing_value_through_a_round_trip() {
    let app = app(
        "one-column",
        &[("in.csv", "n\n1\n\n3\n")],
        "let frame = unwrap(frame_read_csv(\"in.csv\"))\nprint(frame_rows(frame))\nprint(unwrap(frame_write_csv(frame, \"out.csv\")))\nlet again = unwrap(frame_read_csv(\"out.csv\"))\nprint(frame_rows(again))\nprint(column_missing(unwrap(frame_column(again, \"n\"))))\nprint(frame_show(again, 10) == frame_show(frame, 10))",
    );
    assert_eq!(stdout(&app), "3\n3\n3\n1\ntrue\n");
}
