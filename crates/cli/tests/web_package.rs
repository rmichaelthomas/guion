//! The `web` package (packages/web) through the real package mechanism: its own tests, its public
//! surface, and an application that depends on it by path and serves over a real socket.

mod common;

use common::{connect_to, free_port, output, spawn};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

fn web_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../packages/web")
        .canonicalize()
        .unwrap()
}

fn guion_in(dir: &Path, args: &[&str]) -> std::process::Output {
    output(
        Command::new(env!("CARGO_BIN_EXE_guion"))
            .args(args)
            .current_dir(dir),
    )
    .unwrap()
}

#[test]
fn the_web_package_passes_its_own_tests() {
    let result = guion_in(&web_dir(), &["test", "tests.gu"]);
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(result.status.success(), "{text}");
    assert!(text.contains("tests passed"), "{text}");
}

const HANDLE_TYPES: &[&str] = &[
    "TcpConn",
    "TcpListener",
    "UdpSock",
    "FileReader",
    "Db",
    "Transaction",
    "TomlDocument",
    "HttpServerRequest",
];

/// Names declared `pub fn` or `pub record` in the web package's own modules.
fn public_names(keyword: &str) -> Vec<String> {
    let mut names = Vec::new();
    let prefix = format!("pub {keyword} ");
    for entry in std::fs::read_dir(web_dir().join("src")).unwrap() {
        let source = std::fs::read_to_string(entry.unwrap().path()).unwrap();
        for line in source.lines() {
            if let Some(rest) = line.strip_prefix(&prefix) {
                let end = rest.find(['(', ':']).unwrap_or(rest.len());
                names.push(rest[..end].trim().to_owned());
            }
        }
    }
    names
}

