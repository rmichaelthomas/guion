//! `tcp_peer` in `std:net`, `http_server_peer` in `std:http_server` and `Request.peer` in
//! `std:http_response` (artifact format 90): the remote address of an accepted connection.

mod common;

use common::{connect_to, free_port, output, spawn};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::Duration;

struct Scratch(PathBuf);
impl Scratch {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("guion-netpeer-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn guion(dir: &Path, args: &[&str]) -> Output {
    output(
        Command::new(env!("CARGO_BIN_EXE_guion"))
            .args(args)
            .current_dir(dir)
            .env("GUION_NO_PROMPT", "1"),
    )
    .expect("guion starts")
}

fn text_of(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn run(scratch: &Scratch, flags: &[&str], source: &str) -> Output {
    std::fs::write(scratch.0.join("main.gu"), source).unwrap();
    let mut args = vec!["run"];
    args.extend_from_slice(flags);
    args.push("main.gu");
    guion(&scratch.0, &args)
}

const LOOPBACK: &[&str] = &[
    "--allow-bind",
    "127.0.0.1:0",
    "--allow-connect",
    "127.0.0.1:*",
];

#[test]
fn tcp_peer_reports_the_address_each_end_sees() {
    let scratch = Scratch::new("tcp");
    let output = run(
        &scratch,
        LOOPBACK,
        r#"import "std:net"
import "std:types"
fn main() -> Int:
    let listener = unwrap(tcp_listen("127.0.0.1", 0))
    let port = unwrap(tcp_listener_port(listener))
    let client = unwrap(tcp_connect("127.0.0.1", port, 2000))
    let server = unwrap(tcp_accept(listener, 2000))
    let client_port = unwrap(tcp_listener_port(TcpListener(client.handle)))
    let seen = unwrap(tcp_peer(server))
    assert(seen.host == "127.0.0.1")
    assert(seen.port == client_port)
    let back = unwrap(tcp_peer(client))
    assert(back.host == "127.0.0.1")
    assert(back.port == port)
    print(seen.host + ":" + int_to_text(seen.port))
    return 0
"#,
    );
    assert!(output.status.success(), "{}", text_of(&output));
    let line = String::from_utf8_lossy(&output.stdout);
    assert!(line.trim().starts_with("127.0.0.1:"), "{line}");
}

#[test]
fn tcp_peer_refuses_a_listener_a_datagram_socket_and_a_closed_handle() {
    let scratch = Scratch::new("handles");
    let output = run(
        &scratch,
        LOOPBACK,
        r#"import "std:net"
import "std:types"
fn code_of(result: Result[PeerAddress, StdError]) -> Text:
    match result:
        Ok(address):
            return "ok"
        Err(problem):
            return problem.code
fn main() -> Int:
    let listener = unwrap(tcp_listen("127.0.0.1", 0))
    let socket = unwrap(udp_bind("127.0.0.1", 0))
    assert(code_of(tcp_peer(TcpConn(listener.handle))) == "E_NET_HANDLE")
    assert(code_of(tcp_peer(TcpConn(socket.handle))) == "E_NET_HANDLE")
    assert(code_of(tcp_peer(TcpConn(12345))) == "E_NET_HANDLE")
    return 0
"#,
    );
    assert!(output.status.success(), "{}", text_of(&output));
}

/// An IPv6 host is reported without brackets. Skipped where the machine has no IPv6 loopback.
#[test]
fn tcp_peer_writes_an_ipv6_host_without_brackets() {
    if std::net::TcpListener::bind("[::1]:0").is_err() {
        return;
    }
    let scratch = Scratch::new("ipv6");
    let output = run(
        &scratch,
        &["--allow-bind", "::1:0", "--allow-connect", "::1:*"],
        r#"import "std:net"
import "std:types"
fn main() -> Int:
    let listener = unwrap(tcp_listen("::1", 0))
    let port = unwrap(tcp_listener_port(listener))
    let client = unwrap(tcp_connect("::1", port, 2000))
    let server = unwrap(tcp_accept(listener, 2000))
    let seen = unwrap(tcp_peer(server))
    assert(seen.host == "::1")
    assert(seen.port == unwrap(tcp_listener_port(TcpListener(client.handle))))
    return 0
"#,
    );
    assert!(output.status.success(), "{}", text_of(&output));
}

const HTTP_SERVER: &str = r#"import "std:net"
import "std:http_server"
import "std:http_response"
import "std:types"
fn main() -> Int:
    let port = unwrap(text_parse_int(args()[0]))
    let listener = unwrap(http_server_listen("127.0.0.1", port))
    let conn = unwrap(http_server_accept(listener, 10000))
    let request = unwrap(http_server_read_request(conn, 5000, 65536))
    let address = unwrap(http_server_peer(request))
    let value = unwrap(http_request_value(request))
    let body = address.host + "|" + int_to_text(address.port) + "|" + value.peer
    let sent = http_server_reply_text(request, 200, body)
    return 0
"#;

#[test]
fn an_http_request_carries_the_peer_of_its_connection() {
    let scratch = Scratch::new("http");
    std::fs::write(scratch.0.join("main.gu"), HTTP_SERVER).unwrap();
    let port = free_port();
    let bind = format!("127.0.0.1:{port}");
    let mut child = spawn(
        Command::new(env!("CARGO_BIN_EXE_guion"))
            .args(["run", "--allow-bind", &bind, "main.gu", &port.to_string()])
            .current_dir(&scratch.0)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    )
    .expect("guion starts");
    let mut stream = None;
    for _ in 0..100 {
        if let Ok(connected) = connect_to(port) {
            stream = Some(connected);
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let mut stream = stream.expect("the server listens");
    // The first connection above is the server's only accept, so it is the request.
    stream
        .set_read_timeout(Some(Duration::from_secs(8)))
        .unwrap();
    let local = stream.local_addr().unwrap();
    stream
        .write_all(b"GET /who HTTP/1.1\r\nHost: x\r\n\r\n")
        .unwrap();
    let mut answer = Vec::new();
    let _ = stream.read_to_end(&mut answer);
    let answer = String::from_utf8_lossy(&answer).into_owned();
    let _ = child.wait();
    let expected = format!("127.0.0.1|{}|127.0.0.1:{}", local.port(), local.port());
    assert!(answer.starts_with("HTTP/1.1 200"), "{answer}");
    assert!(answer.ends_with(&expected), "{answer} vs {expected}");
}

#[test]
fn a_request_literal_with_a_peer_round_trips_and_request_new_leaves_it_empty() {
    let scratch = Scratch::new("literal");
    let output = run(
        &scratch,
        &[],
        r#"import "std:http_response"
import "std:types"
fn main() -> Int:
    let query: Map[Text, List[Text]] = {}
    let params: Map[Text, Text] = {}
    let request = Request("GET", "/", query, [], bytes_from_ints([]), params, "[::1]:4000")
    assert(request.peer == "[::1]:4000")
    let copy = Request(request.method, request.path, request.query, request.headers, request.body, request.params, request.peer)
    assert(copy == request)
    assert(request_new("GET", "/").peer == "")
    return 0
"#,
    );
    assert!(output.status.success(), "{}", text_of(&output));
}

#[test]
fn an_artifact_below_format_90_that_uses_tcp_peer_is_refused() {
    let scratch = Scratch::new("format");
    let program = scratch.0.join("main.gu");
    std::fs::write(
        &program,
        "import \"std:net\"\nfn main() -> Int:\n    print(is_ok(tcp_peer(TcpConn(1))))\n    return 0\n",
    )
    .unwrap();
    let artifact = scratch.0.join("main.gub");
    let build = guion(
        &scratch.0,
        &[
            "build",
            program.to_str().unwrap(),
            "--output",
            artifact.to_str().unwrap(),
        ],
    );
    assert!(build.status.success(), "{}", text_of(&build));
    let mut bytes = std::fs::read(&artifact).unwrap();
    bytes[4..8].copy_from_slice(&89u32.to_le_bytes());
    std::fs::write(&artifact, bytes).unwrap();
    let output = guion(&scratch.0, &["run", artifact.to_str().unwrap()]);
    assert!(!output.status.success());
    assert!(
        text_of(&output).contains("E_ARTIFACT_VERSION"),
        "{}",
        text_of(&output)
    );
}

#[test]
fn inspect_reports_the_socket_effect_for_tcp_peer() {
    let scratch = Scratch::new("inspect");
    std::fs::write(
        scratch.0.join("main.gu"),
        "import \"std:net\"\nfn main() -> Int:\n    print(is_ok(tcp_peer(TcpConn(1))))\n    return 0\n",
    )
    .unwrap();
    let output = guion(&scratch.0, &["inspect", "--json", "main.gu"]);
    assert!(output.status.success(), "{}", text_of(&output));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let effects: Vec<&str> = report["required_effects"]
        .as_array()
        .unwrap()
        .iter()
        .map(|effect| effect.as_str().unwrap())
        .collect();
    assert!(effects.contains(&"network.socket"), "{effects:?}");
}

#[test]
fn guion_doc_lists_the_new_functions() {
    let scratch = Scratch::new("doc");
    let net = guion(&scratch.0, &["doc", "std:net"]);
    assert!(net.status.success(), "{}", text_of(&net));
    let text = text_of(&net);
    assert!(
        text.contains("tcp_peer(conn: TcpConn) -> Result[PeerAddress, StdError]"),
        "{text}"
    );
    let server = guion(&scratch.0, &["doc", "std:http_server"]);
    let text = text_of(&server);
    assert!(
        text.contains(
            "http_server_peer(request: HttpServerRequest) -> Result[PeerAddress, StdError]"
        ),
        "{text}"
    );
}
