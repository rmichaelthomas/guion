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
        [
            "Route",
            "Router",
            "Middleware",
            "App",
            "SessionStore",
            "SessionPolicy",
            "Rendered"
        ]
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
        "html_escape_text",
        "html_escape_attr",
        "html_escape_url",
        "html_script_string",
        "html_embed_rendered",
        "html_finish",
        "html_response",
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
    // Sessions: the shape checks and the cookie builder touch nothing; everything that reads the time
    // reports the clock. (The effect analysis does not model SQLite yet, so opening the session
    // database reports no filesystem effect; the package README says so and the package declares
    // `files` in its capabilities.)
    for pure in [
        "session_id_valid",
        "session_id_new",
        "session_cookie",
        "session_policy_default",
        "session_policy_check",
        "session_id_of",
        "csrf_exempt_method",
    ] {
        assert!(effects(pure).is_empty(), "{pure}: {:?}", effects(pure));
    }
    for timed in [
        "session_open",
        "session_get",
        "session_set",
        "session_rotate",
        "session_middleware",
        "csrf_token",
        "csrf_middleware",
    ] {
        assert!(effects(timed).contains(&"clock.read".to_owned()), "{timed}");
    }
    // The explicit-clock forms take the time as an argument.
    assert!(!effects("session_get_at").contains(&"clock.read".to_owned()));
    assert!(!effects("session_open_at").contains(&"clock.read".to_owned()));
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

fn stamp(config: Map[Text, Text], request: Request, response: Response) -> Result[Response, HttpFailure]:
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
    start_app(name, workers, APP, &[])
}

