mod common;

use common::{connect_to, free_port, output, spawn};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

fn project(name: &str, source: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("guion-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("main.gu"), source).unwrap();
    dir
}

/// Runs a program that needs no grants and returns its stdout, failing on a nonzero exit.
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

fn spawn_guion(dir: &Path, flags: &[&str]) -> Child {
    spawn(
        Command::new(env!("CARGO_BIN_EXE_guion"))
            .arg("run")
            .args(flags)
            .arg("main.gu")
            .current_dir(dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    )
    .expect("guion starts")
}

fn connect(port: u16) -> TcpStream {
    for _ in 0..100 {
        if let Ok(stream) = connect_to(port) {
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            return stream;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("server never listened on {port}");
}

fn exchange(port: u16, request: &[u8]) -> Vec<u8> {
    let mut stream = connect(port);
    stream.write_all(request).unwrap();
    let mut response = Vec::new();
    let _ = stream.read_to_end(&mut response);
    response
}

fn split_head(response: &[u8]) -> (String, Vec<u8>) {
    let at = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("a blank line ends the head");
    (
        String::from_utf8_lossy(&response[..at]).into_owned(),
        response[at + 4..].to_vec(),
    )
}

#[test]
fn three_set_cookie_headers_serialize_as_three_lines() {
    let stdout = run_program(
        "resp-cookies",
        r##"import "std:http_response"
import "std:types"
fn build() -> Response:
    let first = response_text(200, "hi")
    let second = unwrap(response_add_header(first, "Set-Cookie", "a=1; Path=/"))
    let third = unwrap(response_add_header(second, "Set-Cookie", "b=2; HttpOnly"))
    return unwrap(response_add_header(third, "Set-Cookie", "c=3; Secure"))
fn main() -> Int:
    let wire = unwrap(response_serialize(build()))
    print(text_from_bytes_lossy(wire))
    return 0
"##,
    );
    assert!(stdout.starts_with("HTTP/1.1 200 OK\r\n"), "{stdout:?}");
    assert!(
        stdout.contains(
            "Set-Cookie: a=1; Path=/\r\nSet-Cookie: b=2; HttpOnly\r\nSet-Cookie: c=3; Secure\r\n"
        ),
        "{stdout:?}"
    );
    assert_eq!(stdout.matches("Set-Cookie:").count(), 3, "{stdout:?}");
    assert!(stdout.contains("Content-Length: 2\r\n"), "{stdout:?}");
    assert!(stdout.contains("Content-Type: text/plain; charset=utf-8\r\n"));
    assert!(stdout.contains("Connection: close\r\n\r\nhi"), "{stdout:?}");
}

#[test]
fn a_response_returned_through_wrappers_equals_the_original() {
    let stdout = run_program(
        "resp-wrappers",
        r##"import "std:http_response"
import "std:types"
fn build() -> Response:
    let base = response_text(201, "made")
    let one = unwrap(response_add_header(base, "X-One", "1"))
    return unwrap(response_add_header(one, "Set-Cookie", "k=v"))
fn wrap_a(r: Response) -> Response:
    return r
fn wrap_b(r: Response) -> Response:
    return wrap_a(r)
fn main() -> Int:
    let original = build()
    let passed = wrap_b(wrap_a(original))
    assert(passed == original)
    assert(passed == build())
    assert(unwrap(response_header(passed, "x-one")) == "1")
    let changed = unwrap(response_set_header(passed, "X-ONE", "2"))
    assert(changed != original)
    assert(unwrap(response_header(original, "X-One")) == "1")
    assert(not is_some(response_header(original, "X-Two")))
    assert(len(changed.headers) == 2)
    print("equal")
    return 0
"##,
    );
    assert!(stdout.contains("equal"), "{stdout}");
}

#[test]
fn a_bytes_body_with_nul_and_invalid_utf8_round_trips() {
    let stdout = run_program(
        "resp-bytes",
        r##"import "std:http_response"
import "std:types"
fn main() -> Int:
    let payload = bytes_from_ints([0, 255, 254, 65, 0, 128, 10, 13, 0])
    let r = response_bytes(200, payload)
    assert(r.body.data == payload)
    let wire = unwrap(response_serialize(r))
    let tail = bytes_slice(wire, len(wire) - 9, len(wire))
    assert(tail == payload)
    let head = text_from_bytes_lossy(bytes_slice(wire, 0, len(wire) - 9))
    print(head)
    return 0
"##,
    );
    assert!(stdout.contains("Content-Length: 9\r\n"), "{stdout:?}");
    assert!(
        stdout.contains("Content-Type: application/octet-stream\r\n"),
        "{stdout:?}"
    );
}

#[test]
fn a_header_with_a_line_break_is_refused_at_construction() {
    let stdout = run_program(
        "resp-split",
        r##"import "std:http_response"
import "std:types"
fn refused(r: Result[Response, HttpFailure]) -> Bool:
    return not is_ok(r)
fn main() -> Int:
    let base = response_text(200, "x")
    assert(refused(response_add_header(base, "X-A", "ok\r\nSet-Cookie: evil=1")))
    assert(refused(response_add_header(base, "X-A", "ok\nmore")))
    assert(refused(response_add_header(base, "X-A", "ok\rmore")))
    assert(refused(response_set_header(base, "X-A", "a\r\nb")))
    assert(refused(response_add_header(base, "Set-Cookie", "sid=1\r\nX-Injected: 1")))
    assert(refused(response_add_header(base, "X-A\r\nB", "v")))
    assert(refused(response_add_header(base, "X A", "v")))
    assert(refused(response_add_header(base, "", "v")))
    assert(refused(response_add_header(base, "X-A", "nul" + text_from_bytes_lossy(bytes_from_ints([0])))))
    assert(refused(response_redirect(302, "/next\r\nSet-Cookie: x=1")))
    assert(is_ok(response_redirect(302, "/next")))
    assert(is_ok(response_add_header(base, "X-A", "tab\tis fine")))
    let bad = response_add_header(base, "X-A", "a\r\nb")
    print(error(bad).code)
    # A response written as a record literal skips the constructors, so serialization checks again.
    let forged = Response(200, [Header("X-A", "a\r\nSet-Cookie: evil=1")], body_text("x"))
    assert(not is_ok(response_serialize(forged)))
    let forged_name = Response(200, [Header("Bad Name", "v")], body_text("x"))
    assert(not is_ok(response_serialize(forged_name)))
    let forged_status = Response(99, [], body_text("x"))
    assert(not is_ok(response_serialize(forged_status)))
    print("refused")
    return 0
"##,
    );
    assert!(stdout.contains("E_HTTP_HEADER_INVALID"), "{stdout}");
    assert!(stdout.contains("refused"), "{stdout}");
}

#[test]
fn a_handler_runs_with_no_socket_from_a_request_literal() {
    let stdout = run_program(
        "resp-handler",
        r##"import "std:http_response"
import "std:types"
fn greet(request: Request) -> Result[Response, HttpFailure]:
    var name = "world"
    match map_get(request.params, "name"):
        Some(found):
            set name = found
        None:
            let nothing = 0
    return Ok(response_text(200, "hello " + name + " via " + request.method))
fn call(handler: fn(Request) -> Result[Response, HttpFailure], request: Request) -> Result[Response, HttpFailure]:
    return handler(request)
fn main() -> Int:
    let query: Map[Text, List[Text]] = {}
    let params: Map[Text, Text] = {"name": "ada"}
    let request = Request("GET", "/hi/ada", query, [Header("accept", "text/plain")], bytes_from_ints([]), params, "")
    let result = call(greet, request)
    let response = unwrap(result)
    assert(response.status == 200)
    print(text_from_bytes_lossy(response.body.data))
    assert(unwrap(request_header(request, "Accept")) == "text/plain")
    let plain = request_new("POST", "/x")
    assert(len(plain.params) == 0)
    assert(plain.method == "POST")
    return 0
"##,
    );
    assert!(stdout.contains("hello ada via GET"), "{stdout}");
}

const SERVER: &str = r##"import "std:http_server"
import "std:http_response"
import "std:net"
import "std:text"
import "std:types"
fn handle(request: Request) -> Result[Response, HttpFailure]:
    if request.path == "/value":
        var seen = request.method + "|"
        match map_get(request.query, "a"):
            Some(values):
                set seen = seen + text_join(values, ",")
            None:
                set seen = seen + "-"
        match map_get(request.query, "b"):
            Some(values):
                set seen = seen + "|" + text_join(values, ",")
            None:
                set seen = seen + "|-"
        match request_header(request, "x-test"):
            Some(found):
                set seen = seen + "|" + found
            None:
                set seen = seen + "|-"
        set seen = seen + "|" + int_to_text(len(request.body)) + "|" + int_to_text(len(request.params))
        let base = response_text(200, seen)
        let one = response_add_header(base, "Set-Cookie", "a=1")
        let two = response_add_header(unwrap(one), "Set-Cookie", "b=2")
        return two
    if request.path == "/list":
        let chunks = [bytes_from_text("hello"), bytes_from_text(" "), bytes_from_text("world")]
        let base = response_new(200, body_stream(stream_from_list(chunks)))
        return response_add_header(base, "X-Mode", "list")
    if request.path == "/file":
        return Ok(response_new(200, body_stream(stream_from_file("big.bin"))))
    if request.path == "/gone":
        return Ok(response_text(404, "gone"))
    return Err(HttpFailure(500, "E_TEST", "no route"))
fn serve_one(handler: fn(Request) -> Result[Response, HttpFailure], raw: HttpServerRequest) -> Unit:
    match http_request_value(raw):
        Ok(request):
            match handler(request):
                Ok(response):
                    let sent = http_server_send_response(raw, response)
                    if not is_ok(sent):
                        print("send " + error(sent).code)
                Err(problem):
                    let sent = http_server_respond_text(raw, problem.status, problem.message)
        Err(problem):
            print("value " + problem.code)
fn main() -> Int:
    let listener = unwrap(http_server_listen("127.0.0.1", PORT))
    var served = 0
    while served < 4:
        match http_server_accept(listener, 20000):
            Ok(conn):
                match http_server_read_request(conn, 3000, 100000):
                    Ok(raw):
                        serve_one(handle, raw)
                    Err(problem):
                        print(problem.code)
            Err(problem):
                print("accept " + problem.code)
        set served = served + 1
    return 0
"##;

fn decode_chunked(body: &[u8]) -> (Vec<usize>, Vec<u8>) {
    let mut sizes = Vec::new();
    let mut data = Vec::new();
    let mut rest = body;
    loop {
        let at = rest
            .windows(2)
            .position(|window| window == b"\r\n")
            .expect("chunk size line");
        let size = usize::from_str_radix(std::str::from_utf8(&rest[..at]).unwrap(), 16).unwrap();
        rest = &rest[at + 2..];
        sizes.push(size);
        if size == 0 {
            assert_eq!(rest, b"\r\n", "the stream ends with a bare CRLF");
            return (sizes, data);
        }
        data.extend_from_slice(&rest[..size]);
        assert_eq!(&rest[size..size + 2], b"\r\n", "each chunk ends with CRLF");
        rest = &rest[size + 2..];
    }
}

#[test]
fn a_server_sends_values_and_chunked_streams_over_a_real_socket() {
    let port = free_port();
    let dir = project("resp-server", &SERVER.replace("PORT", &port.to_string()));
    let big: Vec<u8> = (0..40_000u32).map(|n| (n % 251) as u8).collect();
    std::fs::write(dir.join("big.bin"), &big).unwrap();
    let root = dir.to_string_lossy().into_owned();
    let server = spawn_guion(
        &dir,
        &["--allow-bind", "127.0.0.1:*", "--file-root", root.as_str()],
    );

    let value = exchange(
        port,
        b"POST /value?a=1&a=2&b=x%20y HTTP/1.1\r\nHost: t\r\nX-Test: 7\r\nContent-Length: 3\r\n\r\nabc",
    );
    let (head, body) = split_head(&value);
    assert!(head.starts_with("HTTP/1.1 200 OK"), "{head}");
    assert_eq!(head.matches("Set-Cookie:").count(), 2, "{head}");
    assert!(
        head.contains("Set-Cookie: a=1\r\nSet-Cookie: b=2"),
        "{head}"
    );
    assert_eq!(String::from_utf8_lossy(&body), "POST|1,2|x y|7|3|0");

    let list = exchange(port, b"GET /list HTTP/1.1\r\nHost: t\r\n\r\n");
    let (head, body) = split_head(&list);
    assert!(head.contains("Transfer-Encoding: chunked"), "{head}");
    assert!(!head.contains("Content-Length"), "{head}");
    assert!(head.contains("X-Mode: list"), "{head}");
    let (sizes, data) = decode_chunked(&body);
    assert_eq!(sizes, vec![5, 1, 5, 0]);
    assert_eq!(data, b"hello world");

    let file = exchange(port, b"GET /file HTTP/1.1\r\nHost: t\r\n\r\n");
    let (head, body) = split_head(&file);
    assert!(head.contains("Transfer-Encoding: chunked"), "{head}");
    let (sizes, data) = decode_chunked(&body);
    assert_eq!(sizes, vec![16384, 16384, 7232, 0]);
    assert_eq!(data, big);

    let gone = exchange(port, b"GET /gone HTTP/1.1\r\nHost: t\r\n\r\n");
    let (head, body) = split_head(&gone);
    assert!(head.starts_with("HTTP/1.1 404 Not Found"), "{head}");
    assert_eq!(body, b"gone");

    let result = server.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert!(
        result.status.success(),
        "{stdout}{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(stdout.trim().is_empty(), "{stdout}");
    let _ = std::fs::remove_dir_all(&dir);
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

const HANDLE_TYPES: &[&str] = &[
    "TcpConn",
    "TcpListener",
    "UdpSock",
    "FileReader",
    "Db",
    "Transaction",
    "TomlDocument",
];

fn mentions_handle(text: &str) -> bool {
    HANDLE_TYPES.iter().any(|handle| text.contains(handle))
}

#[test]
fn no_handle_in_public_signatures() {
    // The new module: no function parameter or result and no record field names a handle.
    let report = doc_json("std:http_response");
    let functions = report["functions"].as_array().unwrap();
    assert!(functions.len() >= 15, "{functions:?}");
    for function in functions {
        let name = function["name"].as_str().unwrap();
        assert!(
            !mentions_handle(&function.to_string()),
            "{name}: {function}"
        );
    }
    for record in report["records"].as_array().unwrap() {
        assert!(!mentions_handle(&record.to_string()), "{record}");
    }
    // The two functions added to std:http_server cross the connection-bound request, never a handle.
    let server = doc_json("std:http_server");
    for name in ["http_request_value", "http_server_send_response"] {
        let function = server["functions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|function| function["name"] == name)
            .unwrap_or_else(|| panic!("{name} is documented"));
        assert!(
            !mentions_handle(&function.to_string()),
            "{name}: {function}"
        );
    }
}

#[test]
fn every_new_function_declares_its_effects() {
    let report = doc_json("std:http_response");
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
    // Values built in memory are pure; reading a streamed file shows up where the file is read.
    assert!(effects("response_text").is_empty());
    assert!(effects("response_add_header").is_empty());
    assert!(effects("stream_from_file").is_empty());
    assert!(effects("response_serialize").contains(&"filesystem.read".to_owned()));
    let server = doc_json("std:http_server");
    let send = server["functions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|function| function["name"] == "http_server_send_response")
        .unwrap();
    // The send paths that existed before keep their effect set: no file effect reaches them.
    for old in [
        "http_server_respond",
        "http_server_reply",
        "http_server_send",
    ] {
        let function = server["functions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|function| function["name"] == old)
            .unwrap();
        assert_eq!(
            function["effects"],
            serde_json::json!(["network.socket"]),
            "{old}"
        );
    }
    assert!(effects("response_wire_memory").is_empty());
    let send_effects = send["effects"].to_string();
    assert!(send_effects.contains("network.socket"), "{send_effects}");
    assert!(send_effects.contains("filesystem.read"), "{send_effects}");
}
