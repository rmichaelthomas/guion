//! `language/examples/mcp_server`: an MCP server over newline-delimited JSON-RPC on stdio.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::thread;
use std::time::Duration;

const READ_TIMEOUT: Duration = Duration::from_secs(30);

struct Server {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: Receiver<Option<String>>,
}

impl Server {
    fn start() -> Server {
        let example =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../language/examples/mcp_server");
        let mut child = Command::new(env!("CARGO_BIN_EXE_guion"))
            .args(["run", example.to_str().unwrap()])
            .env("GUION_NO_PROMPT", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take();
        let stdout: ChildStdout = child.stdout.take().unwrap();
        let (sender, lines) = channel();
        thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) | Err(_) => {
                        let _ = sender.send(None);
                        return;
                    }
                    Ok(_) => {
                        if sender.send(Some(line.trim_end().to_string())).is_err() {
                            return;
                        }
                    }
                }
            }
        });
        Server {
            child,
            stdin,
            lines,
        }
    }

    fn send(&mut self, request: &str) {
        let stdin = self.stdin.as_mut().unwrap();
        stdin.write_all(request.as_bytes()).unwrap();
        stdin.write_all(b"\n").unwrap();
        stdin.flush().unwrap();
    }

    /// Reads exactly one response line, failing rather than blocking forever.
    fn read(&self) -> String {
        match self.lines.recv_timeout(READ_TIMEOUT) {
            Ok(Some(line)) => line,
            Ok(None) => panic!("server closed its output before replying"),
            Err(_) => panic!("no reply within {READ_TIMEOUT:?}"),
        }
    }

    fn call(&mut self, request: &str) -> String {
        self.send(request);
        self.read()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stdin.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn parse(line: &str) -> serde_json::Value {
    serde_json::from_str(line).unwrap_or_else(|error| panic!("not JSON ({error}): {line}"))
}

const INITIALIZE: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"0"}}}"#;

#[test]
fn mcp_handshake_and_call() {
    let mut server = Server::start();
    let init = parse(&server.call(INITIALIZE));
    assert_eq!(init["jsonrpc"], "2.0");
    assert_eq!(init["id"], 1);
    assert_eq!(init["result"]["serverInfo"]["name"], "guion-example");
    assert!(init["result"]["capabilities"]["tools"].is_object());

    // A notification has no reply: the next line read belongs to the next request.
    server.send(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#);

    let ping = parse(&server.call(r#"{"jsonrpc":"2.0","id":2,"method":"ping"}"#));
    assert_eq!(ping["id"], 2);
    assert!(ping["result"].is_object());

    let list = parse(&server.call(r#"{"jsonrpc":"2.0","id":3,"method":"tools/list"}"#));
    assert_eq!(list["id"], 3);
    assert_eq!(list["result"]["tools"][0]["name"], "echo");
    assert_eq!(list["result"]["tools"][0]["inputSchema"]["type"], "object");

    let call = parse(&server.call(
        r#"{"jsonrpc":"2.0","id":"abc","method":"tools/call","params":{"name":"echo","arguments":{"text":"hi"}}}"#,
    ));
    assert_eq!(call["id"], "abc");
    assert_eq!(call["result"]["content"][0]["type"], "text");
    assert_eq!(call["result"]["content"][0]["text"], "hi");
}

#[test]
fn mcp_survives_1000_calls() {
    let mut server = Server::start();
    server.call(INITIALIZE);
    for n in 0..1000 {
        let request = format!(
            r#"{{"jsonrpc":"2.0","id":{n},"method":"tools/call","params":{{"name":"echo","arguments":{{"text":"call {n}"}}}}}}"#
        );
        let reply = parse(&server.call(&request));
        assert_eq!(reply["id"], n);
        assert_eq!(reply["result"]["content"][0]["text"], format!("call {n}"));
    }
}

#[test]
fn mcp_unknown_method() {
    let mut server = Server::start();
    let reply = parse(&server.call(r#"{"jsonrpc":"2.0","id":7,"method":"nope/nothing"}"#));
    assert_eq!(reply["id"], 7);
    assert_eq!(reply["error"]["code"], -32601);
}

#[test]
fn mcp_eof_exits_zero() {
    let mut server = Server::start();
    server.call(INITIALIZE);
    server.stdin.take();
    let status = server.child.wait().unwrap();
    assert!(status.success(), "{status:?}");
}

#[test]
fn mcp_wrong_typed_fields_get_error_replies_and_the_server_keeps_serving() {
    let mut server = Server::start();
    server.call(INITIALIZE);

    // A `method` that is not text: Invalid Request, with the request's own id.
    let reply = parse(&server.call(r#"{"jsonrpc":"2.0","id":11,"method":5}"#));
    assert_eq!(reply["id"], 11);
    assert_eq!(reply["error"]["code"], -32600);

    // A request that is not an object at all: Invalid Request with a null id.
    for line in ["[1,2,3]", "5", "\"ping\"", "null"] {
        let reply = parse(&server.call(line));
        assert_eq!(reply["id"], serde_json::Value::Null, "{line}");
        assert_eq!(reply["error"]["code"], -32600, "{line}");
    }

    // Wrong-typed params: Invalid params.
    let reply =
        parse(&server.call(
            r#"{"jsonrpc":"2.0","id":12,"method":"initialize","params":{"protocolVersion":7}}"#,
        ));
    assert_eq!(reply["id"], 12);
    assert_eq!(reply["error"]["code"], -32602);
    let reply = parse(&server.call(
        r#"{"jsonrpc":"2.0","id":13,"method":"tools/call","params":{"name":3,"arguments":{"text":"x"}}}"#,
    ));
    assert_eq!(reply["id"], 13);
    assert_eq!(reply["error"]["code"], -32602);
    let reply = parse(&server.call(
        r#"{"jsonrpc":"2.0","id":14,"method":"tools/call","params":{"name":"echo","arguments":{"text":false}}}"#,
    ));
    assert_eq!(reply["id"], 14);
    assert_eq!(reply["error"]["code"], -32602);
    let reply =
        parse(&server.call(r#"{"jsonrpc":"2.0","id":15,"method":"tools/call","params":"echo"}"#));
    assert_eq!(reply["error"]["code"], -32602);

    // An invalid request with no id is still answered, with a null id (JSON-RPC 2.0, section 5).
    let reply = parse(&server.call(r#"{"jsonrpc":"2.0","method":false}"#));
    assert_eq!(reply["id"], serde_json::Value::Null);
    assert_eq!(reply["error"]["code"], -32600);

    // The server is still serving.
    let reply = parse(&server.call(
        r#"{"jsonrpc":"2.0","id":16,"method":"tools/call","params":{"name":"echo","arguments":{"text":"still here"}}}"#,
    ));
    assert_eq!(reply["id"], 16);
    assert_eq!(reply["result"]["content"][0]["text"], "still here");
}
