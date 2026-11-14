mod common;

use common::{connect_to, free_port, output, spawn};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// A fixed modification time: Tue, 15 Nov 1994 08:12:31 GMT.
const MTIME_SECONDS: u64 = 784_887_151;
const LAST_MODIFIED: &str = "Tue, 15 Nov 1994 08:12:31 GMT";

fn data_bytes(length: usize) -> Vec<u8> {
    (0..length).map(|n| b'a' + (n % 26) as u8).collect()
}

fn set_mtime(path: &Path) {
    let file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
    file.set_modified(UNIX_EPOCH + Duration::from_secs(MTIME_SECONDS))
        .unwrap();
}

fn tree(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("guion-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let public = dir.join("public");
    std::fs::create_dir_all(public.join("sub")).unwrap();
    std::fs::create_dir_all(public.join("empty")).unwrap();
    std::fs::write(dir.join("secret.txt"), "TOP-SECRET-OUTSIDE").unwrap();
    std::fs::write(public.join("hello.txt"), "Hello, static!\n").unwrap();
    std::fs::write(public.join("data.bin"), data_bytes(1000)).unwrap();
    std::fs::write(public.join("index.html"), "<h1>root index</h1>").unwrap();
    std::fs::write(public.join("sub/index.html"), "<h1>sub index</h1>").unwrap();
    std::fs::write(public.join(".hidden"), "dotfile").unwrap();
    for (file, body) in [
        ("a.css", "body{}"),
        ("a.js", "1"),
        ("a.json", "{}"),
        ("a.png", "png"),
        ("a.svg", "<svg/>"),
        ("a.woff2", "w"),
        ("a.wasm", "w"),
        ("a.zzz", "z"),
        ("noext", "n"),
        ("UP.HTML", "<p/>"),
    ] {
        std::fs::write(public.join(file), body).unwrap();
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("../secret.txt", public.join("link.txt")).unwrap();
        std::os::unix::fs::symlink("../", public.join("updir")).unwrap();
        std::os::unix::fs::symlink("hello.txt", public.join("inside.txt")).unwrap();
    }
    for file in ["hello.txt", "data.bin"] {
        set_mtime(&public.join(file));
    }
    dir
}

struct Reply {
    status: String,
    head: String,
    body: Vec<u8>,
}

impl Reply {
    fn header(&self, name: &str) -> Option<String> {
        let wanted = format!("{}:", name.to_ascii_lowercase());
        self.head.split("\r\n").skip(1).find_map(|line| {
            if line.to_ascii_lowercase().starts_with(&wanted) {
                Some(line[wanted.len()..].trim().to_owned())
            } else {
                None
            }
        })
    }
    fn code(&self) -> u32 {
        self.status.split(' ').nth(1).unwrap().parse().unwrap()
    }
}

fn guion_text(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

struct Case {
    method: &'static str,
    path: String,
    headers: Vec<(&'static str, String)>,
}

fn case(method: &'static str, path: &str, headers: &[(&'static str, &str)]) -> Case {
    Case {
        method,
        path: path.to_owned(),
        headers: headers
            .iter()
            .map(|(name, value)| (*name, (*value).to_owned()))
            .collect(),
    }
}

fn get(path: &str) -> Case {
    case("GET", path, &[])
}

fn range(spec: &str) -> Case {
    case("GET", "/data.bin", &[("range", spec)])
}

/// Runs `static_serve` in a program with a file grant and returns every response as serialized.
fn serve(dir: &Path, cases: &[Case]) -> Vec<Reply> {
    let mut source = String::from(
        r##"import "std:http_static"
import "std:http_response"
import "std:encoding"
import "std:types"
fn req(method: Text, path: Text, headers: List[Header]) -> Request:
    let base = request_new(method, path)
    return Request(base.method, base.path, base.query, headers, base.body, base.params)
fn show(r: Result[Response, HttpFailure]) -> Unit:
    match r:
        Ok(response):
            match response_serialize(response):
                Ok(wire):
                    print(encoding_utf8_decode_lossy(wire))
                Err(f):
                    print("serialize " + f.code)
        Err(f):
            print("ERR " + f.code)
    print("=====")
fn main() -> Int:
"##,
    );
    for case in cases {
        let headers: Vec<String> = case
            .headers
            .iter()
            .map(|(name, value)| format!("Header(\"{}\", \"{}\")", name, guion_text(value)))
            .collect();
        source.push_str(&format!(
            "    show(static_serve(req(\"{}\", \"{}\", [{}]), \"public\"))\n",
            case.method,
            guion_text(&case.path),
            headers.join(", ")
        ));
    }
    source.push_str("    return 0\n");
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
    let mut replies = Vec::new();
    for segment in stdout.split("\n=====\n") {
        if segment.trim().is_empty() {
            continue;
        }
        let at = segment
            .find("\r\n\r\n")
            .unwrap_or_else(|| panic!("no head in {segment:?}"));
        let head = segment[..at].to_owned();
        replies.push(Reply {
            status: head.lines().next().unwrap().to_owned(),
            head,
            body: segment.as_bytes()[at + 4..].to_vec(),
        });
    }
    assert_eq!(replies.len(), cases.len(), "{stdout}");
    replies
}

fn etag(size: usize) -> String {
    format!("W/\"{size}-{}\"", MTIME_SECONDS * 1000)
}

#[test]
fn a_file_is_served_with_type_validators_and_no_listing_header_leaks() {
    let dir = tree("static-basic");
    let replies = serve(&dir, &[get("/hello.txt"), get("/data.bin")]);
    let hello = &replies[0];
    assert_eq!(hello.code(), 200, "{}", hello.head);
    assert_eq!(
        hello.header("content-type").unwrap(),
        "text/plain; charset=utf-8"
    );
    assert_eq!(hello.header("etag").unwrap(), etag(15));
    assert_eq!(hello.header("last-modified").unwrap(), LAST_MODIFIED);
    assert_eq!(hello.header("accept-ranges").unwrap(), "bytes");
    assert_eq!(hello.header("content-length").unwrap(), "15");
    assert_eq!(hello.body, b"Hello, static!\n");
    assert_eq!(replies[1].body, data_bytes(1000));
    assert_eq!(replies[1].header("content-length").unwrap(), "1000");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn conditional_requests_answer_304_with_no_body() {
    let dir = tree("static-conditional");
    let tag = etag(15);
    let list = format!("\"nope\", {tag}");
    let replies = serve(
        &dir,
        &[
            case("GET", "/hello.txt", &[("if-none-match", tag.as_str())]),
            case("GET", "/hello.txt", &[("if-none-match", list.as_str())]),
            case("GET", "/hello.txt", &[("if-none-match", "*")]),
            case("GET", "/hello.txt", &[("if-none-match", "W/\"1-2\"")]),
            case("GET", "/hello.txt", &[("if-modified-since", LAST_MODIFIED)]),
            case(
                "GET",
                "/hello.txt",
                &[("if-modified-since", "Tue, 15 Nov 1994 08:12:30 GMT")],
            ),
            case(
                "GET",
                "/hello.txt",
                &[("if-modified-since", "Wed, 16 Nov 2044 00:00:00 GMT")],
            ),
            case("GET", "/hello.txt", &[("if-modified-since", "garbage")]),
            // If-None-Match takes precedence: a mismatch is a 200 even when the date matches.
            case(
                "GET",
                "/hello.txt",
                &[
                    ("if-none-match", "W/\"1-2\""),
                    ("if-modified-since", LAST_MODIFIED),
                ],
            ),
            case("HEAD", "/hello.txt", &[("if-none-match", tag.as_str())]),
        ],
    );
    let codes: Vec<u32> = replies.iter().map(Reply::code).collect();
    assert_eq!(
        codes,
        vec![304, 304, 304, 200, 304, 200, 304, 200, 200, 304]
    );
    for index in [0, 1, 2, 4, 9] {
        assert!(replies[index].body.is_empty(), "case {index}");
        assert_eq!(replies[index].header("etag").unwrap(), tag);
        assert_eq!(
            replies[index].header("last-modified").unwrap(),
            LAST_MODIFIED
        );
        assert!(replies[index].header("content-length").is_none());
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn single_ranges_give_206_and_unsatisfiable_gives_416() {
    let dir = tree("static-range");
    let all = data_bytes(1000);
    let replies = serve(
        &dir,
        &[
            range("bytes=0-99"),
            range("bytes=-500"),
            range("bytes=0-0"),
            range("bytes=999999-"),
            range("bytes=990-5000"),
            range("bytes=-5000"),
            range("bytes=500-"),
            range("bytes=-0"),
            range("bytes=1000-"),
            range("bytes=999-999"),
        ],
    );
    assert_eq!(replies[0].code(), 206, "{}", replies[0].head);
    assert_eq!(
        replies[0].header("content-range").unwrap(),
        "bytes 0-99/1000"
    );
    assert_eq!(replies[0].header("content-length").unwrap(), "100");
    assert_eq!(replies[0].body, all[..100]);
    assert_eq!(replies[1].code(), 206);
    assert_eq!(
        replies[1].header("content-range").unwrap(),
        "bytes 500-999/1000"
    );
    assert_eq!(replies[1].body, all[500..]);
    assert_eq!(replies[2].code(), 206);
    assert_eq!(
        replies[2].header("content-range").unwrap(),
        "bytes 0-0/1000"
    );
    assert_eq!(replies[2].body, b"a");
    assert_eq!(replies[3].code(), 416);
    assert_eq!(replies[3].header("content-range").unwrap(), "bytes */1000");
    assert!(replies[3].body.is_empty());
    assert_eq!(
        replies[4].header("content-range").unwrap(),
        "bytes 990-999/1000"
    );
    assert_eq!(replies[4].body, all[990..]);
    assert_eq!(replies[5].code(), 206);
    assert_eq!(
        replies[5].header("content-range").unwrap(),
        "bytes 0-999/1000"
    );
    assert_eq!(replies[6].body, all[500..]);
    assert_eq!(replies[7].code(), 416);
    assert_eq!(replies[8].code(), 416);
    assert_eq!(replies[9].body, all[999..]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn multiple_ranges_and_unusable_range_headers_serve_the_whole_file() {
    // The documented choice: more than one range, an unknown unit, or a malformed range is
    // answered with the whole file and status 200 (RFC 9110 allows ignoring Range).
    let dir = tree("static-range-whole");
    let replies = serve(
        &dir,
        &[
            range("bytes=0-1,5-9"),
            range("items=0-1"),
            range("bytes=5-2"),
            range("bytes=abc"),
            range("bytes="),
            case(
                "GET",
                "/data.bin",
                &[("range", "bytes=0-9"), ("if-range", "\"other\"")],
            ),
            case(
                "GET",
                "/data.bin",
                &[("range", "bytes=0-9"), ("if-range", LAST_MODIFIED)],
            ),
        ],
    );
    for (index, reply) in replies.iter().enumerate().take(6) {
        assert_eq!(reply.code(), 200, "case {index}");
        assert_eq!(reply.body, data_bytes(1000), "case {index}");
        assert!(reply.header("content-range").is_none());
    }
    // An If-Range date that matches keeps the range.
    assert_eq!(replies[6].code(), 206);
    assert_eq!(replies[6].body, data_bytes(10));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn head_gives_headers_only_including_with_a_range() {
    let dir = tree("static-head");
    let replies = serve(
        &dir,
        &[
            case("HEAD", "/data.bin", &[]),
            case("HEAD", "/data.bin", &[("range", "bytes=0-99")]),
            case("HEAD", "/data.bin", &[("range", "bytes=999999-")]),
            case("HEAD", "/missing.txt", &[]),
        ],
    );
    assert_eq!(replies[0].code(), 200);
    assert_eq!(replies[0].header("content-length").unwrap(), "1000");
    assert_eq!(replies[0].header("etag").unwrap(), etag(1000));
    assert!(replies[0].body.is_empty());
    assert_eq!(replies[1].code(), 206);
    assert_eq!(replies[1].header("content-length").unwrap(), "100");
    assert_eq!(
        replies[1].header("content-range").unwrap(),
        "bytes 0-99/1000"
    );
    assert!(replies[1].body.is_empty());
    assert_eq!(replies[2].code(), 416);
    assert_eq!(replies[3].code(), 404);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn traversal_in_every_spelling_is_refused_and_nothing_outside_leaks() {
    let dir = tree("static-traversal");
    let paths = [
        "/../secret.txt",
        "/%2e%2e%2fsecret.txt",
        "/%2e%2e/secret.txt",
        "/..%2fsecret.txt",
        "/%2E%2E%2Fsecret.txt",
        "/%252e%252e%252fsecret.txt",
        "/sub/../../secret.txt",
        "/sub/%2e%2e/%2e%2e/secret.txt",
        "/..\\secret.txt",
        "/%5c..%5csecret.txt",
        "/a%00b",
        "/hello.txt%00.png",
        "//etc/passwd",
        "/link.txt",
        "/updir/secret.txt",
        "/.hidden",
        "/sub/.hidden",
        "/.%68idden",
        "/empty/",
        "/empty",
    ];
    let cases: Vec<Case> = paths.iter().map(|path| get(path)).collect();
    let replies = serve(&dir, &cases);
    for (path, reply) in paths.iter().zip(&replies) {
        assert_eq!(reply.code(), 404, "{path}: {}", reply.head);
        assert!(
            !String::from_utf8_lossy(&reply.body).contains("TOP-SECRET"),
            "{path}"
        );
        assert!(!reply.head.contains("TOP-SECRET"), "{path}");
    }
    let replies = serve(&dir, &[get("/%zz"), get("hello.txt"), get("")]);
    assert_eq!(replies[0].code(), 400, "malformed percent-encoding");
    assert_eq!(replies[1].code(), 400, "a path that is not absolute");
    assert_eq!(replies[2].code(), 400);
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn a_symlink_inside_the_root_to_a_file_inside_the_root_is_followed() {
    let dir = tree("static-inside-link");
    let replies = serve(&dir, &[get("/inside.txt")]);
    assert_eq!(replies[0].code(), 200);
    assert_eq!(replies[0].body, b"Hello, static!\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn index_files_are_served_and_directories_are_never_listed() {
    let dir = tree("static-index");
    let replies = serve(&dir, &[get("/"), get("/sub/"), get("/sub"), get("/empty/")]);
    assert_eq!(replies[0].code(), 200);
    assert_eq!(replies[0].body, b"<h1>root index</h1>");
    assert_eq!(
        replies[0].header("content-type").unwrap(),
        "text/html; charset=utf-8"
    );
    assert_eq!(replies[1].body, b"<h1>sub index</h1>");
    assert_eq!(replies[2].body, b"<h1>sub index</h1>");
    assert_eq!(replies[3].code(), 404);
    assert!(!String::from_utf8_lossy(&replies[3].body).contains("index"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn content_types_come_from_the_extension_table() {
    let dir = tree("static-types");
    let replies = serve(
        &dir,
        &[
            get("/a.css"),
            get("/a.js"),
            get("/a.json"),
            get("/a.png"),
            get("/a.svg"),
            get("/a.woff2"),
            get("/a.wasm"),
            get("/a.zzz"),
            get("/noext"),
            get("/UP.HTML"),
        ],
    );
    let types: Vec<String> = replies
        .iter()
        .map(|reply| reply.header("content-type").unwrap())
        .collect();
    assert_eq!(
        types,
        vec![
            "text/css; charset=utf-8",
            "text/javascript; charset=utf-8",
            "application/json",
            "image/png",
            "image/svg+xml",
            "font/woff2",
            "application/wasm",
            "application/octet-stream",
            "application/octet-stream",
            "text/html; charset=utf-8",
        ]
    );
    for reply in &replies {
        assert_eq!(reply.header("x-content-type-options").unwrap(), "nosniff");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn other_methods_are_405_and_a_missing_file_is_404() {
    let dir = tree("static-methods");
    let replies = serve(
        &dir,
        &[
            case("POST", "/hello.txt", &[]),
            case("DELETE", "/hello.txt", &[]),
            get("/nope.txt"),
        ],
    );
    assert_eq!(replies[0].code(), 405);
    assert_eq!(replies[0].header("allow").unwrap(), "GET, HEAD");
    assert_eq!(replies[1].code(), 405);
    assert_eq!(replies[2].code(), 404);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_range_of_a_two_gigabyte_sparse_file_does_not_read_the_whole_file() {
    let dir = tree("static-sparse");
    let public = dir.join("public");
    let size: u64 = 2 * 1024 * 1024 * 1024;
    {
        let file = std::fs::File::create(public.join("huge.bin")).unwrap();
        file.set_len(size).unwrap();
        // A marker in the middle that the range must find.
        let mut file = file;
        use std::io::{Seek, SeekFrom};
        file.seek(SeekFrom::Start(1_500_000_000)).unwrap();
        file.write_all(b"MARKER-0123456789").unwrap();
    }
    let started = std::time::Instant::now();
    let replies = serve(
        &dir,
        &[
            case(
                "GET",
                "/huge.bin",
                &[("range", "bytes=1500000000-1500000016")],
            ),
            case("GET", "/huge.bin", &[("range", "bytes=-17")]),
            case("HEAD", "/huge.bin", &[]),
        ],
    );
    assert_eq!(replies[0].code(), 206, "{}", replies[0].head);
    assert_eq!(replies[0].body, b"MARKER-0123456789");
    assert_eq!(
        replies[0].header("content-range").unwrap(),
        format!("bytes 1500000000-1500000016/{size}")
    );
    assert_eq!(replies[1].body, vec![0u8; 17]);
    assert_eq!(
        replies[2].header("content-length").unwrap(),
        size.to_string()
    );
    // Reading 2 GB (or refusing it at the file-size ceiling) would not finish like this.
    assert!(started.elapsed() < Duration::from_secs(30));
    let _ = std::fs::remove_dir_all(&dir);
}

const SERVER: &str = r##"import "std:http_server"
import "std:http_static"
import "std:http_response"
import "std:types"
fn serve_one(raw: HttpServerRequest) -> Unit:
    match http_request_value(raw):
        Ok(request):
            match static_serve(request, "public"):
                Ok(response):
                    let sent = http_server_send_response(raw, response)
                    if not is_ok(sent):
                        print("send " + error(sent).code)
                Err(problem):
                    print("serve " + problem.code)
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
                        serve_one(raw)
                    Err(problem):
                        print(problem.code)
            Err(problem):
                print("accept " + problem.code)
        set served = served + 1
    return 0
"##;

fn connect(port: u16) -> TcpStream {
    for _ in 0..100 {
        if let Ok(stream) = connect_to(port) {
            stream
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            return stream;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("server never listened on {port}");
}

fn exchange(port: u16, request: &[u8]) -> (String, Vec<u8>) {
    let mut stream = connect(port);
    stream.write_all(request).unwrap();
    let mut response = Vec::new();
    let _ = stream.read_to_end(&mut response);
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
fn a_real_server_streams_files_ranges_and_head_over_a_socket() {
    let port = free_port();
    let dir = tree("static-socket");
    // Larger than one block so the send loop runs more than once.
    let big: Vec<u8> = (0..300_000u32).map(|n| (n % 251) as u8).collect();
    std::fs::write(dir.join("public/big.bin"), &big).unwrap();
    std::fs::write(
        dir.join("main.gu"),
        SERVER.replace("PORT", &port.to_string()),
    )
    .unwrap();
    let root = dir.to_string_lossy().into_owned();
    let server: Child = spawn(
        Command::new(env!("CARGO_BIN_EXE_guion"))
            .args([
                "run",
                "--allow-bind",
                "127.0.0.1:*",
                "--file-root",
                root.as_str(),
            ])
            .arg("main.gu")
            .current_dir(&dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    )
    .expect("guion starts");

    let (head, body) = exchange(port, b"GET /big.bin HTTP/1.1\r\nHost: t\r\n\r\n");
    assert!(head.starts_with("HTTP/1.1 200 OK"), "{head}");
    assert!(head.contains("Content-Length: 300000"), "{head}");
    assert!(!head.contains("Transfer-Encoding"), "{head}");
    assert_eq!(body, big);

    let (head, body) = exchange(
        port,
        b"GET /big.bin HTTP/1.1\r\nHost: t\r\nRange: bytes=70000-70099\r\n\r\n",
    );
    assert!(head.starts_with("HTTP/1.1 206 Partial Content"), "{head}");
    assert!(
        head.contains("Content-Range: bytes 70000-70099/300000"),
        "{head}"
    );
    assert_eq!(body, big[70000..70100]);

    let (head, body) = exchange(
        port,
        b"HEAD /big.bin HTTP/1.1\r\nHost: t\r\nRange: bytes=0-99\r\n\r\n",
    );
    assert!(head.starts_with("HTTP/1.1 206"), "{head}");
    assert!(head.contains("Content-Length: 100"), "{head}");
    assert!(body.is_empty());

    let (head, body) = exchange(
        port,
        b"GET /hello.txt HTTP/1.1\r\nHost: t\r\nIf-None-Match: W/\"15-784887151000\"\r\n\r\n",
    );
    assert!(head.starts_with("HTTP/1.1 304 Not Modified"), "{head}");
    assert!(body.is_empty());

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

#[test]
fn no_handle_in_public_signatures() {
    let report = doc_json("std:http_static");
    let functions = report["functions"].as_array().unwrap();
    let names: Vec<&str> = functions
        .iter()
        .map(|function| function["name"].as_str().unwrap())
        .collect();
    for wanted in ["static_serve", "static_content_type", "static_http_date"] {
        assert!(names.contains(&wanted), "{wanted} missing from {names:?}");
    }
    for function in functions {
        let text = function.to_string();
        assert!(!HANDLE_TYPES.iter().any(|h| text.contains(h)), "{text}");
        assert!(function["effects"].as_array().is_some(), "{text}");
    }
    for record in report["records"].as_array().unwrap() {
        let text = record.to_string();
        assert!(!HANDLE_TYPES.iter().any(|h| text.contains(h)), "{text}");
    }
    let serve = functions
        .iter()
        .find(|function| function["name"] == "static_serve")
        .unwrap();
    assert!(
        serve["effects"].to_string().contains("filesystem.read"),
        "{serve}"
    );
    for pure in ["static_content_type", "static_http_date"] {
        let function = functions
            .iter()
            .find(|function| function["name"] == pure)
            .unwrap();
        assert!(function["effects"].as_array().unwrap().is_empty(), "{pure}");
    }
    // The pieces the server uses to send a file in bounded blocks cross no handle either.
    let response = doc_json("std:http_response");
    for name in [
        "response_file_plan",
        "response_file_head",
        "response_file_block",
        "response_file_end",
    ] {
        let function = response["functions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|function| function["name"] == name)
            .unwrap_or_else(|| panic!("{name} is documented"));
        assert!(
            !HANDLE_TYPES
                .iter()
                .any(|h| function.to_string().contains(h)),
            "{name}"
        );
    }
}

#[test]
fn the_module_name_is_reserved() {
    let _ = SystemTime::now();
    let names =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/package_name.rs"))
            .unwrap();
    assert!(names.contains("http-static"));
}
