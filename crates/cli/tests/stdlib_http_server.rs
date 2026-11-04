use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

fn free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

fn project(name: &str, source: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("guion-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("main.gu"), source).unwrap();
    dir
}

fn spawn_guion(dir: &Path, flags: &[&str]) -> Child {
    Command::new(env!("CARGO_BIN_EXE_guion"))
        .arg("run")
        .args(flags)
        .arg("main.gu")
        .current_dir(dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("guion starts")
}

fn connect(port: u16) -> TcpStream {
    for _ in 0..100 {
        if let Ok(stream) = TcpStream::connect(("127.0.0.1", port)) {
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            return stream;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("server never listened on {port}");
}

fn exchange(port: u16, request: &[u8]) -> String {
    let mut stream = connect(port);
    stream.write_all(request).unwrap();
    let mut response = Vec::new();
    let _ = stream.read_to_end(&mut response);
    String::from_utf8_lossy(&response).into_owned()
}

const HTTP_SERVER: &str = r##"import "std:http_server"
import "std:net"
import "std:types"
fn route(request: HttpServerRequest) -> Int:
    if request.path == "/hello":
        let sent = http_server_respond_text(request, 200, "hello")
    else:
        if request.path == "/echo" and request.method == "POST":
            let sent = http_server_respond(request, 200, "application/octet-stream", request.body)
        else:
            if request.path == "/query":
                let sent = http_server_respond_text(request, 200, request.path + "|" + request.query + "|" + request.method)
            else:
                let sent = http_server_respond_text(request, 404, "no route")
    return 0
fn main() -> Int:
    let listener = unwrap(http_server_listen("127.0.0.1", PORT))
    var served = 0
    while served < 8:
        match http_server_accept(listener, 20000):
            Ok(conn):
                match http_server_read_request(conn, 3000, 1000):
                    Ok(request):
                        let done = route(request)
                    Err(problem):
                        print(problem.code)
            Err(problem):
                print("accept " + problem.code)
        set served = served + 1
    return 0
"##;

#[test]
fn http_server_routes_requests_and_rejects_malformed_or_oversized_ones() {
    let port = free_port();
    let dir = project(
        "http-server",
        &HTTP_SERVER.replace("PORT", &port.to_string()),
    );
    let server = spawn_guion(&dir, &["--allow-bind", "127.0.0.1:*"]);

    let hello = exchange(port, b"GET /hello HTTP/1.1\r\nHost: x\r\n\r\n");
    assert!(hello.starts_with("HTTP/1.1 200 OK\r\n"), "{hello}");
    assert!(hello.contains("Content-Length: 5\r\n"), "{hello}");
    assert!(hello.ends_with("\r\n\r\nhello"), "{hello}");

    let echoed = exchange(
        port,
        b"POST /echo HTTP/1.1\r\nHost: x\r\nContent-Length: 3\r\n\r\nabc",
    );
    assert!(echoed.ends_with("\r\n\r\nabc"), "{echoed}");

    let query = exchange(port, b"GET /query?a=1&b=2 HTTP/1.0\r\n\r\n");
    assert!(query.ends_with("/query|a=1&b=2|GET"), "{query}");

    let missing = exchange(port, b"GET /nope HTTP/1.1\r\n\r\n");
    assert!(missing.starts_with("HTTP/1.1 404 Not Found"), "{missing}");

    let chunked = exchange(
        port,
        b"POST /echo HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n",
    );
    assert!(chunked.starts_with("HTTP/1.1 501"), "{chunked}");

    let big = exchange(port, b"POST /echo HTTP/1.1\r\nContent-Length: 5000\r\n\r\n");
    assert!(big.starts_with("HTTP/1.1 413"), "{big}");

    let garbage = exchange(port, b"FOO\r\n\r\n");
    assert!(garbage.starts_with("HTTP/1.1 400"), "{garbage}");

    let mut huge = b"GET / HTTP/1.1\r\nX-Pad: ".to_vec();
    huge.extend(std::iter::repeat_n(b'a', 20000));
    huge.extend_from_slice(b"\r\n\r\n");
    let too_large = exchange(port, &huge);
    assert!(too_large.starts_with("HTTP/1.1 431"), "{too_large}");

    let output = server.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "{stdout}{}",
        String::from_utf8_lossy(&output.stderr)
    );
    for code in [
        "E_HTTP_UNSUPPORTED",
        "E_HTTP_BODY_TOO_LARGE",
        "E_HTTP_REQUEST_INVALID",
        "E_HTTP_HEADER_TOO_LARGE",
    ] {
        assert!(stdout.contains(code), "{code} missing from {stdout}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

const WS_SERVER: &str = r##"import "std:http_server"
import "std:websocket"
import "std:net"
import "std:types"
fn serve(ws: WsConn) -> Int:
    var running = true
    while running:
        match ws_receive(ws, 5000, 100000):
            Ok(message):
                if message.kind == "text":
                    let sent = ws_send_text(ws, "echo:" + message.text)
                else:
                    if message.kind == "binary":
                        let sent = ws_send_binary(ws, message.data)
                    else:
                        if message.kind == "close":
                            set running = false
            Err(problem):
                print(problem.code)
                set running = false
    return 0
fn main() -> Int:
    let listener = unwrap(http_server_listen("127.0.0.1", PORT))
    var served = 0
    while served < 4:
        match http_server_accept(listener, 20000):
            Ok(conn):
                match http_server_read_request(conn, 3000, 1000):
                    Ok(request):
                        match ws_server_accept(request):
                            Ok(ws):
                                let done = serve(ws)
                                let closed = ws_close(ws)
                            Err(problem):
                                print(problem.code)
                    Err(problem):
                        print(problem.code)
            Err(problem):
                print("accept " + problem.code)
        set served = served + 1
    return 0
"##;

fn upgrade(port: u16) -> TcpStream {
    let mut stream = connect(port);
    stream
        .write_all(b"GET /ws HTTP/1.1\r\nHost: x\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n")
        .unwrap();
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        stream.read_exact(&mut byte).unwrap();
        head.push(byte[0]);
    }
    let head = String::from_utf8_lossy(&head).into_owned();
    assert!(
        head.starts_with("HTTP/1.1 101 Switching Protocols"),
        "{head}"
    );
    assert!(
        head.contains("Sec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo=\r\n"),
        "{head}"
    );
    stream
}

fn masked_frame(first: u8, payload: &[u8]) -> Vec<u8> {
    let mask = [0x37u8, 0xfa, 0x21, 0x3d];
    let mut frame = vec![first];
    if payload.len() < 126 {
        frame.push(0x80 | payload.len() as u8);
    } else {
        frame.push(0x80 | 126);
        frame.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    }
    frame.extend_from_slice(&mask);
    frame.extend(payload.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
    frame
}

fn read_frame(stream: &mut TcpStream) -> (u8, Vec<u8>) {
    let mut head = [0u8; 2];
    stream.read_exact(&mut head).unwrap();
    assert_eq!(head[1] & 0x80, 0, "server frames are never masked");
    let mut length = usize::from(head[1] & 0x7f);
    if length == 126 {
        let mut extended = [0u8; 2];
        stream.read_exact(&mut extended).unwrap();
        length = usize::from(u16::from_be_bytes(extended));
    }
    let mut payload = vec![0u8; length];
    stream.read_exact(&mut payload).unwrap();
    (head[0], payload)
}

#[test]
fn websocket_server_handshakes_echoes_reassembles_and_enforces_masking() {
    let port = free_port();
    let dir = project("ws-server", &WS_SERVER.replace("PORT", &port.to_string()));
    let server = spawn_guion(&dir, &["--allow-bind", "127.0.0.1:*"]);

    // 1. A normal conversation: text, ping, fragmentation, a 300-byte binary frame, close.
    let mut stream = upgrade(port);
    stream.write_all(&masked_frame(0x81, b"hi")).unwrap();
    assert_eq!(read_frame(&mut stream), (0x81, b"echo:hi".to_vec()));
    stream.write_all(&masked_frame(0x89, b"beat")).unwrap();
    assert_eq!(read_frame(&mut stream), (0x8a, b"beat".to_vec()));
    stream.write_all(&masked_frame(0x01, b"ab")).unwrap();
    stream.write_all(&masked_frame(0x00, b"c")).unwrap();
    stream.write_all(&masked_frame(0x80, b"d")).unwrap();
    assert_eq!(read_frame(&mut stream), (0x81, b"echo:abcd".to_vec()));
    let blob = vec![7u8; 300];
    stream.write_all(&masked_frame(0x82, &blob)).unwrap();
    assert_eq!(read_frame(&mut stream), (0x82, blob));
    stream
        .write_all(&masked_frame(0x88, &1000u16.to_be_bytes()))
        .unwrap();
    assert_eq!(
        read_frame(&mut stream),
        (0x88, 1000u16.to_be_bytes().to_vec())
    );

    // 2. An unmasked client frame is a protocol error answered with close code 1002.
    let mut stream = upgrade(port);
    stream.write_all(&[0x81, 0x02, b'h', b'i']).unwrap();
    assert_eq!(
        read_frame(&mut stream),
        (0x88, 1002u16.to_be_bytes().to_vec())
    );

    // 3. A message over the 100,000-byte limit is answered with close code 1009.
    let mut stream = upgrade(port);
    let mut oversized = vec![0x82, 0x80 | 127];
    oversized.extend_from_slice(&200_000u64.to_be_bytes());
    oversized.extend_from_slice(&[1, 2, 3, 4]);
    stream.write_all(&oversized).unwrap();
    assert_eq!(
        read_frame(&mut stream),
        (0x88, 1009u16.to_be_bytes().to_vec())
    );

    // 4. A plain GET without upgrade headers is refused and the server keeps running.
    let refused = exchange(port, b"GET /ws HTTP/1.1\r\nHost: x\r\n\r\n");
    assert!(refused.starts_with("HTTP/1.1 400"), "{refused}");

    let output = server.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "{stdout}{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(stdout.contains("E_WS_PROTOCOL"), "{stdout}");
    assert!(stdout.contains("E_WS_TOO_LARGE"), "{stdout}");
    assert!(stdout.contains("E_WS_HANDSHAKE"), "{stdout}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn guion_websocket_client_talks_to_a_guion_server() {
    let port = free_port();
    let dir = project(
        "ws-pair-server",
        &WS_SERVER.replace("PORT", &port.to_string()),
    );
    let mut server = spawn_guion(&dir, &["--allow-bind", "127.0.0.1:*"]);
    let _ = connect(port); // wait until it listens; this connection is abandoned
    let client = format!(
        r##"import "std:websocket"
import "std:net"
import "std:types"
fn main() -> Int:
    let conn = unwrap(tcp_connect("127.0.0.1", {port}, 3000))
    let ws = unwrap(ws_client_handshake(conn, "127.0.0.1", "/ws", 3000, 12345))
    assert(is_ok(ws_send_text(ws, "ping pong")))
    let reply = unwrap(ws_receive(ws, 3000, 100000))
    assert(reply.kind == "text")
    assert(reply.text == "echo:ping pong")
    var big = ""
    var n = 0
    while n < 400:
        set big = big + "xy"
        set n = n + 1
    assert(is_ok(ws_send_text(ws, big)))
    assert(unwrap(ws_receive(ws, 3000, 100000)).text == "echo:" + big)
    assert(is_ok(ws_send_close(ws, 1000)))
    let closing = unwrap(ws_receive(ws, 3000, 100000))
    assert(closing.kind == "close")
    assert(closing.code == 1000)
    print("client-ok")
    return 0
"##
    );
    let client_dir = project("ws-pair-client", &client);
    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", "--allow-connect", "127.0.0.1:*", "main.gu"])
        .current_dir(&client_dir)
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("client-ok"),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let _ = server.kill();
    let _ = server.wait();
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&client_dir);
}

#[test]
fn sha1_and_xor_match_known_answers_and_the_rfc_6455_accept_key() {
    let dir = project(
        "sha1",
        r##"import "std:websocket"
import "std:crypto"
import "std:encoding"
fn main() -> Int:
    assert(ws_accept_key("dGhlIHNhbXBsZSBub25jZQ==") == "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=")
    assert(hex_encode(crypto_sha1(bytes_from_text("abc"))) == "a9993e364706816aba3e25717850c26c9cd0d89d")
    assert(hex_encode(crypto_sha1(bytes_from_text(""))) == "da39a3ee5e6b4b0d3255bfef95601890afd80709")
    assert(hex_encode(crypto_sha1(bytes_from_text("abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"))) == "84983e441c3bd26ebaae4aa1f95129e5e54670f1")
    assert(bytes_xor_key(bytes_from_ints([1, 2, 3, 4, 5]), bytes_from_ints([255, 0])) == bytes_from_ints([254, 2, 252, 4, 250]))
    return 0
"##,
    );
    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", "main.gu"])
        .current_dir(&dir)
        .output()
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
