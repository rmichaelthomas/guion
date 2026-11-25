mod common;

use common::{connect_to, free_port, output, spawn};
use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

const BOUNDARY: &str = "XBOUNDARYX";

fn project(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("guion-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("up")).unwrap();
    dir
}

fn run_in(dir: &Path, source: &str) -> String {
    std::fs::write(dir.join("main.gu"), source).unwrap();
    let root = dir.to_string_lossy().into_owned();
    let result = output(
        Command::new(env!("CARGO_BIN_EXE_guion"))
            .args(["run", "--file-root", root.as_str(), "main.gu"])
            .current_dir(dir),
    )
    .unwrap();
    let stdout = String::from_utf8_lossy(&result.stdout).into_owned();
    assert!(
        result.status.success(),
        "{stdout}{}",
        String::from_utf8_lossy(&result.stderr)
    );
    stdout
}

fn escape(text: &str) -> String {
    text.replace('"', "\\\"")
}

/// The program every multipart test runs: it reads `body.bin`, builds a Request and prints the result
/// as `OK`, then `FIELDS <map>` and one `FILE field|name|bytes|type|stored` per file; or one
/// `ERR status|code|message`.
fn multipart_program(content_type: &str, limits: [i64; 5]) -> String {
    format!(
        r##"import "std:http_body"
import "std:http_response"
import "std:types"
fn main() -> Int:
    let body = unwrap(read_bytes("body.bin"))
    let request = Request("POST", "/", {{}}, [Header("content-type", "{}")], body, {{}}, "")
    let limits = BodyLimits({}, {}, {}, {}, {})
    match multipart_parse(request, "up", limits):
        Ok(parsed):
            print("OK")
            print("FIELDS")
            print(parsed.fields)
            var index = 0
            while index < len(parsed.files):
                let file = parsed.files[index]
                print("FILE " + file.field + "|" + file.original_name + "|" + int_to_text(file.bytes) + "|" + file.content_type + "|" + file.stored_path)
                set index = index + 1
        Err(problem):
            print("ERR " + int_to_text(problem.status) + "|" + problem.code + "|" + problem.message)
    return 0
"##,
        escape(content_type),
        limits[0],
        limits[1],
        limits[2],
        limits[3],
        limits[4]
    )
}

const DEFAULTS: [i64; 5] = [16, 8192, 1000, 8_388_608, 16_777_216];

fn part(headers: &[&str], data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(format!("--{BOUNDARY}\r\n").as_bytes());
    for header in headers {
        out.extend_from_slice(header.as_bytes());
        out.extend_from_slice(b"\r\n");
    }
    out.extend_from_slice(b"\r\n");
    out.extend_from_slice(data);
    out.extend_from_slice(b"\r\n");
    out
}

fn field(name: &str, value: &str) -> Vec<u8> {
    part(
        &[&format!("Content-Disposition: form-data; name=\"{name}\"")],
        value.as_bytes(),
    )
}

fn file(name: &str, filename: &str, data: &[u8]) -> Vec<u8> {
    part(
        &[
            &format!("Content-Disposition: form-data; name=\"{name}\"; filename=\"{filename}\""),
            "Content-Type: application/octet-stream",
        ],
        data,
    )
}

fn close(mut body: Vec<u8>) -> Vec<u8> {
    body.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());
    body
}

fn join(parts: Vec<Vec<u8>>) -> Vec<u8> {
    close(parts.concat())
}

fn content_type() -> String {
    format!("multipart/form-data; boundary={BOUNDARY}")
}

struct Outcome {
    stdout: String,
    dir: PathBuf,
}

