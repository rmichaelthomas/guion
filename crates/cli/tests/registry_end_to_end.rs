//! Exercises `guion publish`, `guion add NAME`, `guion yank` and the verification chain against
//! the reference registry (registry/) running locally under `wrangler dev --local` on loopback.
//! Nothing here touches the network beyond the local machine or any Cloudflare account.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};

const ADMIN_TOKEN: &str = "local-admin-token-for-tests";

struct LocalRegistry {
    child: Child,
    url: String,
    state: PathBuf,
}

impl Drop for LocalRegistry {
    fn drop(&mut self) {
        // SIGTERM lets wrangler stop its workerd children; a plain kill would orphan them.
        let _ = Command::new("kill")
            .args(["-TERM", &self.child.id().to_string()])
            .status();
        for _ in 0..50 {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.state);
    }
}

fn wrangler_available() -> bool {
    Command::new("wrangler")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn start_registry(name: &str) -> LocalRegistry {
    let state = std::env::temp_dir().join(format!("guion-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&state);
    std::fs::create_dir_all(&state).unwrap();
    let port = free_port();
    let registry_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../registry");
    let child = Command::new("wrangler")
        .args([
            "dev",
            "--local",
            "--port",
            &port.to_string(),
            "--persist-to",
        ])
        .arg(&state)
        .args(["--var", &format!("ADMIN_TOKEN:{ADMIN_TOKEN}")])
        .current_dir(registry_dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("wrangler starts");
    let url = format!("http://127.0.0.1:{port}");
    let registry = LocalRegistry { child, url, state };
    for _ in 0..120 {
        if curl(&["-s", "-m", "1", &format!("{}/", registry.url)]).contains("guion-registry") {
            return registry;
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    panic!("the local registry did not start");
}

fn curl(arguments: &[&str]) -> String {
    let output = Command::new("curl")
        .args(arguments)
        .output()
        .expect("curl runs");
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn new_publisher(registry: &LocalRegistry, id: &str) -> String {
    let body = curl(&[
        "-s",
        "-X",
        "POST",
        &format!("{}/v1/admin/publishers", registry.url),
        "-H",
        &format!("authorization: Bearer {ADMIN_TOKEN}"),
        "-d",
        &format!("{{\"id\":\"{id}\"}}"),
    ]);
    let value: serde_json::Value = serde_json::from_str(&body).unwrap();
    value["token"].as_str().unwrap().to_owned()
}

fn guion(
    directory: &Path,
    registry: &LocalRegistry,
    token: Option<&str>,
    arguments: &[&str],
) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_guion"));
    command
        .args(arguments)
        .current_dir(directory)
        .env("GUION_REGISTRY_URL", &registry.url)
        .env_remove("GUION_REGISTRY_TOKEN");
    if let Some(token) = token {
        command.env("GUION_REGISTRY_TOKEN", token);
    }
    command.output().expect("guion starts")
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn write_library(root: &Path, name: &str, version: &str, capabilities: &str, body: &str) {
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("guion.toml"),
        format!("[package]\nname = \"{name}\"\nversion = \"{version}\"\nentry = \"src/lib.gu\"\nexports = [\"src/lib.gu\"]\n{capabilities}"),
    )
    .unwrap();
    std::fs::write(root.join("src/lib.gu"), body).unwrap();
}

fn new_project(root: &Path, import: &str, call: &str) {
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("guion.toml"),
        "[project]\nname = \"app\"\nversion = \"0.1.0\"\nentry = \"src/main.gu\"\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/main.gu"),
        format!("import \"{import}\"\n\nfn main() -> Int:\n    print({call})\n    return 0\n"),
    )
    .unwrap();
}

#[test]
fn publish_add_yank_and_verify_against_the_local_registry() {
    if !wrangler_available() {
        eprintln!(
            "SKIPPED: wrangler is not installed; scripts/verify-package-registry.sh requires it"
        );
        return;
    }
    let registry = start_registry("registry-e2e");
    let alice = new_publisher(&registry, "alice");
    let bob = new_publisher(&registry, "bob");
    let work = std::env::temp_dir().join(format!("guion-registry-work-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).unwrap();
    let mut transcript = String::new();

    // Publish two versions of a library.
    let library = work.join("number-tools");
    write_library(
        &library,
        "number-tools",
        "1.0.0",
        "",
        "pub fn double(value: Int) -> Int:\n    return value * 2\n",
    );
    let first = guion(&library, &registry, Some(&alice), &["publish"]);
    transcript.push_str(&text(&first));
    assert!(first.status.success(), "{}", text(&first));
    write_library(&library, "number-tools", "1.1.0", "", "pub fn double(value: Int) -> Int:\n    return value * 2\n\npub fn triple(value: Int) -> Int:\n    return value * 3\n");
    let second = guion(&library, &registry, Some(&alice), &["publish"]);
    transcript.push_str(&text(&second));
    assert!(second.status.success(), "{}", text(&second));

    // Versions are immutable: the same version with different content is refused.
    write_library(
        &library,
        "number-tools",
        "1.1.0",
        "",
        "pub fn double(value: Int) -> Int:\n    return value * 5\n",
    );
    let changed = guion(&library, &registry, Some(&alice), &["publish"]);
    transcript.push_str(&text(&changed));
    assert!(!changed.status.success());
    assert!(
        text(&changed).contains("E_VERSION_EXISTS"),
        "{}",
        text(&changed)
    );

    // Another publisher cannot publish under, or spell their way around, the owned name.
    write_library(
        &library,
        "number_tools",
        "2.0.0",
        "",
        "pub fn double(value: Int) -> Int:\n    return value\n",
    );
    let stolen = guion(&library, &registry, Some(&bob), &["publish"]);
    transcript.push_str(&text(&stolen));
    assert!(!stolen.status.success());
    assert!(text(&stolen).contains("E_NOT_OWNER"), "{}", text(&stolen));

    // Publishing needs a token.
    let anonymous = guion(&library, &registry, None, &["publish"]);
    assert!(!anonymous.status.success());
    assert!(
        text(&anonymous).contains("GUION_REGISTRY_TOKEN"),
        "{}",
        text(&anonymous)
    );

    // A library that uses more than it declares is refused before upload.
    let greedy = work.join("greedy-lib");
    write_library(
        &greedy,
        "greedy-lib",
        "1.0.0",
        "",
        "pub fn load(path: Text) -> Bool:\n    return is_ok(read_text(path))\n",
    );
    let refused = guion(&greedy, &registry, Some(&alice), &["publish"]);
    transcript.push_str(&text(&refused));
    assert!(!refused.status.success());
    assert!(
        text(&refused).contains("does not declare"),
        "{}",
        text(&refused)
    );
    assert_eq!(
        curl(&[
            "-s",
            "-o",
            "/dev/null",
            "-w",
            "%{http_code}",
            &format!("{}/index/greedy-lib.json", registry.url)
        ]),
        "404"
    );

    // A reserved name is refused by the client.
    let reserved = work.join("regex");
    write_library(
        &reserved,
        "regex",
        "1.0.0",
        "",
        "pub fn x() -> Int:\n    return 1\n",
    );
    let reserved_attempt = guion(&reserved, &registry, Some(&alice), &["publish"]);
    assert!(!reserved_attempt.status.success());
    assert!(
        text(&reserved_attempt).contains("reserved"),
        "{}",
        text(&reserved_attempt)
    );

    // A consumer resolves a range to the highest version and the hash agrees at every hop.
    let app = work.join("app");
    new_project(&app, "pkg:number-tools/src/lib.gu", "triple(14)");
    let added = guion(
        &app,
        &registry,
        None,
        &["add", "number-tools", "--version", "^1.0"],
    );
    transcript.push_str(&text(&added));
    assert!(added.status.success(), "{}", text(&added));
    let lock = std::fs::read_to_string(app.join("guion.lock")).unwrap();
    assert!(lock.contains("version = \"1.1.0\""), "{lock}");
    let index: serde_json::Value = serde_json::from_str(&curl(&[
        "-s",
        &format!("{}/index/number-tools.json", registry.url),
    ]))
    .unwrap();
    let entry = index["versions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["version"] == "1.1.0")
        .unwrap();
    let index_hash = entry["sha256"].as_str().unwrap();
    assert!(
        text(&added).contains(index_hash) || lock.contains(index_hash) || {
            // The lock records the content hash of the unpacked tree; the archive hash is in the project manifest.
            std::fs::read_to_string(app.join("guion.toml"))
                .unwrap()
                .contains(index_hash)
        }
    );
    let log_entry: serde_json::Value = serde_json::from_str(&curl(&[
        "-s",
        &format!(
            "{}/log/{:010}.json",
            registry.url,
            entry["log_seq"].as_u64().unwrap()
        ),
    ]))
    .unwrap();
    assert_eq!(log_entry["sha256"], index_hash);
    let run = guion(&app, &registry, None, &["run", "src/main.gu"]);
    assert!(run.status.success(), "{}", text(&run));
    assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), "42");

    // An exact older version can be requested.
    let pinned = work.join("pinned");
    new_project(&pinned, "pkg:number-tools/src/lib.gu", "double(21)");
    let older = guion(
        &pinned,
        &registry,
        None,
        &["add", "number-tools", "--version", "=1.0.0"],
    );
    assert!(older.status.success(), "{}", text(&older));
    assert!(std::fs::read_to_string(pinned.join("guion.lock"))
        .unwrap()
        .contains("version = \"1.0.0\""));

    // Yank hides 1.1.0 from new resolution; a project that already locked it still installs it.
    let yanked = guion(
        &library,
        &registry,
        Some(&alice),
        &["yank", "number-tools", "1.1.0"],
    );
    transcript.push_str(&text(&yanked));
    assert!(yanked.status.success(), "{}", text(&yanked));
    let fresh = work.join("fresh");
    new_project(&fresh, "pkg:number-tools/src/lib.gu", "double(21)");
    let after_yank = guion(
        &fresh,
        &registry,
        None,
        &["add", "number-tools", "--version", "^1.0"],
    );
    assert!(after_yank.status.success(), "{}", text(&after_yank));
    assert!(
        std::fs::read_to_string(fresh.join("guion.lock"))
            .unwrap()
            .contains("version = \"1.0.0\""),
        "a yanked version must not be chosen for a new resolution"
    );
    let again = guion(
        &app,
        &registry,
        None,
        &["add", "number-tools", "--version", "^1.0"],
    );
    assert!(
        again.status.success(),
        "a locked yanked version stays installable: {}",
        text(&again)
    );
    let still_there = curl(&[
        "-s",
        "-o",
        "/dev/null",
        "-w",
        "%{http_code}",
        &format!("{}/objects/{index_hash}.tar", registry.url),
    ]);
    assert_eq!(still_there, "200");

    // A missing package says so.
    let missing = guion(&fresh, &registry, None, &["add", "does-not-exist"]);
    assert!(!missing.status.success());
    assert!(
        text(&missing).contains("no versions are available"),
        "{}",
        text(&missing)
    );

    // No output anywhere contains a token.
    for secret in [&alice, &bob] {
        assert!(!transcript.contains(secret.as_str()));
        assert!(!lock.contains(secret.as_str()));
    }
    let _ = std::fs::remove_dir_all(&work);
}

#[test]
fn a_published_library_can_depend_on_another_registry_library() {
    if !wrangler_available() {
        eprintln!(
            "SKIPPED: wrangler is not installed; scripts/verify-package-registry.sh requires it"
        );
        return;
    }
    let registry = start_registry("registry-deps");
    let alice = new_publisher(&registry, "alice");
    let work = std::env::temp_dir().join(format!("guion-dependency-work-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).unwrap();

    let base = work.join("base-math");
    write_library(
        &base,
        "base-math",
        "1.2.0",
        "",
        "pub fn double(value: Int) -> Int:\n    return value * 2\n",
    );
    let published = guion(&base, &registry, Some(&alice), &["publish"]);
    assert!(published.status.success(), "{}", text(&published));

    // `top-math` asks for a range; publishing pins it to what the registry holds now.
    let top = work.join("top-math");
    write_library(
        &top,
        "top-math",
        "0.1.0",
        "",
        "import \"pkg:base-math/src/lib.gu\"\n\npub fn quadruple(value: Int) -> Int:\n    return double(double(value))\n",
    );
    let mut manifest = std::fs::read_to_string(top.join("guion.toml")).unwrap();
    manifest.push_str("\n[dependencies]\nbase-math = \"^1.0\"\n");
    std::fs::write(top.join("guion.toml"), manifest).unwrap();
    let published = guion(&top, &registry, Some(&alice), &["publish"]);
    assert!(published.status.success(), "{}", text(&published));

    // A consumer adds only `top-math`; `base-math` arrives with it, pinned and hash-checked.
    let app = work.join("app");
    new_project(&app, "pkg:top-math/src/lib.gu", "quadruple(5)");
    let added = guion(&app, &registry, None, &["add", "top-math"]);
    assert!(added.status.success(), "{}", text(&added));
    let lock = std::fs::read_to_string(app.join("guion.lock")).unwrap();
    assert!(lock.contains("base-math"), "{lock}");
    assert!(lock.contains("1.2.0"), "{lock}");
    let run = guion(&app, &registry, None, &["run", "src/main.gu"]);
    assert!(run.status.success(), "{}", text(&run));
    assert!(text(&run).contains("20"), "{}", text(&run));
    let _ = std::fs::remove_dir_all(&work);
}
