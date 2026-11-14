//! `guion lsp` (scripted protocol messages) and `guion repl` (piped input).

use serde_json::{json, Value};
use std::io::Write;
use std::process::{Command, Stdio};

fn frame(message: &Value) -> Vec<u8> {
    let body = message.to_string();
    format!("Content-Length: {}\r\n\r\n{}", body.len(), body).into_bytes()
}

/// Sends the messages to `guion lsp`, then `shutdown` and `exit`, and returns every message
/// the server wrote.
fn converse(messages: Vec<Value>) -> Vec<Value> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_guion"))
        .arg("lsp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("guion lsp starts");
    let mut stdin = child.stdin.take().unwrap();
    for message in &messages {
        stdin.write_all(&frame(message)).unwrap();
    }
    stdin
        .write_all(&frame(&json!({"jsonrpc": "2.0", "id": 999, "method": "shutdown"})))
        .unwrap();
    stdin
        .write_all(&frame(&json!({"jsonrpc": "2.0", "method": "exit"})))
        .unwrap();
    drop(stdin);
    let output = child.wait_with_output().unwrap();
    let mut rest = output.stdout.as_slice();
    let mut found = Vec::new();
    while let Some(split) = rest.windows(4).position(|window| window == b"\r\n\r\n") {
        let header = std::str::from_utf8(&rest[..split]).unwrap();
        let length: usize = header
            .to_ascii_lowercase()
            .strip_prefix("content-length:")
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        let body = &rest[split + 4..split + 4 + length];
        found.push(serde_json::from_slice(body).unwrap());
        rest = &rest[split + 4 + length..];
    }
    found
}

const URI: &str = "file:///tmp/guion-lsp-test/main.gu";

fn open(text: &str) -> Vec<Value> {
    vec![
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}}),
        json!({"jsonrpc": "2.0", "method": "initialized", "params": {}}),
        json!({"jsonrpc": "2.0", "method": "textDocument/didOpen", "params": {
            "textDocument": {"uri": URI, "languageId": "guion", "version": 1, "text": text}}}),
    ]
}

fn response(messages: &[Value], id: u64) -> &Value {
    messages
        .iter()
        .find(|message| message["id"] == json!(id))
        .unwrap_or_else(|| panic!("no response with id {id}: {messages:?}"))
}

const SAMPLE: &str = "# Adds one.\nfn bump(n: Int) -> Int:\n    return n + 1\n\nrecord Point:\n    x: Int\n\nfn main() -> Int:\n    let total = bump(2)\n    print(f\"{total}\")\n    return 0\n";

#[test]
fn initialize_announces_the_features() {
    let messages = converse(open(SAMPLE));
    let capabilities = &response(&messages, 1)["result"]["capabilities"];
    assert_eq!(capabilities["hoverProvider"], json!(true));
    assert_eq!(capabilities["definitionProvider"], json!(true));
    assert_eq!(capabilities["documentFormattingProvider"], json!(true));
    assert!(capabilities["completionProvider"].is_object());
}

#[test]
fn a_clean_buffer_has_no_diagnostics_and_a_broken_one_has_the_compiler_error() {
    let clean = converse(open(SAMPLE));
    let published = clean
        .iter()
        .find(|message| message["method"] == "textDocument/publishDiagnostics")
        .unwrap();
    assert_eq!(published["params"]["diagnostics"], json!([]));

    let broken = SAMPLE.replace("bump(2)", "bumpp(2)");
    let messages = converse(open(&broken));
    let published = messages
        .iter()
        .find(|message| message["method"] == "textDocument/publishDiagnostics")
        .unwrap();
    let diagnostic = &published["params"]["diagnostics"][0];
    assert_eq!(diagnostic["code"], json!("E_NAME_UNKNOWN"));
    assert_eq!(diagnostic["range"]["start"]["line"], json!(8));
    assert_eq!(diagnostic["severity"], json!(1));
    assert!(diagnostic["message"].as_str().unwrap().contains("bump"));
}

#[test]
fn a_change_replaces_the_diagnostics() {
    let mut messages = open(&SAMPLE.replace("bump(2)", "bumpp(2)"));
    messages.push(json!({"jsonrpc": "2.0", "method": "textDocument/didChange", "params": {
        "textDocument": {"uri": URI, "version": 2},
        "contentChanges": [{"text": SAMPLE}]}}));
    let replies = converse(messages);
    let published: Vec<&Value> = replies
        .iter()
        .filter(|message| message["method"] == "textDocument/publishDiagnostics")
        .collect();
    assert_eq!(published.len(), 2);
    assert_eq!(published[0]["params"]["diagnostics"].as_array().unwrap().len(), 1);
    assert_eq!(published[1]["params"]["diagnostics"], json!([]));
}

