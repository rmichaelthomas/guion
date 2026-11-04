use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::Duration;

fn fixtures() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tls")
}

fn project(name: &str, source: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("guion-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("main.gu"), source).unwrap();
    for file in ["ca.pem", "other-ca.pem", "server.pem", "server.key"] {
        std::fs::copy(fixtures().join(file), dir.join(file)).unwrap();
    }
    dir
}

fn guion(dir: &Path, flags: &[&str]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_guion"));
    command
        .arg("run")
        .args(flags)
        .arg("main.gu")
        .current_dir(dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

fn free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

const SERVER: &str = r#"import "std:net"
import "std:types"
fn main() -> Int:
    let chain = unwrap(read_text("server.pem"))
    let key = unwrap(read_text("server.key"))
    let listener = unwrap(tcp_listen("127.0.0.1", PORT))
    match tls_accept(listener, 8000, chain, key):
        Ok(conn):
            let request = unwrap(tcp_recv(conn, 64, 4000))
            let reply = bytes_from_text("echo:" + text_from_bytes_lossy(request))
            let sent = tcp_send(conn, reply)
            print("served")
        Err(problem):
            print("server-error " + problem.code)
    return 0
"#;

fn client(ca_file: &str, port: u16) -> String {
    format!(
        r#"import "std:net"
import "std:types"
fn main() -> Int:
    let ca = {ca}
    match tls_connect("127.0.0.1", {port}, 4000, ca):
        Ok(conn):
            let sent = unwrap(tcp_send_text(conn, "hello tls"))
            let reply = unwrap(tcp_recv(conn, 64, 4000))
            print(text_from_bytes_lossy(reply))
        Err(problem):
            print(problem.code)
    return 0
"#,
        ca = if ca_file.is_empty() {
            "\"\"".to_string()
        } else {
            format!("unwrap(read_text(\"{ca_file}\"))")
        }
    )
}

/// Starts the TLS server program, then runs the client until it stops seeing a refused connect.
fn exchange(name: &str, ca_file: &str) -> (Output, Output) {
    let port = free_port();
    let dir = project(name, &SERVER.replace("PORT", &port.to_string()));
    let mut server = guion(&dir, &["--allow-bind", "127.0.0.1:*"])
        .spawn()
        .expect("server starts");
    let client_dir = project(&format!("{name}-client"), &client(ca_file, port));
    let mut result = None;
    for _ in 0..50 {
        let output = guion(&client_dir, &["--allow-connect", "127.0.0.1:*"])
            .output()
            .expect("client starts");
        if !String::from_utf8_lossy(&output.stdout).contains("E_NET_REFUSED") {
            result = Some(output);
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let client_output = result.expect("server never accepted a connection");
    // A handshake that fails on the client side may leave the server waiting; end it.
    std::thread::sleep(Duration::from_millis(200));
    let _ = server.kill();
    let server_output = server.wait_with_output().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&client_dir);
    (client_output, server_output)
}

#[test]
fn tls_round_trip_verifies_the_server_certificate_against_a_supplied_ca() {
    let (client, server) = exchange("tls-ok", "ca.pem");
    let client_out = String::from_utf8_lossy(&client.stdout);
    assert_eq!(client_out.trim(), "echo:hello tls", "{client:?}");
    assert!(
        String::from_utf8_lossy(&server.stdout).contains("served"),
        "{server:?}"
    );
}

#[test]
fn untrusted_issuers_are_rejected_with_a_certificate_error() {
    for (name, ca) in [("tls-other", "other-ca.pem"), ("tls-roots", "")] {
        let (client, _) = exchange(name, ca);
        assert_eq!(
            String::from_utf8_lossy(&client.stdout).trim(),
            "E_TLS_CERTIFICATE",
            "{name}: {client:?}"
        );
    }
}

#[test]
fn tls_needs_the_same_connect_grant_as_tcp() {
    let dir = project("tls-deny", &client("ca.pem", 9));
    let output = guion(&dir, &[]).output().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "E_NET_DENIED"
    );
}