fn web_doc() -> serde_json::Value {
    let result = guion_in(&web_dir(), &["doc", "--json", "tests.gu"]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    serde_json::from_slice(&result.stdout).unwrap()
}

#[test]
fn no_handle_in_public_signatures() {
    let report = web_doc();
    let functions = public_names("fn");
    assert!(functions.len() >= 15, "{functions:?}");
    for name in &functions {
        let function = report["functions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|function| function["name"] == name.as_str())
            .unwrap_or_else(|| panic!("{name} is documented"));
        for handle in HANDLE_TYPES {
            assert!(
                !function.to_string().contains(handle),
                "{name} names {handle}: {function}"
            );
        }
    }
    let records = public_names("record");
    assert!(
        ["Route", "Router", "Middleware", "App"]
            .iter()
            .all(|name| records.iter().any(|record| record == name)),
        "{records:?}"
    );
    for name in &records {
        let record = report["records"]
            .as_array()
            .unwrap()
            .iter()
            .find(|record| record["name"] == name.as_str())
            .unwrap_or_else(|| panic!("{name} is documented"));
        for handle in HANDLE_TYPES {
            assert!(
                !record.to_string().contains(handle),
                "{name} names {handle}: {record}"
            );
        }
    }
    // The one private record that holds the shared listener stays private.
    assert!(!records.iter().any(|record| record == "Worker"));
    assert!(!functions.iter().any(|name| name.starts_with("sv_")));
}

#[test]
fn every_public_function_declares_its_effects() {
    let report = web_doc();
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
    for name in public_names("fn") {
        let _ = effects(&name);
    }
    for pure in [
        "route",
        "router",
        "router_handle",
        "wrap",
        "app",
        "path_param",
        "path_param_int",
    ] {
        assert!(effects(pure).is_empty(), "{pure}: {:?}", effects(pure));
    }
    for serving in ["serve", "serve_for"] {
        let found = effects(serving);
        assert!(found.contains(&"network.socket".to_owned()), "{found:?}");
        assert!(
            found.contains(&"concurrency.parallel".to_owned()),
            "{found:?}"
        );
    }
    assert!(effects("app_respond").contains(&"filesystem.write".to_owned()));
}

const APP: &str = r#"import "pkg:web/src/lib.gu"
import "std:text"

fn user(request: Request) -> Result[Response, HttpFailure]:
    return Ok(response_text(200, "user " + int_to_text(unwrap(path_param_int(request, "id")))))

fn slug(request: Request) -> Result[Response, HttpFailure]:
    return Ok(response_text(200, "slug " + unwrap(path_param(request, "slug"))))

fn boom(request: Request) -> Result[Response, HttpFailure]:
    return Err(HttpFailure(404, "DB_SECRET", "database password hunter2"))

fn cookies(request: Request) -> Result[Response, HttpFailure]:
    let first = response_add_header(response_text(200, "ok"), "Set-Cookie", "a=1")?
    let second = response_add_header(first, "Set-Cookie", "b=2")?
    return response_add_header(second, "Set-Cookie", "c=3")

fn slow(request: Request) -> Result[Response, HttpFailure]:
    var count = 0
    while count < 2000000:
        set count = count + 1
    return Ok(response_text(200, "slow done"))

fn stamp(request: Request, response: Response) -> Result[Response, HttpFailure]:
    return response_add_header(response, "X-App", "web")

fn parse_or(text: Text, fallback: Int) -> Int:
    let parsed = text_parse_int(text)
    if is_ok(parsed):
        return unwrap(parsed)
    return fallback

fn main() -> Int:
    let arguments = args()
    let port = parse_or(arguments[0], 8099)
    let workers = parse_or(arguments[1], 1)
    let table = unwrap(router([route("GET", "/users/{id:int}", user), route("GET", "/files/{slug}", slug), route("GET", "/boom", boom), route("GET", "/cookies", cookies), route("GET", "/slow", slow)]))
    let application = wrap(app(table), middleware_after(stamp))
    match serve_for("127.0.0.1", port, application, workers, 1500):
        Ok(done):
            print("served")
            return 0
        Err(failure):
            print("failed " + failure.code)
            return 1
"#;

struct Server {
    child: Child,
    port: u16,
    dir: PathBuf,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn start(name: &str, workers: u32) -> Server {
    let dir = std::env::temp_dir().join(format!("guion-web-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("guion.toml"),
        "[project]\nname = \"app\"\nversion = \"0.1.0\"\nentry = \"src/main.gu\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("src/main.gu"), APP).unwrap();
    let added = guion_in(&dir, &["add", "web", "--path", web_dir().to_str().unwrap()]);
    assert!(
        added.status.success(),
        "{}{}",
        String::from_utf8_lossy(&added.stdout),
        String::from_utf8_lossy(&added.stderr)
    );
    let port = free_port();
    let bind = format!("127.0.0.1:{port}");
    let child = spawn(
        Command::new(env!("CARGO_BIN_EXE_guion"))
            .args(["run", "--no-cache", "--allow-bind", &bind, "src/main.gu"])
            .arg(port.to_string())
            .arg(workers.to_string())
            .current_dir(&dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    )
    .expect("guion starts");
    let server = Server { child, port, dir };
    for _ in 0..100 {
        if connect_to(port).is_ok() {
            // The probe connection is closed at once; the worker that took it reads EOF.
            return server;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("the server never listened");
}

fn exchange(port: u16, request: &str) -> String {
    let mut stream = connect_to(port).expect("connects");
    stream
        .set_read_timeout(Some(Duration::from_secs(8)))
        .unwrap();
    stream.write_all(request.as_bytes()).unwrap();
    let mut answer = Vec::new();
    let _ = stream.read_to_end(&mut answer);
    String::from_utf8_lossy(&answer).into_owned()
}

fn get(port: u16, path: &str) -> String {
    exchange(port, &format!("GET {path} HTTP/1.1\r\nHost: x\r\n\r\n"))
}

#[test]
fn an_application_using_web_by_path_serves_over_a_real_socket() {
    let server = start("routes", 2);
    let port = server.port;

    let user = get(port, "/users/42");
    assert!(user.starts_with("HTTP/1.1 200 OK"), "{user}");
    assert!(user.ends_with("user 42"), "{user}");
    assert!(user.contains("X-App: web"), "{user}");

    let not_int = get(port, "/users/abc");
    assert!(not_int.starts_with("HTTP/1.1 404"), "{not_int}");
    assert!(
        not_int.contains("X-App: web"),
        "middleware wraps the 404: {not_int}"
    );

    let encoded = get(port, "/files/a%2Fb");
    assert!(encoded.ends_with("slug a/b"), "{encoded}");
    assert!(get(port, "/files/a/b").starts_with("HTTP/1.1 404"));

    let boom = get(port, "/boom");
    assert!(boom.starts_with("HTTP/1.1 500"), "{boom}");
    assert!(boom.ends_with("Internal Server Error"), "{boom}");
    assert!(
        !boom.contains("hunter2") && !boom.contains("DB_SECRET"),
        "{boom}"
    );

    let cookies = get(port, "/cookies");
    assert_eq!(cookies.matches("Set-Cookie: ").count(), 3, "{cookies}");

    let wrong_method = exchange(port, "DELETE /users/1 HTTP/1.1\r\nHost: x\r\n\r\n");
    assert!(wrong_method.starts_with("HTTP/1.1 405"), "{wrong_method}");
    assert!(wrong_method.contains("Allow: GET"), "{wrong_method}");

    // A malformed request line is answered by std:http_server before any handler runs.
    let garbage = exchange(port, "NOT HTTP\r\n\r\n");
    assert!(garbage.starts_with("HTTP/1.1 400"), "{garbage}");

    // Once idle the workers stop and serve_for returns Ok.
    let mut server = server;
    let status = server.child.wait().unwrap();
    let mut out = String::new();
    server
        .child
        .stdout
        .take()
        .unwrap()
        .read_to_string(&mut out)
        .unwrap();
    assert!(
        status.success() && out.trim() == "served",
        "{status:?} {out}"
    );
}

#[test]
fn a_client_that_disconnects_before_the_answer_does_not_stop_the_server() {
    let server = start("disconnect", 1);
    let port = server.port;
    for _ in 0..3 {
        let mut stream: TcpStream = connect_to(port).expect("connects");
        stream
            .write_all(b"GET /slow HTTP/1.1\r\nHost: x\r\n\r\n")
            .unwrap();
        drop(stream);
    }
    let after = get(port, "/users/7");
    assert!(
        after.starts_with("HTTP/1.1 200 OK") && after.ends_with("user 7"),
        "{after}"
    );
}

#[test]
fn a_stalled_client_holds_one_worker_not_the_server() {
    let server = start("stall", 2);
    let port = server.port;
    let mut hold = connect_to(port).expect("connects");
    hold.write_all(b"GET /hold HTTP/1.1\r\nHost: x\r\n")
        .unwrap();
    std::thread::sleep(Duration::from_millis(300));
    let began = std::time::Instant::now();
    let second = get(port, "/users/9");
    assert!(second.ends_with("user 9"), "{second}");
    assert!(
        began.elapsed() < Duration::from_secs(3),
        "{:?}",
        began.elapsed()
    );
    drop(hold);
}