#[test]
fn hover_shows_signatures_and_doc_lines() {
    let mut messages = open(SAMPLE);
    // `bump` in `let total = bump(2)` (line 8, column 16), and the built-in `len`.
    messages.push(json!({"jsonrpc": "2.0", "id": 2, "method": "textDocument/hover", "params": {
        "textDocument": {"uri": URI}, "position": {"line": 8, "character": 16}}}));
    messages.push(json!({"jsonrpc": "2.0", "id": 3, "method": "textDocument/hover", "params": {
        "textDocument": {"uri": URI}, "position": {"line": 0, "character": 0}}}));
    let replies = converse(messages);
    let hover = response(&replies, 2)["result"]["contents"]["value"].as_str().unwrap();
    assert!(hover.contains("fn bump(n: Int) -> Int"), "{hover}");
    assert!(hover.contains("Adds one."), "{hover}");
    assert!(response(&replies, 3)["result"].is_null());
}

#[test]
fn go_to_definition_finds_functions_and_records() {
    let mut messages = open(SAMPLE);
    messages.push(json!({"jsonrpc": "2.0", "id": 2, "method": "textDocument/definition", "params": {
        "textDocument": {"uri": URI}, "position": {"line": 8, "character": 16}}}));
    let replies = converse(messages);
    let location = &response(&replies, 2)["result"];
    assert_eq!(location["uri"], json!(URI));
    assert_eq!(location["range"]["start"]["line"], json!(1));
    assert_eq!(location["range"]["start"]["character"], json!(3));
}

#[test]
fn completion_lists_names_in_scope_builtins_and_keywords() {
    let mut messages = open(SAMPLE);
    messages.push(json!({"jsonrpc": "2.0", "id": 2, "method": "textDocument/completion", "params": {
        "textDocument": {"uri": URI}, "position": {"line": 9, "character": 4}}}));
    let replies = converse(messages);
    let labels: Vec<&str> = response(&replies, 2)["result"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["label"].as_str().unwrap())
        .collect();
    for wanted in ["bump", "Point", "total", "print", "while", "int_to_text"] {
        assert!(labels.contains(&wanted), "{wanted} missing from completion");
    }
}

#[test]
fn formatting_returns_the_formatted_document() {
    let mut messages = open("fn main() -> Int:\n    print(len([1,2]))\n    return 0\n");
    messages.push(json!({"jsonrpc": "2.0", "id": 2, "method": "textDocument/formatting", "params": {
        "textDocument": {"uri": URI}, "options": {"tabSize": 4, "insertSpaces": true}}}));
    let replies = converse(messages);
    let edits = response(&replies, 2)["result"].as_array().unwrap();
    assert_eq!(edits.len(), 1);
    assert_eq!(
        edits[0]["newText"],
        json!("fn main() -> Int:\n    print(len([1, 2]))\n    return 0\n")
    );
}

#[test]
fn an_unknown_request_is_answered_and_the_server_exits_cleanly() {
    let replies = converse(vec![
        json!({"jsonrpc": "2.0", "id": 5, "method": "workspace/nonsense", "params": {}}),
    ]);
    assert_eq!(response(&replies, 5)["error"]["code"], json!(-32601));
    assert_eq!(response(&replies, 999)["result"], Value::Null);
}

fn repl(input: &str) -> (String, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_guion"))
        .arg("repl")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("guion repl starts");
    child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
    let output = child.wait_with_output().unwrap();
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

#[test]
fn the_repl_shows_values_and_types_and_keeps_definitions() {
    let (out, err) = repl(
        "let x = 4\nx * 2\nfn double(n: Int) -> Int:\n    return n * 2\n\ndouble(x)\n\"hi\"\n[1, 2, 3]\nprint(\"printed\")\n1.5 + 1.0\n:quit\n",
    );
    assert_eq!(
        out,
        "ok\n8 : Int\nok\n8 : Int\nhi : Text\n[1, 2, 3] : List[Int]\nprinted\n2.5 : Float64\n",
        "stderr: {err}"
    );
}

#[test]
fn the_repl_reports_errors_and_keeps_going() {
    let (out, err) = repl("nonsense(1)\nlet y = 2\ny + 1\n:quit\n");
    assert!(err.contains("E_NAME_UNKNOWN"), "{err}");
    assert_eq!(out, "ok\n3 : Int\n");
}

#[test]
fn the_repl_runs_statements_once_and_knows_records() {
    let (out, err) = repl(
        "record Pair:\n    a: Int\n    b: Int\n\nPair(1, 2)\nfor i in range(3):\n    print(f\"{i}\")\n\n:quit\n",
    );
    assert!(out.starts_with("ok\n"), "{out} {err}");
    assert!(out.contains("Pair : Pair") || out.contains(": Pair"), "{out} {err}");
    assert!(out.ends_with("0\n1\n2\n"), "{out} {err}");
}
