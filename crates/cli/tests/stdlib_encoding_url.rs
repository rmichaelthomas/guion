use std::process::Command;

fn run_source(name: &str, source: &str) -> std::process::Output {
    let dir = std::env::temp_dir().join(format!("guion-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("main.gu"), source).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", "main.gu"])
        .current_dir(&dir)
        .output()
        .expect("guion starts");
    let _ = std::fs::remove_dir_all(&dir);
    output
}

// Expected values below were produced with CPython's base64, binascii and urllib.parse.
#[test]
fn encodings_match_cpython_known_answers_and_reject_invalid_input() {
    let source = r#"import "std:encoding"
fn check(data: Bytes, standard: Text, urlsafe: Text, hex: Text) -> Unit:
    assert(base64_encode(data) == standard)
    assert(base64url_encode(data) == urlsafe)
    assert(hex_encode(data) == hex)
    assert(unwrap(base64_decode(standard)) == data)
    assert(unwrap(base64url_decode(urlsafe)) == data)
    assert(unwrap(hex_decode(hex)) == data)
fn main() -> Int:
    check(bytes_from_text(""), "", "", "")
    check(bytes_from_text("f"), "Zg==", "Zg==", "66")
    check(bytes_from_text("fo"), "Zm8=", "Zm8=", "666f")
    check(bytes_from_text("foobar"), "Zm9vYmFy", "Zm9vYmFy", "666f6f626172")
    check(bytes_from_ints([251, 255, 254]), "+//+", "-__-", "fbfffe")
    check(bytes_from_text("héllo wörld 😀"), "aMOpbGxvIHfDtnJsZCDwn5iA", "aMOpbGxvIHfDtnJsZCDwn5iA", "68c3a96c6c6f2077c3b6726c6420f09f9880")
    assert(not is_ok(base64_decode("abc")))
    assert(not is_ok(base64_decode("ab=c")))
    assert(not is_ok(base64_decode("a b=")))
    assert(not is_ok(base64_decode("-__-")))
    assert(not is_ok(base64url_decode("+//+")))
    assert(not is_ok(hex_decode("abc")))
    assert(not is_ok(hex_decode("zz")))
    assert(hex_decode("ABff") == hex_decode("abFF"))
    let data = bytes_from_ints([0, 1, 2, 255])
    assert(bytes_to_ints(data) == [0, 1, 2, 255])
    assert(bytes_slice(data, 1, 3) == bytes_from_ints([1, 2]))
    assert(len(bytes_slice(data, 2, 2)) == 0)
    assert(is_ok(encoding_utf8_decode(bytes_from_text("ok ü"))))
    assert(not is_ok(encoding_utf8_decode(bytes_from_ints([255, 254]))))
    assert(encoding_utf8_decode_lossy(bytes_from_ints([104, 255, 105])) == "h\u{FFFD}i")
    return 0
"#;
    let output = run_source("encoding", source);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn bytes_slice_out_of_range_reports_a_stable_code() {
    let output = run_source(
        "bytes-slice",
        "fn main() -> Int:\n    print(bytes_slice(bytes_from_ints([1, 2]), 1, 5))\n    return 0\n",
    );
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("R_INDEX_BOUNDS"));
}

#[test]
fn url_parsing_building_and_percent_encoding_follow_rfc_3986() {
    let source = r#"import "std:url"
fn main() -> Int:
    assert(url_encode_component("a b&c/ü") == "a%20b%26c%2F%C3%BC")
    assert(url_encode_component("~-._ok") == "~-._ok")
    assert(url_encode_component("100%") == "100%25")
    assert(url_encode_component("😀") == "%F0%9F%98%80")
    assert(unwrap(url_decode_component("a%20b%26c%2F%C3%BC")) == "a b&c/ü")
    assert(not is_ok(url_decode_component("100%")))
    assert(not is_ok(url_decode_component("%zz")))
    assert(not is_ok(url_decode_component("%FF")))
    let full = unwrap(url_parse("https://user:pw@example.com:8080/a/b?x=1&y=a%20b+c#frag"))
    assert(full.scheme == "https")
    assert(full.userinfo == "user:pw")
    assert(full.host == "example.com")
    assert(full.port == 8080)
    assert(full.path == "/a/b")
    assert(full.query == "x=1&y=a%20b+c")
    assert(full.fragment == "frag")
    assert(url_to_text(full) == "https://user:pw@example.com:8080/a/b?x=1&y=a%20b+c#frag")
    let bare = unwrap(url_parse("http://example.com"))
    assert(bare.port == -1)
    assert(bare.path == "")
    let mail = unwrap(url_parse("mailto:rob@example.com"))
    assert(mail.scheme == "mailto")
    assert(mail.host == "")
    assert(mail.path == "rob@example.com")
    let relative = unwrap(url_parse("/just/a/path?q=1"))
    assert(relative.scheme == "")
    assert(relative.path == "/just/a/path")
    let v6 = unwrap(url_parse("http://[::1]:3000/x"))
    assert(v6.host == "[::1]")
    assert(v6.port == 3000)
    assert(not is_ok(url_parse("http://h:99999/")))
    assert(not is_ok(url_parse("http://h:abc/")))
    assert(not is_ok(url_parse("http://[::1/x")))
    let pairs = unwrap(url_query_parse("x=1&y=a%20b+c&flag&=v"))
    assert(len(pairs) == 4)
    assert(pairs[1].value == "a b c")
    assert(pairs[2].name == "flag")
    assert(pairs[2].value == "")
    assert(url_query_build(pairs) == "x=1&y=a%20b%20c&flag=&=v")
    assert(len(unwrap(url_query_parse(""))) == 0)
    return 0
"#;
    let output = run_source("url", source);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn codec_instructions_are_rejected_in_artifact_format_52() {
    let dir = std::env::temp_dir().join(format!("guion-codec-format-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("main.gu"),
        "fn main() -> Int:\n    print(len(bytes_to_ints(bytes_from_text(\"ab\"))))\n    return 0\n",
    )
    .unwrap();
    let built = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["build", "main.gu", "--output", "main.gub"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
    let mut bytes = std::fs::read(dir.join("main.gub")).unwrap();
    bytes[4..8].copy_from_slice(&52u32.to_le_bytes());
    std::fs::write(dir.join("old.gub"), bytes).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", "old.gub"])
        .current_dir(&dir)
        .output()
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("E_ARTIFACT_VERSION"));
}
