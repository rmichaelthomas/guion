use std::process::Command;

fn run_project(name: &str, manifest: &str, flags: &[&str], source: &str) -> std::process::Output {
    let dir = std::env::temp_dir().join(format!("guion-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("main.gu"), source).unwrap();
    if !manifest.is_empty() {
        std::fs::write(dir.join("guion.toml"), manifest).unwrap();
    }
    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .arg("run")
        .args(flags)
        .arg("main.gu")
        .current_dir(&dir)
        .output()
        .expect("guion starts");
    let _ = std::fs::remove_dir_all(&dir);
    output
}

fn assert_ok(output: &std::process::Output) {
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

const LOOPBACK: &[&str] = &[
    "--allow-bind",
    "127.0.0.1:0",
    "--allow-connect",
    "127.0.0.1:*",
];

#[test]
fn sockets_are_denied_by_default_and_grants_are_exact() {
    let source = r#"import "std:net"
import "std:types"
fn code_of(result: Result[TcpConn, StdError]) -> Text:
    match result:
        Ok(conn):
            return "ok"
        Err(problem):
            return problem.code
fn main() -> Int:
    assert(code_of(tcp_connect("127.0.0.1", 9, 500)) == "E_NET_DENIED")
    match tcp_listen("127.0.0.1", 0):
        Ok(listener):
            return 1
        Err(problem):
            assert(problem.code == "E_NET_DENIED")
            assert(text_find(problem.message, "--allow-bind", 0) >= 0)
    return 0
"#;
    assert_ok(&run_project("net-deny", "", &[], source));
    // Binding is granted but connecting to another port or host is not.
    let source = r#"import "std:net"
import "std:types"
fn main() -> Int:
    let listener = unwrap(tcp_listen("127.0.0.1", 0))
    assert(not is_ok(tcp_connect("127.0.0.1", 9, 500)))
    assert(not is_ok(tcp_connect("localhost", 9, 500)))
    assert(not is_ok(tcp_listen("127.0.0.1", 8080)))
    return 0
"#;
    assert_ok(&run_project(
        "net-exact",
        "",
        &[
            "--allow-bind",
            "127.0.0.1:0",
            "--allow-connect",
            "127.0.0.1:1",
        ],
        source,
    ));
}

#[test]
fn tcp_loopback_echoes_bytes_reports_eof_and_times_out() {
    let source = r#"import "std:net"
import "std:types"
fn main() -> Int:
    let listener = unwrap(tcp_listen("127.0.0.1", 0))
    let port = unwrap(tcp_listener_port(listener))
    let client = unwrap(tcp_connect("127.0.0.1", port, 2000))
    let server = unwrap(tcp_accept(listener, 2000))
    assert(unwrap(tcp_send_text(client, "ping")) == 4)
    assert(unwrap(tcp_recv(server, 64, 2000)) == bytes_from_text("ping"))
    assert(unwrap(tcp_send(server, bytes_from_ints([0, 255, 7]))) == 3)
    assert(unwrap(tcp_recv(client, 64, 2000)) == bytes_from_ints([0, 255, 7]))
    match tcp_recv(client, 64, 100):
        Ok(data):
            return 1
        Err(problem):
            assert(problem.code == "E_NET_TIMEOUT")
    assert(is_ok(tcp_close(server)))
    assert(len(unwrap(tcp_recv(client, 64, 2000))) == 0)
    assert(is_ok(tcp_close(client)))
    assert(not is_ok(tcp_close(client)))
    match tcp_accept(listener, 100):
        Ok(conn):
            return 2
        Err(problem):
            assert(problem.code == "E_NET_TIMEOUT")
    assert(is_ok(tcp_listener_close(listener)))
    return 0
"#;
    let started = std::time::Instant::now();
    assert_ok(&run_project("net-tcp", "", LOOPBACK, source));
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
}

#[test]
fn refused_connections_bad_arguments_and_wrong_handles_return_stable_codes() {
    let source = r#"import "std:net"
import "std:types"
fn code_of(result: Result[TcpConn, StdError]) -> Text:
    match result:
        Ok(conn):
            return "ok"
        Err(problem):
            return problem.code
fn main() -> Int:
    let listener = unwrap(tcp_listen("127.0.0.1", 0))
    let port = unwrap(tcp_listener_port(listener))
    assert(is_ok(tcp_listener_close(listener)))
    assert(code_of(tcp_connect("127.0.0.1", port, 2000)) == "E_NET_REFUSED")
    assert(code_of(tcp_connect("127.0.0.1", 70000, 2000)) == "E_NET_ARGUMENT")
    assert(code_of(tcp_connect("127.0.0.1", port, 0)) == "E_NET_ARGUMENT")
    assert(code_of(tcp_connect("127.0.0.1", port, 999999)) == "E_NET_ARGUMENT")
    let ghost = TcpConn(12345)
    match tcp_recv(ghost, 16, 100):
        Ok(data):
            return 1
        Err(problem):
            assert(problem.code == "E_NET_HANDLE")
    match tcp_recv(ghost, 0, 100):
        Ok(data):
            return 2
        Err(problem):
            assert(problem.code == "E_NET_ARGUMENT")
    return 0
"#;
    assert_ok(&run_project("net-errors", "", LOOPBACK, source));
}

#[test]
fn udp_loopback_round_trips_datagrams_with_sender_addresses() {
    let source = r#"import "std:net"
import "std:types"
fn main() -> Int:
    let first = unwrap(udp_bind("127.0.0.1", 0))
    let second = unwrap(udp_bind("127.0.0.1", 0))
    let first_port = unwrap(udp_port(first))
    let second_port = unwrap(udp_port(second))
    assert(unwrap(udp_send_to(first, "127.0.0.1", second_port, bytes_from_text("hello"))) == 5)
    let received = unwrap(udp_recv_from(second, 64, 2000))
    assert(received.data == bytes_from_text("hello"))
    assert(received.host == "127.0.0.1")
    assert(received.port == first_port)
    match udp_recv_from(first, 64, 100):
        Ok(datagram):
            return 1
        Err(problem):
            assert(problem.code == "E_NET_TIMEOUT")
    assert(is_ok(udp_close(first)))
    assert(is_ok(udp_close(second)))
    return 0
"#;
    assert_ok(&run_project("net-udp", "", LOOPBACK, source));
}

#[test]
fn manifest_grants_work_and_the_socket_limit_is_enforced() {
    let source = r#"import "std:net"
import "std:types"
fn main() -> Int:
    var held: List[UdpSock] = []
    var count = 0
    while count < 64:
        set held = list_push(held, unwrap(udp_bind("127.0.0.1", 0)))
        set count = count + 1
    match udp_bind("127.0.0.1", 0):
        Ok(extra):
            return 1
        Err(problem):
            assert(problem.code == "E_NET_LIMIT")
    return 0
"#;
    assert_ok(&run_project(
        "net-limit",
        "[sockets]\nbind = [\"127.0.0.1:0\"]\n",
        &[],
        source,
    ));
}

#[test]
fn inspection_reports_the_socket_effect_and_grants() {
    let dir = std::env::temp_dir().join(format!("guion-net-inspect-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("main.gu"),
        "import \"std:net\"\nfn main() -> Int:\n    let ignored = tcp_connect(\"127.0.0.1\", 80, 100)\n    return 0\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("guion.toml"),
        "[sockets]\nconnect = [\"127.0.0.1:80\"]\n",
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["inspect", "--json", "main.gu"])
        .current_dir(&dir)
        .output()
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert_ok(&output);
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(report["required_effects"]
        .to_string()
        .contains("network.socket"));
    assert_eq!(report["socket_grant"]["required_for_entry"], true);
    assert_eq!(
        report["manifest_grants"]["socket_connect"],
        serde_json::json!(["127.0.0.1:80"])
    );
}

// Handles opened by the main program are usable from `parallel_map` workers, and handles opened
// in one worker are usable from another: two workers accept on the shared listener while two
// others connect to it, and every connection gets its own reply.
#[test]
fn socket_handles_are_shared_between_parallel_workers() {
    let source = r#"import "std:net"
import "std:types"

# An item packs a role (0 server, 1 client), the listener handle and its port.
fn serve(listener_handle: Int) -> Int:
    let listener = TcpListener(listener_handle)
    let conn = unwrap(tcp_accept(listener, 5000))
    let request = unwrap(tcp_recv(conn, 64, 5000))
    assert(is_ok(tcp_send(conn, request)))
    assert(is_ok(tcp_close(conn)))
    return 1

fn call(port: Int) -> Int:
    let conn = unwrap(tcp_connect("127.0.0.1", port, 5000))
    assert(is_ok(tcp_send_text(conn, "ping")))
    assert(unwrap(tcp_recv(conn, 64, 5000)) == bytes_from_text("ping"))
    assert(is_ok(tcp_close(conn)))
    return 1

fn work(item: Int) -> Int:
    let role = item % 10
    let rest = item // 10
    if role == 0:
        return serve(rest)
    return call(rest)

fn main() -> Int:
    let listener = unwrap(tcp_listen("127.0.0.1", 0))
    let port = unwrap(tcp_listener_port(listener))
    let items = [listener.handle * 10, listener.handle * 10, port * 10 + 1, port * 10 + 1]
    match parallel_map(items, work, 4):
        Ok(done):
            assert(done == [1, 1, 1, 1])
        Err(problem):
            print(task_message(problem))
            return 1
    assert(is_ok(tcp_listener_close(listener)))
    return 0
"#;
    assert_ok(&run_project("net-shared", "", LOOPBACK, source));
}