impl Outcome {
    fn lines(&self, prefix: &str) -> Vec<String> {
        self.stdout
            .lines()
            .filter_map(|line| line.strip_prefix(prefix))
            .map(str::to_owned)
            .collect()
    }
    fn err(&self) -> String {
        self.lines("ERR ").first().cloned().unwrap_or_else(|| {
            panic!("expected a failure, got {}", self.stdout);
        })
    }
    fn ok(&self) {
        assert!(self.stdout.starts_with("OK"), "{}", self.stdout);
    }
    fn stored(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(self.dir.join("up"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }
}

fn parse_with(name: &str, ct: &str, body: &[u8], limits: [i64; 5]) -> Outcome {
    let dir = project(name);
    std::fs::write(dir.join("body.bin"), body).unwrap();
    let stdout = run_in(&dir, &multipart_program(ct, limits));
    Outcome { stdout, dir }
}

fn parse(name: &str, body: &[u8]) -> Outcome {
    parse_with(name, &content_type(), body, DEFAULTS)
}

fn tree(dir: &Path) -> BTreeSet<String> {
    fn walk(dir: &Path, prefix: &str, out: &mut BTreeSet<String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let name = format!("{prefix}/{}", entry.file_name().to_string_lossy());
            if entry.path().is_dir() {
                walk(&entry.path(), &name, out);
            }
            out.insert(name);
        }
    }
    let mut out = BTreeSet::new();
    walk(dir, "", &mut out);
    out
}

fn assert_err(outcome: &Outcome, status: u16, code: &str, names: &str) {
    let line = outcome.err();
    assert!(
        line.starts_with(&format!("{status}|{code}|")),
        "{line} (wanted {status} {code})"
    );
    assert!(line.contains(names), "{line} should name {names}");
    assert!(
        outcome.stored().is_empty(),
        "a refused body leaves no file: {:?}",
        outcome.stored()
    );
}

#[test]
fn a_traversal_filename_is_confined_and_nothing_lands_outside_the_root() {
    let dir = project("body-confine");
    let body = join(vec![file("doc", "../../etc/passwd", b"secret")]);
    std::fs::write(dir.join("body.bin"), &body).unwrap();
    std::fs::write(dir.join("main.gu"), "").unwrap();
    let before = tree(&dir);
    let stdout = run_in(&dir, &multipart_program(&content_type(), DEFAULTS));
    let outcome = Outcome { stdout, dir };
    outcome.ok();
    let files = outcome.lines("FILE ");
    assert_eq!(files.len(), 1, "{}", outcome.stdout);
    let fields: Vec<&str> = files[0].split('|').collect();
    assert_eq!(
        &fields[..4],
        ["doc", "passwd", "6", "application/octet-stream"]
    );
    let stored = fields[4];
    let name = stored.strip_prefix("up/").expect("stored under the root");
    assert_eq!(name.len(), 32, "{stored}");
    assert!(name.chars().all(|c| c.is_ascii_hexdigit()), "{stored}");
    assert_eq!(std::fs::read(outcome.dir.join(stored)).unwrap(), b"secret");
    let mut after = tree(&outcome.dir);
    // Only the one stored file (and main.gu, rewritten by the harness) is new anywhere.
    after.remove(&format!("/{stored}"));
    after.remove("/main.gu");
    after.retain(|path| !path.starts_with("/.guion"));
    let mut expected = before.clone();
    expected.remove("/main.gu");
    assert_eq!(after, expected);
    let parent: Vec<_> = std::fs::read_dir(outcome.dir.parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert!(!parent.iter().any(|n| n == "etc"), "{parent:?}");
}

#[test]
fn a_boundary_inside_binary_part_data_does_not_split_the_part() {
    let mut data: Vec<u8> = (0..=255u8).collect();
    data.extend_from_slice(format!("\r\n--{BOUNDARY}X tail").as_bytes());
    data.extend_from_slice(format!("--{BOUNDARY}").as_bytes());
    data.extend_from_slice(format!("\r\n--{BOUNDARY}-x").as_bytes());
    data.extend_from_slice(&[0, 0, 13, 10, 13, 10, 255, 254]);
    let body = join(vec![file("bin", "blob.bin", &data), field("after", "ok")]);
    let outcome = parse("body-binary", &body);
    outcome.ok();
    let files = outcome.lines("FILE ");
    let fields: Vec<&str> = files[0].split('|').collect();
    assert_eq!(fields[2], data.len().to_string());
    assert_eq!(
        std::fs::read(outcome.dir.join(fields[4])).unwrap(),
        data,
        "the stored bytes are the part data exactly"
    );
    assert!(
        outcome.stdout.contains("\"after\": [ok]"),
        "{}",
        outcome.stdout
    );
}

#[test]
fn the_total_limit_is_exact_and_names_itself() {
    let body = join(vec![field("a", "12345")]);
    let n = body.len() as i64;
    let at = parse_with(
        "body-total-at",
        &content_type(),
        &body,
        [16, 8192, 1000, 8_388_608, n],
    );
    at.ok();
    let over = parse_with(
        "body-total-over",
        &content_type(),
        &body,
        [16, 8192, 1000, 8_388_608, n - 1],
    );
    assert_err(&over, 413, "E_BODY_LIMIT", "max_total_bytes");
}

#[test]
fn a_part_without_a_filename_is_a_field_and_an_empty_file_input_is_skipped() {
    let body = join(vec![
        part(
            &["Content-Disposition: form-data; name=\"note\""],
            "caf\u{e9}".as_bytes(),
        ),
        file("empty", "", b""),
        file("real", "a.txt", b"x"),
    ]);
    let outcome = parse("body-field", &body);
    outcome.ok();
    assert!(
        outcome.stdout.contains("\"note\": [caf\u{e9}]"),
        "{}",
        outcome.stdout
    );
    let files = outcome.lines("FILE ");
    assert_eq!(files.len(), 1, "{}", outcome.stdout);
    assert!(files[0].starts_with("real|a.txt|1|"), "{}", files[0]);
}

#[test]
fn windows_nul_and_dot_only_filenames_are_sanitized() {
    let cases: [(&str, &str); 5] = [
        (r"..\..\x.txt", "x.txt"),
        (r"C:\Users\me\photo.png", "photo.png"),
        ("..", "unnamed"),
        ("....", "unnamed"),
        (".", "unnamed"),
    ];
    for (index, (given, wanted)) in cases.iter().enumerate() {
        let body = join(vec![file("f", given, b"1")]);
        let outcome = parse(&format!("body-name-{index}"), &body);
        outcome.ok();
        let files = outcome.lines("FILE ");
        let name = files[0].split('|').nth(1).unwrap().to_owned();
        assert_eq!(&name, wanted, "{given}");
        assert_eq!(outcome.stored().len(), 1);
    }
    // NUL, encoded: dropped, never part of a path.
    let body = join(vec![part(
        &["Content-Disposition: form-data; name=\"f\"; filename*=UTF-8''a%00b.txt"],
        b"1",
    )]);
    let outcome = parse("body-nul-encoded", &body);
    outcome.ok();
    assert!(
        outcome.lines("FILE ")[0].starts_with("f|ab.txt|1|"),
        "{}",
        outcome.stdout
    );
    // NUL, raw: a control character in a header is refused.
    let mut raw = Vec::new();
    raw.extend_from_slice(
        format!("--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"f\"; filename=\"a")
            .as_bytes(),
    );
    raw.push(0);
    raw.extend_from_slice(b"b.txt\"\r\n\r\n1\r\n");
    let body = close(raw);
    let outcome = parse("body-nul-raw", &body);
    assert_err(&outcome, 400, "E_BODY_MALFORMED", "control character");
}

#[test]
fn a_quoted_extended_filename_wins_and_a_bad_one_is_refused() {
    let body = join(vec![part(
        &["Content-Disposition: form-data; name=\"f\"; filename=\"evil.exe\"; filename*=\"UTF-8''%E2%82%AC%20rates.txt\""],
        b"1",
    )]);
    let outcome = parse("body-ext", &body);
    outcome.ok();
    assert!(
        outcome.lines("FILE ")[0].starts_with("f|\u{20ac} rates.txt|1|"),
        "{}",
        outcome.stdout
    );
    let latin = join(vec![part(
        &["Content-Disposition: form-data; name=\"f\"; filename*=ISO-8859-1''a.txt"],
        b"1",
    )]);
    assert_err(
        &parse("body-ext-latin", &latin),
        400,
        "E_BODY_MALFORMED",
        "UTF-8",
    );
    let bad = join(vec![part(
        &["Content-Disposition: form-data; name=\"f\"; filename*=UTF-8''%ZZ"],
        b"1",
    )]);
    assert_err(
        &parse("body-ext-bad", &bad),
        400,
        "E_BODY_MALFORMED",
        "percent",
    );
}

#[test]
fn a_filename_is_at_most_255_bytes() {
    let at = "a".repeat(255);
    let body = join(vec![file("f", &at, b"1")]);
    parse("body-name-255", &body).ok();
    let over = "a".repeat(256);
    let body = join(vec![file("f", &over, b"1")]);
    assert_err(
        &parse("body-name-256", &body),
        400,
        "E_BODY_MALFORMED",
        "255",
    );
}

#[test]
fn duplicate_field_names_keep_every_value_in_order() {
    let body = join(vec![
        field("tag", "one"),
        field("other", "x"),
        field("tag", "two"),
        field("tag", ""),
        field("tag", "three"),
    ]);
    let outcome = parse("body-dup", &body);
    outcome.ok();
    assert!(
        outcome
            .stdout
            .contains("\"other\": [x], \"tag\": [one, two, , three]"),
        "{}",
        outcome.stdout
    );
}

#[test]
fn the_part_count_limit_is_exact_and_ten_thousand_parts_hit_it() {
    let thousand: Vec<Vec<u8>> = (0..1000).map(|n| field(&format!("f{n}"), "v")).collect();
    parse("body-parts-1000", &join(thousand)).ok();
    let over: Vec<Vec<u8>> = (0..1001).map(|n| field(&format!("f{n}"), "v")).collect();
    assert_err(
        &parse("body-parts-1001", &join(over)),
        413,
        "E_BODY_LIMIT",
        "max_parts",
    );
    let many: Vec<Vec<u8>> = (0..10_000).map(|n| field(&format!("f{n}"), "v")).collect();
    assert_err(
        &parse("body-parts-10000", &join(many)),
        413,
        "E_BODY_LIMIT",
        "max_parts",
    );
}

#[test]
fn an_unterminated_final_boundary_and_a_missing_open_are_400() {
    let body = field("a", "1"); // no closing boundary line
    assert_err(
        &parse("body-unterminated", &body),
        400,
        "E_BODY_MALFORMED",
        "boundary",
    );
    let mut cut = join(vec![field("a", "1")]);
    cut.truncate(cut.len() - 4); // `--XBOUNDARYX` with the closing dashes cut
    assert_err(
        &parse("body-cut", &cut),
        400,
        "E_BODY_MALFORMED",
        "boundary",
    );
    let no_open = b"hello\r\n--XBOUNDARYX--\r\n".to_vec();
    assert_err(
        &parse("body-no-open", &no_open),
        400,
        "E_BODY_MALFORMED",
        "boundary",
    );
    let no_headers_end = format!("--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"a\"");
    assert_err(
        &parse("body-no-head-end", no_headers_end.as_bytes()),
        400,
        "E_BODY_MALFORMED",
        "boundary",
    );
}

#[test]
fn a_boundary_of_70_characters_is_accepted_and_71_is_not() {
    for (length, ok) in [(70usize, true), (71, false)] {
        let boundary = "b".repeat(length);
        let ct = format!("multipart/form-data; boundary={boundary}");
        let mut body = Vec::new();
        body.extend_from_slice(
            format!("--{boundary}\r\nContent-Disposition: form-data; name=\"a\"\r\n\r\nv\r\n--{boundary}--\r\n")
                .as_bytes(),
        );
        let outcome = parse_with(&format!("body-boundary-{length}"), &ct, &body, DEFAULTS);
        if ok {
            outcome.ok();
        } else {
            assert_err(&outcome, 400, "E_BODY_MALFORMED", "70");
        }
    }
    // A quoted boundary works, a bad character and a missing boundary do not.
    let quoted = "multipart/form-data; boundary=\"XBOUNDARYX\"";
    parse_with(
        "body-boundary-quoted",
        quoted,
        &join(vec![field("a", "1")]),
        DEFAULTS,
    )
    .ok();
    let bad = parse_with(
        "body-boundary-bad",
        "multipart/form-data; boundary=a<b",
        b"x",
        DEFAULTS,
    );
    assert_err(&bad, 400, "E_BODY_MALFORMED", "letters");
    let none = parse_with("body-boundary-none", "multipart/form-data", b"x", DEFAULTS);
    assert_err(&none, 400, "E_BODY_MALFORMED", "boundary");
    let other = parse_with("body-not-multipart", "text/plain", b"x", DEFAULTS);
    assert_err(&other, 400, "E_BODY_NOT_MULTIPART", "multipart");
}

#[test]
fn a_folded_part_header_is_refused() {
    let body = close(
        format!("--{BOUNDARY}\r\nContent-Disposition: form-data;\r\n name=\"a\"\r\n\r\nv\r\n")
            .into_bytes(),
    );
    assert_err(
        &parse("body-fold", &body),
        400,
        "E_BODY_MALFORMED",
        "obs-fold",
    );
    let tab = close(
        format!("--{BOUNDARY}\r\nContent-Disposition: form-data;\r\n\tname=\"a\"\r\n\r\nv\r\n")
            .into_bytes(),
    );
    assert_err(
        &parse("body-fold-tab", &tab),
        400,
        "E_BODY_MALFORMED",
        "obs-fold",
    );
    // A bare line feed does not end a header line either.
    let bare = close(
        format!("--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"a\"\nX: y\r\n\r\nv\r\n")
            .into_bytes(),
    );
    assert_err(
        &parse("body-bare-lf", &bare),
        400,
        "E_BODY_MALFORMED",
        "line",
    );
}

#[test]
fn a_part_needs_a_name_and_a_utf8_field_value() {
    let no_name = join(vec![part(&["Content-Disposition: form-data"], b"v")]);
    assert_err(
        &parse("body-noname", &no_name),
        400,
        "E_BODY_MALFORMED",
        "name",
    );
    let no_head = close(format!("--{BOUNDARY}\r\n\r\nv\r\n").into_bytes());
    assert_err(
        &parse("body-nohead", &no_head),
        400,
        "E_BODY_MALFORMED",
        "headers",
    );
    let not_utf8 = join(vec![part(
        &["Content-Disposition: form-data; name=\"a\""],
        &[0xff, 0xfe],
    )]);
    assert_err(
        &parse("body-utf8", &not_utf8),
        400,
        "E_BODY_MALFORMED",
        "UTF-8",
    );
    let attachment = join(vec![part(
        &["Content-Disposition: attachment; name=\"a\""],
        b"v",
    )]);
    assert_err(
        &parse("body-attach", &attachment),
        400,
        "E_BODY_MALFORMED",
        "form-data",
    );
}

#[test]
fn the_header_count_and_size_limits_are_exact() {
    let lines = |count: usize| -> Vec<String> {
        let mut out = vec!["Content-Disposition: form-data; name=\"a\"".to_owned()];
        for n in 1..count {
            out.push(format!("X-{n}: v"));
        }
        out
    };
    for (count, ok) in [(4usize, true), (5, false)] {
        let owned = lines(count);
        let refs: Vec<&str> = owned.iter().map(String::as_str).collect();
        let body = join(vec![part(&refs, b"v")]);
        let outcome = parse_with(
            &format!("body-headers-{count}"),
            &content_type(),
            &body,
            [4, 8192, 1000, 8_388_608, 16_777_216],
        );
        if ok {
            outcome.ok();
        } else {
            assert_err(&outcome, 413, "E_BODY_LIMIT", "max_headers");
        }
    }
    let block = "Content-Disposition: form-data; name=\"a\"\r\nX-Pad: 0123456789";
    let body = join(vec![part(&block.split("\r\n").collect::<Vec<_>>(), b"v")]);
    let n = block.len() as i64;
    parse_with(
        "body-hbytes-at",
        &content_type(),
        &body,
        [16, n, 1000, 8_388_608, 16_777_216],
    )
    .ok();
    let over = parse_with(
        "body-hbytes-over",
        &content_type(),
        &body,
        [16, n - 1, 1000, 8_388_608, 16_777_216],
    );
    assert_err(&over, 413, "E_BODY_LIMIT", "max_header_bytes");
}

#[test]
fn the_part_size_limit_is_exact_for_files_and_fields() {
    let data = vec![b'z'; 100];
    for (name, body) in [
        ("file", join(vec![file("f", "a.bin", &data)])),
        ("field", join(vec![field("f", &"z".repeat(100))])),
    ] {
        parse_with(
            &format!("body-part-at-{name}"),
            &content_type(),
            &body,
            [16, 8192, 1000, 100, 16_777_216],
        )
        .ok();
    }
    let big = vec![b'z'; 101];
    for (name, body) in [
        ("file", join(vec![file("f", "a.bin", &big)])),
        ("field", join(vec![field("f", &"z".repeat(101))])),
    ] {
        let outcome = parse_with(
            &format!("body-part-over-{name}"),
            &content_type(),
            &body,
            [16, 8192, 1000, 100, 16_777_216],
        );
        assert_err(&outcome, 413, "E_BODY_LIMIT", "max_part_bytes");
    }
}

#[test]
fn nothing_is_written_when_a_later_part_is_bad() {
    let body = join(vec![
        file("one", "a.bin", b"first"),
        file("two", "b.bin", b"second"),
        part(&["Content-Disposition: form-data; name=\"c\""], &[0xff]),
    ]);
    let outcome = parse("body-atomic", &body);
    assert_err(&outcome, 400, "E_BODY_MALFORMED", "UTF-8");
}

#[test]
fn negative_limits_are_a_server_error() {
    let body = join(vec![field("a", "1")]);
    let outcome = parse_with(
        "body-limits-negative",
        &content_type(),
        &body,
        [16, 8192, 1000, 8_388_608, -1],
    );
    assert_err(&outcome, 500, "E_BODY_LIMITS_INVALID", "zero or more");
}

// ---- urlencoded -----------------------------------------------------------------------------

fn form_program(body: &str, limits: [i64; 5]) -> String {
    format!(
        r##"import "std:http_body"
import "std:http_response"
import "std:types"
fn main() -> Int:
    let limits = BodyLimits({}, {}, {}, {}, {})
    match form_parse(bytes_from_text("{}"), limits):
        Ok(fields):
            print("OK")
            print(fields)
        Err(problem):
            print("ERR " + int_to_text(problem.status) + "|" + problem.code + "|" + problem.message)
    return 0
"##,
        limits[0],
        limits[1],
        limits[2],
        limits[3],
        limits[4],
        escape(body)
    )
}

fn form(name: &str, body: &str, limits: [i64; 5]) -> String {
    let dir = project(name);
    let stdout = run_in(&dir, &form_program(body, limits));
    let _ = std::fs::remove_dir_all(&dir);
    stdout.trim().to_owned()
}

#[test]
fn a_urlencoded_body_becomes_a_map_with_repeats_in_order() {
    let out = form("form-basic", "a=1&b=x+y%21&a=%C3%A9&flag&&=skip", DEFAULTS);
    assert_eq!(
        out,
        "OK\n{\"\": [skip], \"a\": [1, \u{e9}], \"b\": [x y!], \"flag\": []}"
    );
}

#[test]
fn form_limits_are_exact_and_a_bad_escape_is_400() {
    assert_eq!(form("form-empty", "", DEFAULTS), "OK\n{}");
    let body = "a=1&b=2";
    let n = body.len() as i64;
    assert!(form("form-total-at", body, [16, 8192, 1000, 100, n]).starts_with("OK"));
    let over = form("form-total-over", body, [16, 8192, 1000, 100, n - 1]);
    assert!(over.starts_with("ERR 413|E_BODY_LIMIT|"), "{over}");
    assert!(over.contains("max_total_bytes"), "{over}");
    assert!(form("form-pairs-at", "a=1&b=2", [16, 8192, 2, 100, 100]).starts_with("OK"));
    let over = form("form-pairs-over", "a=1&b=2&c=3", [16, 8192, 2, 100, 100]);
    assert!(over.starts_with("ERR 413|E_BODY_LIMIT|"), "{over}");
    assert!(over.contains("max_parts"), "{over}");
    assert!(form("form-part-at", "abc=defg", [16, 8192, 10, 4, 100]).starts_with("OK"));
    let over = form("form-part-over", "abc=defgh", [16, 8192, 10, 4, 100]);
    assert!(over.starts_with("ERR 413|E_BODY_LIMIT|"), "{over}");
    assert!(over.contains("max_part_bytes"), "{over}");
    let bad = form("form-bad", "a=%zz", DEFAULTS);
    assert!(bad.starts_with("ERR 400|E_BODY_MALFORMED|"), "{bad}");
}

// ---- refusing before reading ----------------------------------------------------------------

#[test]
fn an_oversized_body_is_refused_with_413_before_it_is_read() {
    let port = free_port();
    let dir = project("body-socket");
    let source = format!(
        r##"import "std:http_body"
import "std:http_server"
import "std:http_response"
import "std:net"
import "std:types"
fn main() -> Int:
    let limits = BodyLimits(16, 8192, 1000, 1000, 5000)
    let listener = unwrap(http_server_listen("127.0.0.1", {port}))
    match http_server_accept(listener, 20000):
        Ok(conn):
            match http_server_read_request(conn, 20000, body_read_limit(limits)):
                Ok(raw):
                    print("read")
                Err(problem):
                    let failure = body_read_failure(problem, limits)
                    print(failure.code)
                    print(failure.message)
        Err(problem):
            print("accept " + problem.code)
    return 0
"##
    );
    std::fs::write(dir.join("main.gu"), source).unwrap();
    let root = dir.to_string_lossy().into_owned();
    let server = spawn(
        Command::new(env!("CARGO_BIN_EXE_guion"))
            .args([
                "run",
                "--allow-bind",
                "127.0.0.1:*",
                "--file-root",
                root.as_str(),
                "main.gu",
            ])
            .current_dir(&dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    )
    .expect("guion starts");
    let mut stream = None;
    for _ in 0..100 {
        if let Ok(s) = connect_to(port) {
            stream = Some(s);
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let mut stream = stream.expect("server listens");
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    // Declares 50 MB and sends only the head plus one buffer's worth: the answer must come anyway.
    let mut request = b"POST /up HTTP/1.1\r\nHost: t\r\nContent-Length: 50000000\r\n\r\n".to_vec();
    request.extend_from_slice(&[b'a'; 1000]);
    stream.write_all(&request).unwrap();
    let mut response = Vec::new();
    let mut buffer = [0u8; 512];
    while !String::from_utf8_lossy(&response).contains("larger") {
        let n = stream
            .read(&mut buffer)
            .expect("an answer before the body is sent");
        if n == 0 {
            break;
        }
        response.extend_from_slice(&buffer[..n]);
    }
    let text = String::from_utf8_lossy(&response).into_owned();
    assert!(text.starts_with("HTTP/1.1 413"), "{text}");
    assert!(text.contains("max_body_bytes (5000)"), "{text}");
    let result = server.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert!(
        result.status.success(),
        "{stdout}{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(
        stdout.starts_with("E_BODY_LIMIT\nmax_total_bytes (5000) exceeded"),
        "{stdout}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- documentation surface ------------------------------------------------------------------

fn doc_json(module: &str) -> serde_json::Value {
    let result =
        output(Command::new(env!("CARGO_BIN_EXE_guion")).args(["doc", module, "--json"])).unwrap();
    assert!(
        result.status.success(),
        "{module}: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    serde_json::from_slice(&result.stdout).unwrap()
}

const HANDLE_TYPES: &[&str] = &[
    "TcpConn",
    "TcpListener",
    "UdpSock",
    "FileReader",
    "Db",
    "Transaction",
    "TomlDocument",
];

#[test]
fn no_handle_in_public_signatures() {
    let report = doc_json("std:http_body");
    let functions = report["functions"].as_array().unwrap();
    for name in [
        "form_parse",
        "multipart_parse",
        "body_limits_default",
        "body_read_limit",
        "body_read_failure",
    ] {
        assert!(functions.iter().any(|f| f["name"] == name), "{name}");
    }
    for function in functions {
        let text = function.to_string();
        assert!(
            !HANDLE_TYPES.iter().any(|handle| text.contains(handle)),
            "{text}"
        );
    }
    for record in report["records"].as_array().unwrap() {
        let text = record.to_string();
        assert!(
            !HANDLE_TYPES.iter().any(|handle| text.contains(handle)),
            "{text}"
        );
    }
}

#[test]
fn every_function_declares_its_effects() {
    let report = doc_json("std:http_body");
    let effects = |name: &str| -> Vec<String> {
        report["functions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|function| function["name"] == name)
            .unwrap_or_else(|| panic!("{name} is documented"))["effects"]
            .as_array()
            .expect("an effects array on every function")
            .iter()
            .map(|effect| effect.as_str().unwrap().to_owned())
            .collect()
    };
    assert!(effects("form_parse").is_empty());
    assert!(effects("body_limits_default").is_empty());
    assert!(effects("body_read_limit").is_empty());
    assert!(effects("body_read_failure").is_empty());
    assert!(effects("multipart_parse").contains(&"filesystem.write".to_owned()));
}
