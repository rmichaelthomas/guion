mod common;

use common::output;
use std::path::PathBuf;
use std::process::Command;
use std::time::Instant;

fn project(name: &str, source: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("guion-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("main.gu"), source).unwrap();
    dir
}

fn run_program(name: &str, source: &str) -> String {
    let dir = project(name, source);
    let result = output(
        Command::new(env!("CARGO_BIN_EXE_guion"))
            .arg("run")
            .arg("main.gu")
            .current_dir(&dir),
    )
    .unwrap();
    let stdout = String::from_utf8_lossy(&result.stdout).into_owned();
    assert!(
        result.status.success(),
        "{stdout}{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let _ = std::fs::remove_dir_all(&dir);
    stdout
}

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

#[test]
fn a_cookie_with_every_flag_round_trips_through_the_parser() {
    let stdout = run_program(
        "cookie-roundtrip",
        r##"import "std:cookie"
import "std:http_response"
import "std:time"
import "std:types"
fn main() -> Int:
    let base = unwrap(cookie_new("sid", "abc 123"))
    let secured = cookie_secure(base, true)
    let a = unwrap(cookie_same_site(secured, "Strict"))
    let b = unwrap(cookie_path(a, "/app"))
    let c = unwrap(cookie_domain(b, "example.com"))
    let d = unwrap(cookie_max_age(c, 3600))
    let e = unwrap(cookie_expires(d, Instant(1445412480000000)))
    let line = unwrap(cookie_serialize(e))
    print(line)
    let back = unwrap(cookie_parse_set(line))
    assert(back == e)
    assert(back.http_only)
    assert(back.secure)
    assert(back.same_site == "Strict")
    assert(back.path == "/app")
    assert(back.domain == "example.com")
    assert(unwrap(back.max_age) == 3600)
    assert(unwrap(back.expires).unix_microseconds == 1445412480000000)
    # The Cookie request header form reads the name and decoded value back.
    let jar = cookie_parse("sid=abc%20123; theme=dark")
    assert(jar["sid"] == "abc 123")
    assert(jar["theme"] == "dark")
    # Defaults: HttpOnly and SameSite=Lax, Secure not forced.
    let plain = unwrap(cookie_serialize(base))
    print(plain)
    # A response carries one Set-Cookie line per cookie.
    let r = unwrap(response_add_cookie(response_text(200, "ok"), e))
    let r2 = unwrap(response_add_cookie(r, base))
    assert(len(response_header_values(r2, "Set-Cookie")) == 2)
    return 0
"##,
    );
    assert!(
        stdout.contains(
            "sid=abc%20123; Path=/app; Domain=example.com; Max-Age=3600; Expires=Wed, 21 Oct 2015 07:28:00 GMT; HttpOnly; Secure; SameSite=Strict\n"
        ),
        "{stdout:?}"
    );
    assert!(
        stdout.contains("sid=abc%20123; HttpOnly; SameSite=Lax\n"),
        "{stdout:?}"
    );
}

#[test]
fn a_value_with_syntax_characters_is_encoded_not_injected() {
    let stdout = run_program(
        "cookie-inject",
        r##"import "std:cookie"
import "std:http_response"
import "std:time"
import "std:types"
import "std:url"
fn none_int() -> Option[Int]:
    return None
fn some_int(value: Int) -> Option[Int]:
    return Some(value)
fn none_instant() -> Option[Instant]:
    return None
fn main() -> Int:
    let nasty = "a;b,c\"d\r\nSet-Cookie: evil=1; Domain=evil.com"
    let c = unwrap(cookie_new("sid", nasty))
    let line = unwrap(cookie_serialize(c))
    print(line)
    let r = unwrap(response_add_cookie(response_text(200, "x"), c))
    assert(len(response_header_values(r, "Set-Cookie")) == 1)
    let wire = text_from_bytes_lossy(unwrap(response_serialize(r)))
    assert(not (text_find(wire, "\r\nSet-Cookie: evil", 0) >= 0))
    # Round trip: the value comes back exactly.
    assert(unwrap(cookie_parse_set(line)).value == nasty)
    assert(cookie_parse("sid=" + url_encode_component(nasty))["sid"] == nasty)
    # A record literal skips the constructors; serialization checks again.
    let forged = Cookie("a b", "v", true, false, "Lax", "", "", none_int(), none_instant())
    assert(not is_ok(cookie_serialize(forged)))
    let forged_path = Cookie("a", "v", true, false, "Lax", "/x\r\nSet-Cookie: e=1", "", none_int(), none_instant())
    assert(not is_ok(cookie_serialize(forged_path)))
    let forged_domain = Cookie("a", "v", true, false, "Lax", "", "x.com; Secure", none_int(), none_instant())
    assert(not is_ok(cookie_serialize(forged_domain)))
    let forged_site = Cookie("a", "v", true, false, "Lax\r\nX: 1", "", "", none_int(), none_instant())
    assert(not is_ok(cookie_serialize(forged_site)))
    let forged_age = Cookie("a", "v", true, false, "Lax", "", "", some_int(-1), none_instant())
    assert(not is_ok(cookie_serialize(forged_age)))
    return 0
"##,
    );
    assert!(stdout.starts_with("sid=a%3Bb%2Cc%22d%0D%0A"), "{stdout:?}");
    assert!(!stdout.contains("Domain=evil"), "{stdout:?}");
    assert_eq!(stdout.lines().count(), 1, "{stdout:?}");
}

#[test]
fn samesite_none_without_secure_is_a_typed_error() {
    let stdout = run_program(
        "cookie-none",
        r##"import "std:cookie"
import "std:http_response"
import "std:time"
import "std:types"
fn none_int() -> Option[Int]:
    return None
fn some_int(value: Int) -> Option[Int]:
    return Some(value)
fn none_instant() -> Option[Instant]:
    return None
fn main() -> Int:
    let base = unwrap(cookie_new("sid", "1"))
    let refused = cookie_same_site(base, "None")
    assert(not is_ok(refused))
    print(error(refused).code)
    print(error(refused).message)
    # Setting Secure first is accepted and serializes with both.
    let ok = unwrap(cookie_same_site(cookie_secure(base, true), "None"))
    print(unwrap(cookie_serialize(ok)))
    # Turning Secure off afterwards, or a literal, is caught at serialization.
    let off = cookie_secure(ok, false)
    let again = cookie_serialize(off)
    assert(not is_ok(again))
    print(error(again).code)
    let literal = Cookie("sid", "1", true, false, "None", "", "", none_int(), none_instant())
    assert(not is_ok(cookie_serialize(literal)))
    let bad = cookie_same_site(base, "Sometimes")
    assert(not is_ok(bad))
    print(error(bad).code)
    return 0
"##,
    );
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines[0], "E_COOKIE_SAMESITE_NONE_NEEDS_SECURE", "{lines:?}");
    assert!(
        lines[1].contains("COOKIE_SAMESITE_NONE_NEEDS_SECURE"),
        "{lines:?}"
    );
    assert_eq!(
        lines[2], "sid=1; HttpOnly; Secure; SameSite=None",
        "{lines:?}"
    );
    assert_eq!(lines[3], "E_COOKIE_SAMESITE_NONE_NEEDS_SECURE", "{lines:?}");
    assert_eq!(lines[4], "E_COOKIE_SAMESITE_INVALID", "{lines:?}");
}

#[test]
fn bad_names_and_attributes_are_refused() {
    let stdout = run_program(
        "cookie-refuse",
        r##"import "std:cookie"
import "std:http_response"
import "std:time"
import "std:types"
fn main() -> Int:
    assert(not is_ok(cookie_new("a b", "v")))
    assert(not is_ok(cookie_new("a=b", "v")))
    assert(not is_ok(cookie_new("a;b", "v")))
    assert(not is_ok(cookie_new("a,b", "v")))
    assert(not is_ok(cookie_new("", "v")))
    assert(not is_ok(cookie_new("a\r\nb", "v")))
    assert(is_ok(cookie_new("good-Name_1", "")))
    let base = unwrap(cookie_new("sid", "1"))
    let negative = cookie_max_age(base, -1)
    assert(not is_ok(negative))
    print(error(negative).code)
    assert(is_ok(cookie_max_age(base, 0)))
    assert(not is_ok(cookie_path(base, "no-slash")))
    assert(not is_ok(cookie_path(base, "/a;b")))
    assert(not is_ok(cookie_path(base, "/a b")))
    assert(not is_ok(cookie_domain(base, "a.com\r\nX: 1")))
    assert(not is_ok(cookie_domain(base, "a.com; Secure")))
    assert(is_ok(cookie_domain(base, ".example.com")))
    assert(not is_ok(cookie_expires(base, Instant(1000000000000000000))))
    assert(not is_ok(cookie_parse_set("noequals; Secure")))
    assert(not is_ok(cookie_parse_set("a=1; Max-Age=-5")))
    assert(not is_ok(cookie_parse_set("a=1; Max-Age=soon")))
    assert(not is_ok(cookie_parse_set("a=1; Expires=tomorrow")))
    assert(not is_ok(cookie_parse_set("a=1; SameSite=None")))
    assert(is_ok(cookie_parse_set("a=1; samesite=none; SECURE")))
    return 0
"##,
    );
    assert_eq!(stdout.trim(), "E_COOKIE_MAX_AGE_INVALID");
}

#[test]
fn cookie_parse_drops_malformed_pairs_and_never_fails() {
    let stdout = run_program(
        "cookie-parse",
        r##"import "std:cookie"
fn main() -> Int:
    let jar = cookie_parse("a=1; ; novalue; =x; b c=2; d=%zz; e=\"q%20r\"; f=; a=second;g=3")
    assert(len(jar) == 4)
    assert(jar["a"] == "1")
    assert(jar["e"] == "q r")
    assert(jar["f"] == "")
    assert(jar["g"] == "3")
    assert(len(cookie_parse("")) == 0)
    assert(len(cookie_parse(";;;===;;")) == 0)
    assert(len(cookie_parse("a=b=c")) == 1)
    assert(cookie_parse("a=b=c")["a"] == "b=c")
    assert(len(cookie_parse(text_from_bytes_lossy(bytes_from_ints([0, 255, 254, 13, 10])))) == 0)
    print("done")
    return 0
"##,
    );
    assert_eq!(stdout.trim(), "done");
}

#[test]
fn a_hundred_kilobytes_of_junk_returns_quickly() {
    let junk_pairs = "a=b;".repeat(25_000);
    let junk_noeq = "x".repeat(100_000);
    let junk_semis = ";".repeat(100_000);
    let junk_eq = "=".repeat(100_000);
    let source = format!(
        r##"import "std:cookie"
fn main() -> Int:
    let a = cookie_parse("{junk_pairs}")
    let b = cookie_parse("{junk_noeq}")
    let c = cookie_parse("{junk_semis}")
    let d = cookie_parse("{junk_eq}")
    print(int_to_text(len(a) + len(b) + len(c) + len(d)))
    return 0
"##
    );
    let started = Instant::now();
    let stdout = run_program("cookie-junk", &source);
    assert_eq!(stdout.trim(), "1");
    assert!(
        started.elapsed().as_secs() < 20,
        "took {:?}",
        started.elapsed()
    );
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
    let report = doc_json("std:cookie");
    let functions = report["functions"].as_array().unwrap();
    assert!(functions.len() >= 11, "{functions:?}");
    for function in functions {
        let text = function.to_string();
        assert!(!HANDLE_TYPES.iter().any(|h| text.contains(h)), "{text}");
        // Every function declares its effects, and none touches anything outside memory.
        let effects = function["effects"].as_array().expect("effects array");
        assert!(effects.is_empty(), "{text}");
    }
    for record in report["records"].as_array().unwrap() {
        let text = record.to_string();
        assert!(!HANDLE_TYPES.iter().any(|h| text.contains(h)), "{text}");
    }
    let names: Vec<&str> = functions
        .iter()
        .map(|function| function["name"].as_str().unwrap())
        .collect();
    for wanted in [
        "cookie_parse",
        "cookie_new",
        "cookie_http_only",
        "cookie_secure",
        "cookie_same_site",
        "cookie_path",
        "cookie_domain",
        "cookie_max_age",
        "cookie_expires",
        "cookie_serialize",
        "cookie_parse_set",
        "response_add_cookie",
    ] {
        assert!(names.contains(&wanted), "{wanted} missing from {names:?}");
    }
}