fn start_app(name: &str, workers: u32, source: &str, grants: &[&str]) -> Server {
    let dir = std::env::temp_dir().join(format!("guion-web-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("guion.toml"),
        "[project]\nname = \"app\"\nversion = \"0.1.0\"\nentry = \"src/main.gu\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("src/main.gu"), source).unwrap();
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
            .args(["run", "--no-cache", "--allow-bind", &bind])
            .args(grants)
            .arg("src/main.gu")
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

#[test]
fn csrf_tokens_are_compared_only_with_the_constant_time_mac_check() {
    // The runtime's check is `Mac::verify_slice`, which compares in constant time. The package must
    // reach it for every token and never compare a token or a MAC with `==` or `!=`. Timing cannot be
    // measured reliably in a test, so this pins the code path instead.
    let source = std::fs::read_to_string(web_dir().join("src/csrf.gu")).unwrap();
    assert!(
        source.contains("crypto_verify_hmac_sha256(store.secret"),
        "no MAC check"
    );
    for (number, line) in source.lines().enumerate() {
        if line.trim_start().starts_with('#') {
            continue;
        }
        for forbidden in [
            "token ==", "token !=", "given ==", "given !=", "== token", "!= token", "== given",
        ] {
            assert!(
                !line.contains(forbidden),
                "csrf.gu:{}: `{forbidden}` compares a token in variable time: {line}",
                number + 1
            );
        }
    }
    let runtime = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../runtime/src/stdlib_services.rs"),
    )
    .unwrap();
    assert!(
        runtime.contains("mac.verify_slice(expected)"),
        "the runtime check changed"
    );
}

#[test]
fn no_session_function_writes_to_a_log_or_the_console() {
    let source = std::fs::read_to_string(web_dir().join("src/session.gu")).unwrap()
        + &std::fs::read_to_string(web_dir().join("src/csrf.gu")).unwrap();
    for line in source.lines() {
        if line.trim_start().starts_with('#') {
            continue;
        }
        for forbidden in [
            "print(",
            "eprint(",
            "log_info",
            "log_debug",
            "log_warn",
            "log_error",
        ] {
            assert!(!line.contains(forbidden), "{forbidden} in {line}");
        }
    }
}

mod pinned {
    //! The embedding-only entropy and clock pins make the session code deterministic for tests. The
    //! pin is a Rust field of `HostPolicy`: nothing here is reachable from a Guion program.

    use guion_model::Value;
    use guion_runtime::{EntropySource, HostPolicy, Runtime};
    use std::path::{Path, PathBuf};

    const PROGRAM: &str = r#"import "src/lib.gu"
import "std:crypto"
import "std:encoding"
import "std:types"

fn main() -> Int:
    return 0

fn flow() -> Text:
    let store = unwrap(session_store_open("pinned.db"))
    let policy = session_policy_default()
    let id = unwrap(session_open(store, policy, request_new("GET", "/")))
    let cookie = Header("Cookie", policy.cookie_name + "=" + id)
    let request = Request("GET", "/", {}, [cookie], bytes_from_text(""), {})
    let token = unwrap(csrf_token(store, policy, request))
    return hex_encode(store.secret) + "|" + id + "|" + hex_encode(unwrap(base64url_decode(token)))

fn after_hours(hours: Int) -> Bool:
    let store = unwrap(session_store_open("pinned.db"))
    let policy = session_policy_default()
    let id = unwrap(session_open_at(store, policy, request_new("GET", "/"), 1000))
    let cookie = Header("Cookie", policy.cookie_name + "=" + id)
    let request = Request("GET", "/", {}, [cookie], bytes_from_text(""), {})
    return unwrap(session_open_at(store, policy, request, 1000 + hours * 3600000)) == id
"#;

    fn scratch(name: &str) -> PathBuf {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "guion-web-pin-{name}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src")).unwrap();
        let web = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packages/web/src");
        for entry in std::fs::read_dir(web).unwrap() {
            let path = entry.unwrap().path();
            std::fs::copy(&path, root.join("src").join(path.file_name().unwrap())).unwrap();
        }
        std::fs::write(root.join("main.gu"), PROGRAM).unwrap();
        root
    }

    fn instance(root: &Path, policy: HostPolicy) -> guion_runtime::Instance {
        let stdlib = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../language/stdlib");
        let artifact = guion_compiler::compile_project_with_roots(
            &root.join("main.gu"),
            root,
            Some(&stdlib),
            &std::collections::HashMap::new(),
        )
        .unwrap_or_else(|error| panic!("{} {}", error.code, error.message));
        Runtime::vm_only()
            .load(artifact, policy.with_file_access(root, true, true))
            .expect("artifact loads")
    }

    fn text(value: Value) -> String {
        match value {
            Value::Text(text) => text.as_ref().to_owned(),
            other => panic!("expected Text, got {other:?}"),
        }
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    fn flow(root: &Path, policy: HostPolicy) -> Vec<String> {
        let mut vm = instance(root, policy);
        let value = vm.invoke("flow", vec![]).expect("flow runs");
        text(value).split('|').map(str::to_owned).collect()
    }

    #[test]
    fn a_pinned_source_makes_the_secret_the_id_and_the_token_known_values() {
        let pinned: Vec<u8> = (0u8..64).collect();
        let policy = || {
            HostPolicy::default()
                .with_entropy_source(EntropySource::fixed(pinned.clone()))
                .with_clock_unix_time_ms(1_000_000)
        };
        let first_root = scratch("first");
        let first = flow(&first_root, policy());
        // The secret is the first 32 pinned bytes and the ID the next 32.
        assert_eq!(first[0], hex(&pinned[..32]));
        assert_eq!(first[1], hex(&pinned[32..]));
        // The token is HMAC-SHA-256(secret, id + "csrf"), checked against an independent HMAC.
        let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, &pinned[..32]);
        let expected = ring::hmac::sign(&key, format!("{}csrf", first[1]).as_bytes());
        assert_eq!(first[2], hex(expected.as_ref()));
        // Same pin, same clock, a fresh database: the same values (determinism, for tests only).
        let second_root = scratch("second");
        assert_eq!(flow(&second_root, policy()), first);
        let _ = std::fs::remove_dir_all(first_root);
        let _ = std::fs::remove_dir_all(second_root);
    }

    #[test]
    fn without_a_pin_the_secret_and_the_id_come_from_the_os_and_differ_every_run() {
        let a_root = scratch("os-a");
        let b_root = scratch("os-b");
        let a = flow(
            &a_root,
            HostPolicy::default().with_clock_unix_time_ms(1_000_000),
        );
        let b = flow(
            &b_root,
            HostPolicy::default().with_clock_unix_time_ms(1_000_000),
        );
        assert_eq!(a[0].len(), 64);
        assert_eq!(a[1].len(), 64);
        assert_ne!(a[0], b[0]);
        assert_ne!(a[1], b[1]);
        assert_ne!(a[2], b[2]);
        assert_ne!(a[0], a[1], "the secret and the ID are separate draws");
        let _ = std::fs::remove_dir_all(a_root);
        let _ = std::fs::remove_dir_all(b_root);
    }

    #[test]
    fn the_idle_and_absolute_limits_follow_the_clock_the_host_gives() {
        let root = scratch("limits");
        let mut vm = instance(&root, HostPolicy::default());
        let hours = |vm: &mut guion_runtime::Instance, hours: i64| {
            vm.invoke(
                "after_hours",
                vec![Value::Int(guion_model::BigInt::from_i64(hours))],
            )
            .expect("runs")
        };
        // Default policy: idle 30 minutes. One hour later the ID is not a live session any more.
        assert_eq!(hours(&mut vm, 0), Value::Bool(true));
        assert_eq!(hours(&mut vm, 1), Value::Bool(false));
        let _ = std::fs::remove_dir_all(root);
    }
}

