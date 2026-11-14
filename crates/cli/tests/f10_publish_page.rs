//! F10, the publish-one-page slice (evaluation/frameworks/publish-page): an application that depends on
//! the `web` package by path and is started as a real server. Every test copies the slice into a
//! scratch directory, runs `guion add web --path` there (the committed manifest names the package by a
//! relative path) and talks to the server over a socket on a port allocated for that test.
//!
//! The server is killed when its guard drops, every wait is bounded, and no test states or asserts a
//! throughput number: the worker pool multiplies waiting, not computing.

mod common;

use common::{connect_to, free_port, output, spawn};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

fn guion() -> &'static str {
    env!("CARGO_BIN_EXE_guion")
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn slice() -> PathBuf {
    root().join("evaluation/frameworks/publish-page")
}

// ---- a scratch copy of the slice ----------------------------------------------------------------

/// `dir` is the project; its parent is a directory only this test uses, so a listing of the parent
/// can be compared before and after.
struct Site {
    dir: PathBuf,
}

impl Drop for Site {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(self.dir.parent().unwrap());
    }
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().to_string_lossy().into_owned();
        // What running the application makes is not part of the slice.
        if [
            "guion.lock",
            ".guion",
            "pages.db",
            "sessions.db",
            "public",
            "dist",
            "uploads",
            "out",
            "__pycache__",
        ]
        .contains(&name.as_str())
            || name.ends_with(".gut.gu")
        {
            continue;
        }
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &to.join(&name));
        } else {
            std::fs::copy(entry.path(), to.join(&name)).unwrap();
        }
    }
}

impl Site {
    fn new(name: &str) -> Site {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let base = std::env::temp_dir().join(format!(
            "guion-f10-{name}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&base);
        let dir = base.join("site");
        copy_tree(&slice(), &dir);
        let site = Site { dir };
        let added = site.guion(&[
            "add",
            "web",
            "--path",
            root().join("packages/web").to_str().unwrap(),
        ]);
        assert!(added.status.success(), "{}", text_of(&added));
        site
    }

    fn guion(&self, args: &[&str]) -> std::process::Output {
        output(Command::new(guion()).args(args).current_dir(&self.dir)).unwrap()
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.dir.join(relative)
    }

    fn listing(&self, relative: &str) -> Vec<String> {
        names(&self.path(relative))
    }
}

fn names(directory: &Path) -> Vec<String> {
    let mut found: Vec<String> = std::fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    found.sort();
    found
}

fn text_of(result: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    )
}

// ---- a running server ---------------------------------------------------------------------------

struct Server {
    child: Child,
    port: u16,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn start(site: &Site, workers: u32) -> Server {
    let port = free_port();
    let bind = format!("127.0.0.1:{port}");
    let child = spawn(
        Command::new(guion())
            .args(["run", "--no-cache", "--allow-bind", &bind, "src/main.gu"])
            .arg(port.to_string())
            .arg(workers.to_string())
            .current_dir(&site.dir)
            .stdout(Stdio::null())
            .stderr(Stdio::null()),
    )
    .expect("guion starts");
    let mut server = Server { child, port };
    for _ in 0..200 {
        if let Some(status) = server.child.try_wait().unwrap() {
            panic!("the server stopped at start: {status:?}");
        }
        // The probe connection is closed at once; the worker that took it reads end of file.
        if connect_to(port).is_ok() {
            return server;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("the server never listened");
}

struct Reply {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Reply {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    fn headers_named(&self, name: &str) -> Vec<&str> {
        self.headers
            .iter()
            .filter(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
            .collect()
    }

    fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

fn parse(raw: &[u8]) -> Reply {
    let split = raw
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .unwrap_or_else(|| panic!("no header block in {:?}", String::from_utf8_lossy(raw)));
    let head = String::from_utf8_lossy(&raw[..split]).into_owned();
    let mut lines = head.split("\r\n");
    let status = lines
        .next()
        .unwrap()
        .split(' ')
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    let headers = lines
        .filter_map(|line| line.split_once(": "))
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect();
    Reply {
        status,
        headers,
        body: raw[split + 4..].to_vec(),
    }
}

/// Sends raw bytes and reads the answer to the end of the connection, for at most 15 seconds.
fn exchange(port: u16, raw: &[u8]) -> Reply {
    let mut stream = connect_to(port).expect("connects");
    stream
        .set_read_timeout(Some(Duration::from_secs(15)))
        .unwrap();
    // A server that refuses a large body early may reset the connection while it is still being
    // written; the answer is then read as far as it arrived.
    let _ = stream.write_all(raw);
    let mut answer = Vec::new();
    let _ = stream.read_to_end(&mut answer);
    parse(&answer)
}

fn request(port: u16, method: &str, path: &str, headers: &[(&str, &str)], body: &[u8]) -> Reply {
    let mut raw = format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\n").into_bytes();
    for (name, value) in headers {
        raw.extend_from_slice(format!("{name}: {value}\r\n").as_bytes());
    }
    if !body.is_empty() || method == "POST" {
        raw.extend_from_slice(format!("Content-Length: {}\r\n", body.len()).as_bytes());
    }
    raw.extend_from_slice(b"\r\n");
    raw.extend_from_slice(body);
    exchange(port, &raw)
}

/// A browser: the session cookie of its first request and the token that request was answered with.
struct Visitor {
    port: u16,
    cookie: String,
    token: String,
}

impl Visitor {
    fn arrive(port: u16) -> Visitor {
        let first = request(port, "GET", "/p/none", &[], b"");
        let set = first
            .header("set-cookie")
            .expect("a session cookie")
            .to_owned();
        Visitor {
            port,
            cookie: set.split(';').next().unwrap().to_owned(),
            token: first.header("x-csrf-token").expect("a token").to_owned(),
        }
    }

    fn get(&self, path: &str, extra: &[(&str, &str)]) -> Reply {
        let mut headers = vec![("Cookie", self.cookie.as_str())];
        headers.extend_from_slice(extra);
        request(self.port, "GET", path, &headers, b"")
    }

    fn post(&self, path: &str, kind: &str, body: &[u8], token: Option<&str>) -> Reply {
        let mut headers = vec![("Cookie", self.cookie.as_str()), ("Content-Type", kind)];
        if let Some(token) = token {
            headers.push(("x-csrf-token", token));
        }
        request(self.port, "POST", path, &headers, body)
    }

    fn create(&self, body: &[u8]) -> Reply {
        self.post(
            "/api/pages",
            "application/json",
            body,
            Some(&self.token.clone()),
        )
    }

    fn publish(&self, slug: &str) -> Reply {
        let body = format!("_csrf={}", self.token.replace('=', "%3D"));
        self.post(
            &format!("/p/{slug}/publish"),
            "application/x-www-form-urlencoded",
            body.as_bytes(),
            None,
        )
    }

    fn cover(&self, id: u32, filename: &str, kind: &str, data: &[u8]) -> Reply {
        let (content_type, body) = multipart("cover", filename, kind, data);
        self.post(
            &format!("/api/pages/{id}/cover"),
            &content_type,
            &body,
            Some(&self.token.clone()),
        )
    }
}

fn multipart(field: &str, filename: &str, kind: &str, data: &[u8]) -> (String, Vec<u8>) {
    let mut body = format!(
        "--XX\r\nContent-Disposition: form-data; name=\"{field}\"; filename=\"{filename}\"\r\nContent-Type: {kind}\r\n\r\n"
    )
    .into_bytes();
    body.extend_from_slice(data);
    body.extend_from_slice(b"\r\n--XX--\r\n");
    ("multipart/form-data; boundary=XX".to_owned(), body)
}

fn fixture(number: u32) -> Vec<u8> {
    std::fs::read(slice().join(format!("fixtures/page{number}.json"))).unwrap()
}

fn slug_of(number: u32) -> &'static str {
    ["field-notes", "river-walk", "hostile"][number as usize - 1]
}

/// The Python twin's rendering of a fixture, the whole page or only the article.
fn twin(number: u32, body_only: bool) -> Vec<u8> {
    let mut command = Command::new("python3");
    command
        .arg(slice().join("twin/render.py"))
        .arg(slice().join(format!("fixtures/page{number}.json")));
    if body_only {
        command.arg("--body");
    }
    let out = output(&mut command).unwrap();
    assert!(out.status.success(), "{}", text_of(&out));
    out.stdout
}

// ---- a PNG to upload ----------------------------------------------------------------------------

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut out = (data.len() as u32).to_be_bytes().to_vec();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut checked = kind.to_vec();
    checked.extend_from_slice(data);
    out.extend_from_slice(&crc32(&checked).to_be_bytes());
    out
}

/// A grayscale PNG, `width` by `height`, with uncompressed (stored) deflate blocks.
fn png(width: u32, height: u32) -> Vec<u8> {
    let mut raw = Vec::new();
    for _ in 0..height {
        raw.push(0);
        raw.extend((0..width).map(|x| (x * 255 / width) as u8));
    }
    let (mut a, mut b) = (1u32, 0u32);
    for byte in &raw {
        a = (a + u32::from(*byte)) % 65521;
        b = (b + a) % 65521;
    }
    let mut zlib = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = raw.chunks(65535).collect();
    for (index, block) in blocks.iter().enumerate() {
        zlib.push(u8::from(index + 1 == blocks.len()));
        zlib.extend_from_slice(&(block.len() as u16).to_le_bytes());
        zlib.extend_from_slice(&(!(block.len() as u16)).to_le_bytes());
        zlib.extend_from_slice(block);
    }
    zlib.extend_from_slice(&((b << 16) | a).to_be_bytes());
    let mut header = width.to_be_bytes().to_vec();
    header.extend_from_slice(&height.to_be_bytes());
    header.extend_from_slice(&[8, 0, 0, 0, 0]);
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    out.extend(chunk(b"IHDR", &header));
    out.extend(chunk(b"IDAT", &zlib));
    out.extend(chunk(b"IEND", b""));
    out
}

fn png_width(bytes: &[u8]) -> u32 {
    u32::from_be_bytes(bytes[16..20].try_into().unwrap())
}

fn have_sips() -> bool {
    let found = Command::new("sips")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok();
    if !found {
        println!(
            "NOTICE: sips is not installed; the cover resize checks are skipped on this machine"
        );
    }
    found
}

// ---- the slice's own checks ----------------------------------------------------------------------

#[test]
fn the_slice_passes_its_own_tests_and_checks() {
    let site = Site::new("own");
    let tests = site.guion(&["test", "tests.gu"]);
    let text = text_of(&tests);
    assert!(tests.status.success(), "{text}");
    assert!(text.contains("10 tests passed"), "{text}");

    let checks = output(
        Command::new("python3")
            .args(["run_checks.py", "--guion", guion()])
            .current_dir(&site.dir),
    )
    .unwrap();
    let text = text_of(&checks);
    println!("{text}");
    assert!(checks.status.success(), "{text}");
    assert!(text.contains("all checks passed"), "{text}");
    assert!(
        text.contains("page3.html: Guion and the Python twin agree"),
        "{text}"
    );
}

fn padded(site: &Site, total: usize) -> String {
    let counted = |text: &str| -> usize {
        text.lines()
            .find_map(|line| line.strip_prefix("source lines (src/, tests excluded): "))
            .and_then(|rest| rest.split(' ').next())
            .map(|number| number.parse().unwrap())
            .unwrap_or_else(|| panic!("no source line count in {text}"))
    };
    let run = || {
        let checks = output(
            Command::new("python3")
                .args(["run_checks.py", "--only", "size"])
                .current_dir(&site.dir),
        )
        .unwrap();
        text_of(&checks)
    };
    let _ = std::fs::remove_file(site.path("src/padding.gu"));
    let base = counted(&run());
    assert!(
        base < total,
        "the slice is {base} lines, over the cap already"
    );
    let filler = "# padding\n".repeat(total - base);
    std::fs::write(site.path("src/padding.gu"), filler).unwrap();
    run()
}

#[test]
fn the_size_cap_holds_at_700_lines_and_fails_at_701() {
    let site = Site::new("cap");
    let at_limit = padded(&site, 700);
    assert!(
        at_limit.contains("ok    application source is 700 lines"),
        "{at_limit}"
    );
    let over = padded(&site, 701);
    assert!(
        over.contains("FAIL  application source is 701 lines"),
        "{over}"
    );
    // The slice itself stays near the 600-line target of the plan.
    let plain = Site::new("cap-plain");
    let checks = output(
        Command::new("python3")
            .args(["run_checks.py", "--only", "size"])
            .current_dir(&plain.dir),
    )
    .unwrap();
    let text = text_of(&checks);
    println!("{text}");
    assert!(checks.status.success(), "{text}");
}

// ---- the six routes, over a socket --------------------------------------------------------------

#[test]
fn the_three_fixture_pages_match_the_python_twin_through_the_server() {
    let site = Site::new("twin");
    let server = start(&site, 4);
    let visitor = Visitor::arrive(server.port);
    for number in 1..=3u32 {
        let created = visitor.create(&fixture(number));
        assert_eq!(created.status, 201, "{}", created.text());
        assert_eq!(
            created.header("location"),
            Some(format!("/p/{}", slug_of(number)).as_str())
        );
    }
    for number in 1..=3u32 {
        let slug = slug_of(number);
        // The article in the served page is the twin's article, byte for byte.
        let shown = visitor.get(&format!("/p/{slug}"), &[]);
        assert_eq!(shown.status, 200);
        assert_eq!(
            shown.header("content-type"),
            Some("text/html; charset=utf-8")
        );
        let html = shown.body.clone();
        let article = twin(number, true);
        assert!(
            html.windows(article.len())
                .any(|window| window == article.as_slice()),
            "page {number}: the served article differs from the twin's\n{}\n--- twin ---\n{}",
            shown.text(),
            String::from_utf8_lossy(&article)
        );
        // The published bundle is the twin's whole page, byte for byte.
        let published = visitor.publish(slug);
        assert_eq!(published.status, 200, "{}", published.text());
        let bundle = std::fs::read(site.path(&format!("dist/{slug}/index.html"))).unwrap();
        assert_eq!(
            bundle,
            twin(number, false),
            "page {number}: the bundle differs from the twin's page"
        );
    }
    // The bundle holds no form and no token.
    let bundle = std::fs::read_to_string(site.path("dist/hostile/index.html")).unwrap();
    assert!(!bundle.contains("<form") && !bundle.contains("_csrf"));
}

#[test]
fn a_first_request_gets_a_session_and_the_page_carries_its_token() {
    let site = Site::new("session");
    let server = start(&site, 2);
    let first = request(server.port, "GET", "/p/none", &[], b"");
    assert_eq!(first.status, 404);
    let cookies = first.headers_named("set-cookie");
    assert_eq!(cookies.len(), 1, "{cookies:?}");
    let cookie = cookies[0];
    assert!(cookie.starts_with("__Host-session="), "{cookie}");
    let id = cookie["__Host-session=".len()..].split(';').next().unwrap();
    assert_eq!(id.len(), 64);
    assert!(
        id.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')),
        "{id}"
    );
    for attribute in ["HttpOnly", "Secure", "SameSite=Lax", "Path=/"] {
        assert!(cookie.contains(attribute), "{cookie}");
    }
    assert!(!cookie.contains("Domain"), "{cookie}");

    let visitor = Visitor::arrive(server.port);
    assert_eq!(visitor.create(&fixture(1)).status, 201);
    let shown = visitor.get("/p/field-notes", &[]);
    let html = shown.text();
    // The form's hidden field carries the token of this session, and so does the header.
    assert!(
        html.contains(&format!("name=\"_csrf\" value=\"{}\"", visitor.token)),
        "{html}"
    );
    assert_eq!(shown.header("x-csrf-token"), Some(visitor.token.as_str()));
    assert_eq!(shown.header("cache-control"), Some("no-store"));
    // Another visitor's token differs.
    assert_ne!(Visitor::arrive(server.port).token, visitor.token);
}

#[test]
fn a_write_without_the_token_is_refused_and_the_same_request_with_it_is_served() {
    let site = Site::new("csrf");
    let server = start(&site, 2);
    let visitor = Visitor::arrive(server.port);
    assert_eq!(visitor.create(&fixture(1)).status, 201);
    let form = "application/x-www-form-urlencoded";

    // POST /publish: no token, a wrong token, another session's token, then the token.
    let without = visitor.post("/p/field-notes/publish", form, b"x=1", None);
    assert_eq!(without.status, 403);
    assert_eq!(without.text(), "Forbidden");
    assert!(!site.path("dist/field-notes/index.html").exists());
    let mut wrong = visitor.token.clone().into_bytes();
    wrong[0] = if wrong[0] == b'A' { b'B' } else { b'A' };
    let wrong = String::from_utf8(wrong).unwrap();
    assert_eq!(
        visitor
            .post("/p/field-notes/publish", form, b"", Some(&wrong))
            .status,
        403
    );
    let stranger = Visitor::arrive(server.port);
    assert_eq!(
        visitor
            .post("/p/field-notes/publish", form, b"", Some(&stranger.token))
            .status,
        403,
        "a token belongs to one session"
    );
    assert!(!site.path("dist/field-notes/index.html").exists());
    let with = visitor.publish("field-notes");
    assert_eq!(with.status, 200, "{}", with.text());
    assert!(site.path("dist/field-notes/index.html").exists());

    // The JSON and multipart writes need it too; HEAD and OPTIONS do not; PUT, PATCH and DELETE do.
    assert_eq!(
        visitor
            .post("/api/pages", "application/json", &fixture(2), None)
            .status,
        403
    );
    assert_eq!(
        visitor
            .post(
                "/api/pages/1/cover",
                "multipart/form-data; boundary=XX",
                b"",
                None
            )
            .status,
        403
    );
    let headers = [("Cookie", visitor.cookie.as_str())];
    assert_ne!(
        request(server.port, "HEAD", "/p/field-notes", &headers, b"").status,
        403
    );
    assert_ne!(
        request(server.port, "OPTIONS", "/p/field-notes", &headers, b"").status,
        403
    );
    for method in ["PUT", "PATCH", "DELETE"] {
        assert_eq!(
            request(server.port, method, "/p/field-notes", &headers, b"x").status,
            403,
            "{method}"
        );
        let with_token = [headers[0], ("x-csrf-token", visitor.token.as_str())];
        assert_eq!(
            request(server.port, method, "/p/field-notes", &with_token, b"x").status,
            405,
            "{method}"
        );
    }
}

#[test]
fn an_attacker_supplied_session_id_is_replaced_not_adopted() {
    let site = Site::new("fixation");
    let server = start(&site, 2);
    let planted = "a".repeat(64);
    let planted_cookie = format!("__Host-session={planted}");
    let answer = request(
        server.port,
        "GET",
        "/p/none",
        &[("Cookie", planted_cookie.as_str())],
        b"",
    );
    let cookie = answer.header("set-cookie").expect("a new cookie");
    assert!(cookie.starts_with("__Host-session="), "{cookie}");
    assert!(
        !cookie.contains(&planted),
        "the planted ID was adopted: {cookie}"
    );
    // A write that presents the planted ID and a token someone derived for it is refused.
    let refused = request(
        server.port,
        "POST",
        "/api/pages",
        &[
            ("Cookie", planted_cookie.as_str()),
            ("Content-Type", "application/json"),
            ("x-csrf-token", answer.header("x-csrf-token").unwrap()),
        ],
        &fixture(1),
    );
    assert_eq!(refused.status, 403);
    // Malformed IDs get a fresh session, never an error.
    for bad in [
        "__Host-session=short",
        "__Host-session=ZZZZ",
        "__Host-session=",
        "other=1",
    ] {
        let answer = request(server.port, "GET", "/p/none", &[("Cookie", bad)], b"");
        assert_eq!(answer.status, 404, "{bad}");
        assert!(answer.header("set-cookie").is_some(), "{bad}");
    }
}

#[test]
fn hostile_inputs_are_rendered_inert_in_the_page_and_in_attributes() {
    let site = Site::new("xss");
    let server = start(&site, 2);
    let visitor = Visitor::arrive(server.port);
    assert_eq!(visitor.create(&fixture(3)).status, 201);
    let html = visitor.get("/p/hostile", &[]).text();
    assert!(!html.contains("<script"), "{html}");
    assert!(!html.contains("<img src=x"), "{html}");
    assert!(
        html.contains("<p>&lt;script&gt;alert(1)&lt;/script&gt;</p>"),
        "{html}"
    );
    assert!(
        html.contains("<p>&lt;/p&gt;&lt;img src=x onerror=alert(2)&gt;</p>"),
        "{html}"
    );
    // The alt text cannot leave its quotes.
    assert!(
        html.contains(
            "alt=\"&quot; onmouseover=&quot;alert(3)&quot; x=&quot;&#96;&#39; &lt;b&gt;&amp;amp;\""
        ),
        "{html}"
    );
    // Hostile slugs, names and shapes are refused by the schema before anything is stored.
    for body in [
        r#"{"slug": "<script>", "title": "t", "blocks": [{"type": "paragraph", "text": "x"}]}"#,
        r#"{"slug": "a", "title": "t", "blocks": [{"type": "html", "text": "<b>x</b>"}]}"#,
        r#"{"slug": "a", "title": "t", "blocks": [{"type": "image", "name": "\"onload=x", "alt": "a"}]}"#,
        r#"{"slug": "a", "title": "t", "blocks": [{"type": "image", "name": "../../x.png", "alt": "a"}]}"#,
        r#"{"slug": "a", "title": "t", "blocks": "nope"}"#,
        "[1,2,3]",
        "not json",
    ] {
        let refused = visitor.create(body.as_bytes());
        assert_eq!(refused.status, 422, "{body}: {}", refused.text());
    }
    assert_eq!(visitor.get("/p/a", &[]).status, 404);
    assert_eq!(visitor.get("/p/%3Cscript%3E", &[]).status, 404);
    // A slug that is the same twice is a conflict, not an overwrite.
    assert_eq!(visitor.create(&fixture(3)).status, 409);
}

#[test]
fn a_cover_is_confined_resized_and_served_with_validators_and_ranges() {
    if !have_sips() {
        return;
    }
    let site = Site::new("cover");
    let server = start(&site, 2);
    let visitor = Visitor::arrive(server.port);
    assert_eq!(visitor.create(&fixture(1)).status, 201);
    let image = png(800, 600);

    // Names that try to leave the directory: refused, and no directory changes.
    let parent = site.dir.parent().unwrap().to_path_buf();
    let outside_before = names(&parent);
    let root_before = site.listing(".");
    let uploads_before = site.listing("uploads");
    let public_before = site.listing("public");
    for name in [
        "../../etc/passwd",
        "..\\..\\etc\\passwd",
        "..",
        "....//....//x",
    ] {
        let refused = visitor.cover(1, name, "image/png", &image);
        assert_eq!(refused.status, 415, "{name:?}: {}", refused.text());
    }
    assert_eq!(
        visitor.cover(1, "a\u{0}.png", "image/png", &image).status,
        400,
        "a NUL in a name"
    );
    assert_eq!(visitor.cover(1, "a.png", "text/html", &image).status, 415);
    assert_eq!(
        visitor
            .cover(1, "a.png", "image/png", b"this is not an image")
            .status,
        415
    );
    assert_eq!(visitor.cover(99, "a.png", "image/png", &image).status, 404);
    assert_eq!(site.listing("uploads"), uploads_before);
    assert_eq!(site.listing("public"), public_before);
    assert_eq!(
        site.listing("."),
        root_before,
        "the parent of uploads changed"
    );
    assert_eq!(
        names(&parent),
        outside_before,
        "the parent of the project changed"
    );

    // A good cover whose name tries to climb out is stored under a random name and resized into public.
    let stored = visitor.cover(1, "../../evil.png", "image/png", &image);
    assert_eq!(stored.status, 200, "{}", stored.text());
    assert_eq!(stored.text(), r#"{"cover": "/static/cover-1.png"}"#);
    assert_eq!(names(&parent), outside_before);
    assert!(!parent.join("evil.png").exists() && !parent.join("etc").exists());
    assert_eq!(
        site.listing("uploads"),
        uploads_before,
        "the upload is removed after the resize"
    );
    assert_eq!(site.listing("public"), vec!["cover-1.png".to_owned()]);

    let file = visitor.get("/static/cover-1.png", &[]);
    assert_eq!(file.status, 200);
    assert_eq!(file.header("content-type"), Some("image/png"));
    assert!(
        png_width(&file.body) <= 512,
        "resized to {}",
        png_width(&file.body)
    );
    assert!(
        file.headers_named("set-cookie").is_empty(),
        "no session cookie on an asset"
    );
    let length = file.body.len();
    assert_eq!(file.header("accept-ranges"), Some("bytes"));

    let tag = file.header("etag").expect("an ETag").to_owned();
    let cached = visitor.get("/static/cover-1.png", &[("If-None-Match", tag.as_str())]);
    assert_eq!(cached.status, 304);
    assert!(cached.body.is_empty());
    let stale = visitor.get("/static/cover-1.png", &[("If-None-Match", "W/\"other\"")]);
    assert_eq!(stale.status, 200);

    let part = visitor.get("/static/cover-1.png", &[("Range", "bytes=0-99")]);
    assert_eq!(part.status, 206);
    assert_eq!(
        part.header("content-range"),
        Some(format!("bytes 0-99/{length}").as_str())
    );
    assert_eq!(part.body, file.body[..100]);
    let suffix = visitor.get("/static/cover-1.png", &[("Range", "bytes=-50")]);
    assert_eq!(suffix.status, 206);
    assert_eq!(suffix.body, file.body[length - 50..]);
    let single = visitor.get("/static/cover-1.png", &[("Range", "bytes=0-0")]);
    assert_eq!((single.status, single.body.len()), (206, 1));
    let beyond = visitor.get(
        "/static/cover-1.png",
        &[("Range", &format!("bytes={length}-"))],
    );
    assert_eq!(beyond.status, 416);
    assert_eq!(
        beyond.header("content-range"),
        Some(format!("bytes */{length}").as_str())
    );
    let head = request(
        server.port,
        "HEAD",
        "/static/cover-1.png",
        &[("Range", "bytes=0-9")],
        b"",
    );
    assert_eq!((head.status, head.body.len()), (206, 0));
    for path in [
        "/static/%2e%2e%2fguion.toml",
        "/static/..%2fguion.toml",
        "/static/../guion.toml",
        "/static/missing.png",
    ] {
        assert_eq!(visitor.get(path, &[]).status, 404, "{path}");
    }

    // The page shows the cover first, and a publish writes it into the bundle.
    let shown = visitor.get("/p/field-notes", &[]).text();
    assert!(
        shown.contains("<figure><img src=\"/static/cover-1.png\" alt=\"cover\"></figure>"),
        "{shown}"
    );

    // A symbolic link in public that points outside it is not followed.
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(site.path("guion.toml"), site.path("public/link.toml")).unwrap();
        assert_eq!(visitor.get("/static/link.toml", &[]).status, 404);
    }
}

#[test]
fn the_limits_of_the_slice_hold_at_the_limit_and_one_over() {
    let site = Site::new("limits");
    let server = start(&site, 2);
    let visitor = Visitor::arrive(server.port);
    let page = |slug: &str, title: &str, blocks: &str| {
        format!(r#"{{"slug": "{slug}", "title": "{title}", "blocks": [{blocks}]}}"#)
    };
    let paragraph = |text: &str| format!(r#"{{"type": "paragraph", "text": "{text}"}}"#);
    // Slug 64 / 65, title 200 / 201, paragraph 10000 / 10001, blocks 100 / 101.
    assert_eq!(
        visitor
            .create(page(&"a".repeat(64), "t", &paragraph("x")).as_bytes())
            .status,
        201
    );
    assert_eq!(
        visitor
            .create(page(&"a".repeat(65), "t", &paragraph("x")).as_bytes())
            .status,
        422
    );
    assert_eq!(
        visitor
            .create(page("t1", &"t".repeat(200), &paragraph("x")).as_bytes())
            .status,
        201
    );
    assert_eq!(
        visitor
            .create(page("t2", &"t".repeat(201), &paragraph("x")).as_bytes())
            .status,
        422
    );
    assert_eq!(
        visitor
            .create(page("p1", "t", &paragraph(&"p".repeat(10000))).as_bytes())
            .status,
        201
    );
    assert_eq!(
        visitor
            .create(page("p2", "t", &paragraph(&"p".repeat(10001))).as_bytes())
            .status,
        422
    );
    let many = |count: usize| vec![paragraph("x"); count].join(", ");
    assert_eq!(
        visitor
            .create(page("b1", "t", &many(100)).as_bytes())
            .status,
        201
    );
    assert_eq!(
        visitor
            .create(page("b2", "t", &many(101)).as_bytes())
            .status,
        422
    );
    assert_eq!(visitor.get("/p/b2", &[]).status, 404);
    assert_eq!(visitor.get("/p/b1", &[]).status, 200);

    // A request body of 1 MiB is read; one byte more is refused before any handler runs.
    let at_limit = visitor.post(
        "/api/pages",
        "application/json",
        &vec![b' '; 1_048_576],
        Some(&visitor.token.clone()),
    );
    assert_eq!(at_limit.status, 422, "{}", at_limit.text());
    let over = visitor.post(
        "/api/pages",
        "application/json",
        &vec![b' '; 1_048_577],
        Some(&visitor.token.clone()),
    );
    assert_eq!(over.status, 413, "{}", over.text());

    // A cover part of 600000 bytes passes the part limit (and is then refused as not an image); one
    // byte more is a 413, and neither leaves a file behind.
    let uploads = site.listing("uploads");
    let at_part = visitor.cover(1, "a.png", "image/png", &vec![b'a'; 600_000]);
    assert_ne!(at_part.status, 413, "{}", at_part.text());
    let over_part = visitor.cover(1, "a.png", "image/png", &vec![b'a'; 600_001]);
    assert_eq!(over_part.status, 413, "{}", over_part.text());
    assert_eq!(site.listing("uploads"), uploads);
}

// ---- concurrency --------------------------------------------------------------------------------

fn second_request_with_hold(workers: u32, window: Duration) -> (bool, Option<Duration>) {
    let site = Site::new("workers");
    let server = start(&site, workers);
    // A client that connects and sends half a request holds the worker that accepted it.
    let mut hold = connect_to(server.port).expect("connects");
    hold.write_all(b"GET /p/hold HTTP/1.1\r\nHost: x\r\n")
        .unwrap();
    std::thread::sleep(Duration::from_millis(300));
    let mut second = connect_to(server.port).expect("connects");
    second
        .write_all(b"GET /p/none HTTP/1.1\r\nHost: x\r\n\r\n")
        .unwrap();
    let began = Instant::now();
    second.set_read_timeout(Some(window)).unwrap();
    let mut buffer = [0u8; 16];
    let served_in_window = matches!(second.read(&mut buffer), Ok(n) if n > 0);
    if served_in_window {
        return (true, Some(began.elapsed()));
    }
    // Release the held request; the waiting one is then answered.
    drop(hold);
    second
        .set_read_timeout(Some(Duration::from_secs(15)))
        .unwrap();
    let served_later = matches!(second.read(&mut buffer), Ok(n) if n > 0);
    (false, served_later.then(|| began.elapsed()))
}

#[test]
fn workers_share_the_listener_so_a_stalled_client_does_not_block_another() {
    let window = Duration::from_secs(2);
    let (fast, at) = second_request_with_hold(4, window);
    assert!(
        fast,
        "four workers did not serve a second request while one was held"
    );
    println!(
        "F10 measure: second request served {:?} after the hold began (4 workers)",
        at.unwrap()
    );
    let (fast, at) = second_request_with_hold(1, window);
    assert!(
        !fast,
        "one worker served a second request while the first was held"
    );
    let at = at.expect("one worker never served the waiting request after the hold was released");
    println!("F10 measure: second request served {at:?} after being sent (1 worker, hold released at {window:?})");
    assert!(at >= window, "{at:?}");
}

struct Holder(Child);

impl Drop for Holder {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn a_page_read_completes_while_another_process_holds_a_write_transaction() {
    let site = Site::new("readers");
    let server = start(&site, 4);
    let visitor = Visitor::arrive(server.port);
    assert_eq!(visitor.create(&fixture(1)).status, 201);

    // A second process opens pages.db, takes the write lock and waits on a socket we keep open.
    let port = free_port();
    let bind = format!("127.0.0.1:{port}");
    let child = spawn(
        Command::new(guion())
            .args(["run", "--no-cache", "--allow-bind", &bind, "probe_hold.gu"])
            .arg(port.to_string())
            .current_dir(&site.dir)
            .stdout(Stdio::null())
            .stderr(Stdio::null()),
    )
    .unwrap();
    let mut holder = Holder(child);
    let mut held = None;
    for _ in 0..200 {
        if let Some(status) = holder.0.try_wait().unwrap() {
            panic!("the holder stopped: {status:?}");
        }
        if let Ok(conn) = connect_to(port) {
            held = Some(conn);
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let held: TcpStream = held.expect("the holder never listened");

    // The write lock is held: a page view still completes, quickly, through the server.
    let began = Instant::now();
    let shown = visitor.get("/p/field-notes", &[]);
    let read_took = began.elapsed();
    assert_eq!(shown.status, 200, "{}", shown.text());
    assert!(shown
        .text()
        .contains("<p>The first page has plain text only.</p>"));
    assert!(
        read_took < Duration::from_millis(1500),
        "the read waited {read_took:?}"
    );
    println!("F10 measure: GET /p/SLUG with a write transaction held took {read_took:?}");

    // The same read through sqlite_begin waits for the writer, then fails once the busy deadline passes.
    let began = Instant::now();
    let probe = output(
        Command::new(guion())
            .args([
                "run",
                "--no-cache",
                "--allow-clock",
                "probe_begin.gu",
                "field-notes",
            ])
            .current_dir(&site.dir),
    )
    .unwrap();
    let begin_took = began.elapsed();
    let said = text_of(&probe);
    assert!(said.starts_with("begin E_SQLITE_BEGIN "), "{said}");
    let waited: u64 = said.trim().rsplit(' ').next().unwrap().parse().unwrap();
    assert!(
        waited >= 1900,
        "sqlite_begin failed after only {waited} ms: {said}"
    );
    println!("F10 measure: sqlite_begin with a write transaction held failed with E_SQLITE_BEGIN after {waited} ms ({begin_took:?} with start-up)");

    // A write through the server queues behind the held transaction too, and fails the same way: a
    // fixed 500 that says nothing about the store.
    let began = Instant::now();
    let write = visitor.create(&fixture(2));
    assert_eq!(write.status, 500, "{}", write.text());
    assert_eq!(write.text(), "Internal Server Error");
    assert!(
        began.elapsed() >= Duration::from_millis(1900),
        "{:?}",
        began.elapsed()
    );

    // Released, writes go through again and the reader sees them.
    drop(held);
    let mut created = 0;
    for _ in 0..40 {
        created = visitor.create(&fixture(2)).status;
        if created == 201 {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(created, 201);
    assert_eq!(visitor.get("/p/river-walk", &[]).status, 200);
    let begin = output(
        Command::new(guion())
            .args([
                "run",
                "--no-cache",
                "--allow-clock",
                "probe_begin.gu",
                "river-walk",
            ])
            .current_dir(&site.dir),
    )
    .unwrap();
    assert!(
        text_of(&begin).starts_with("begin ok "),
        "{}",
        text_of(&begin)
    );
}

// ---- the server in numbers, printed rather than asserted ----------------------------------------

#[test]
fn the_server_resident_size_after_a_full_page_flow_is_printed() {
    let site = Site::new("rss");
    let mut server = start(&site, 4);
    let visitor = Visitor::arrive(server.port);
    let began = Instant::now();
    for number in 1..=3u32 {
        assert_eq!(visitor.create(&fixture(number)).status, 201);
        assert_eq!(
            visitor.get(&format!("/p/{}", slug_of(number)), &[]).status,
            200
        );
        assert_eq!(visitor.publish(slug_of(number)).status, 200);
    }
    println!(
        "F10 measure: create, view and publish of three pages took {:?} in all",
        began.elapsed()
    );
    let pid = server.child.id().to_string();
    let ps = output(Command::new("ps").args(["-o", "rss=", "-p", &pid])).unwrap();
    let rss: u64 = String::from_utf8_lossy(&ps.stdout)
        .trim()
        .parse()
        .unwrap_or(0);
    println!("F10 measure: server resident size after the flow {rss} KiB");
    assert!(rss > 0);
    assert!(
        server.child.try_wait().unwrap().is_none(),
        "the server is still running"
    );
}