const SESSION_APP: &str = r#"import "pkg:web/src/lib.gu"
import "std:text"

fn db() -> Text:
    return "sessions.db"

fn form(request: Request) -> Result[Response, HttpFailure]:
    let store = session_store_open(db())?
    let token = csrf_token(store, session_policy_default(), request)?
    return Ok(response_text(200, token))

fn item(request: Request) -> Result[Response, HttpFailure]:
    return Ok(response_text(200, "saved"))

fn login(request: Request) -> Result[Response, HttpFailure]:
    let store = session_store_open(db())?
    return session_rotate(store, session_policy_default(), request, response_text(200, "welcome"))

fn parse_or(text: Text, fallback: Int) -> Int:
    let parsed = text_parse_int(text)
    if is_ok(parsed):
        return unwrap(parsed)
    return fallback

fn main() -> Int:
    let arguments = args()
    let port = parse_or(arguments[0], 8099)
    let workers = parse_or(arguments[1], 1)
    let table = unwrap(router([route("GET", "/form", form), route("HEAD", "/form", form), route("OPTIONS", "/item", item), route("POST", "/item", item), route("PUT", "/item", item), route("PATCH", "/item", item), route("DELETE", "/item", item), route("POST", "/login", login)]))
    let store = unwrap(session_store_open(db()))
    let policy = session_policy_default()
    let layers = [unwrap(session_middleware(store, policy)), unwrap(csrf_middleware(store, policy))]
    match serve_for("127.0.0.1", port, app_with(table, layers), workers, 1500):
        Ok(done):
            print("served")
            return 0
        Err(failure):
            print("failed " + failure.code)
            return 1
"#;

fn header_value(response: &str, name: &str) -> Option<String> {
    let prefix = format!("{}: ", name.to_ascii_lowercase());
    response
        .split("\r\n\r\n")
        .next()?
        .lines()
        .find(|line| line.to_ascii_lowercase().starts_with(&prefix))
        .map(|line| line[prefix.len()..].to_owned())
}

fn body_of(response: &str) -> &str {
    response.split("\r\n\r\n").nth(1).unwrap_or("")
}

fn send(port: u16, method: &str, path: &str, cookie: &str, token: &str) -> String {
    let mut request = format!("{method} {path} HTTP/1.1\r\nHost: x\r\nContent-Length: 0\r\n");
    if !cookie.is_empty() {
        request.push_str(&format!("Cookie: {cookie}\r\n"));
    }
    if !token.is_empty() {
        request.push_str(&format!("X-CSRF-Token: {token}\r\n"));
    }
    request.push_str("\r\n");
    exchange(port, &request)
}

#[test]
fn sessions_and_csrf_over_a_real_socket_with_two_workers_sharing_one_store() {
    let server = start_app(
        "session",
        2,
        SESSION_APP,
        &["--allow-clock", "--file-root", "."],
    );
    let port = server.port;

    let page = send(port, "GET", "/form", "", "");
    assert!(page.starts_with("HTTP/1.1 200 OK"), "{page}");
    let set = header_value(&page, "set-cookie").expect("a session cookie");
    assert!(set.starts_with("__Host-session="), "{set}");
    for flag in ["; HttpOnly", "; Secure", "; SameSite=Lax", "; Path=/"] {
        assert!(set.contains(flag), "{flag} missing: {set}");
    }
    assert!(!set.contains("Domain"), "{set}");
    let cookie = set.split(';').next().unwrap().to_owned();
    let id = cookie.split('=').nth(1).unwrap().to_owned();
    assert_eq!(id.len(), 64, "{id}");
    let token = body_of(&page).to_owned();
    assert_eq!(token.len(), 44, "{token}");

    // Exempt methods need no token; the others are refused without one and accepted with it.
    assert!(send(port, "HEAD", "/form", &cookie, "").starts_with("HTTP/1.1 200"));
    assert!(send(port, "OPTIONS", "/item", &cookie, "").starts_with("HTTP/1.1 200"));
    for method in ["POST", "PUT", "PATCH", "DELETE"] {
        let refused = send(port, method, "/item", &cookie, "");
        assert!(refused.starts_with("HTTP/1.1 403"), "{method}: {refused}");
        assert_eq!(body_of(&refused), "Forbidden");
        let accepted = send(port, method, "/item", &cookie, &token);
        assert!(accepted.starts_with("HTTP/1.1 200"), "{method}: {accepted}");
    }

    // A second visitor has another session and another token; neither works in the other session.
    let other_page = send(port, "GET", "/form", "", "");
    let other_cookie = header_value(&other_page, "set-cookie")
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let other_token = body_of(&other_page).to_owned();
    assert_ne!(other_cookie, cookie);
    assert_ne!(other_token, token);
    let crossed = send(port, "POST", "/item", &cookie, &other_token);
    assert!(crossed.starts_with("HTTP/1.1 403"), "{crossed}");

    // Fixation: a planted ID the server never issued is replaced, never adopted.
    let planted = format!("__Host-session={}", "a".repeat(64));
    let replaced = send(port, "GET", "/form", &planted, "");
    let reissued = header_value(&replaced, "set-cookie").unwrap();
    assert!(!reissued.contains(&"a".repeat(64)), "{reissued}");
    // A cookie of the wrong length or alphabet is replaced the same way.
    for bad in [
        format!("__Host-session={}", "a".repeat(63)),
        format!("__Host-session={}G", "a".repeat(63)),
        format!("__Host-session={id}0"),
    ] {
        let answer = send(port, "GET", "/form", &bad, "");
        let issued = header_value(&answer, "set-cookie").unwrap();
        assert!(!issued.contains(&id), "{bad}: {issued}");
    }

    // Login rotates: the old cookie and its token die, the new ones work.
    let login = send(port, "POST", "/login", &cookie, &token);
    assert!(login.starts_with("HTTP/1.1 200"), "{login}");
    assert_eq!(
        login.to_ascii_lowercase().matches("set-cookie: ").count(),
        1,
        "{login}"
    );
    let rotated = header_value(&login, "set-cookie")
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    assert_ne!(rotated, cookie);
    let stale = send(port, "POST", "/item", &cookie, &token);
    assert!(stale.starts_with("HTTP/1.1 403"), "{stale}");
    let fresh_page = send(port, "GET", "/form", &rotated, "");
    let fresh_token = body_of(&fresh_page).to_owned();
    assert_ne!(fresh_token, token);
    let ok = send(port, "POST", "/item", &rotated, &fresh_token);
    assert!(ok.starts_with("HTTP/1.1 200"), "{ok}");
}
