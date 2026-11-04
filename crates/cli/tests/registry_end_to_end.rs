//! Exercises `guion publish`, `guion add NAME`, `guion yank` and the verification chain against
//! the reference registry (registry/) running locally under `wrangler dev --local` on loopback.
//! Nothing here touches the network beyond the local machine or any Cloudflare account.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};

const ADMIN_TOKEN: &str = "local-admin-token-for-tests";
// A throwaway ed25519 seed for the local registry's log signatures, and its public key.
const SIGNING_KEY: &str = "2222222222222222222222222222222222222222222222222222222222222222";
const PUBLIC_KEY: &str = "a09aa5f47a6759802ff955f8dc2d2a14a5c99d23be97f864127ff9383455a4f0";

struct LocalRegistry {
    child: Child,
    url: String,
    state: PathBuf,
    _turn: std::sync::MutexGuard<'static, ()>,
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

/// Each test starts its own `wrangler dev`; several starting at once on a busy machine can
/// exceed the start-up wait, so the tests take turns.
static ONE_REGISTRY_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn start_registry(name: &str) -> LocalRegistry {
    // A test that panicked while holding the turn has already reported its failure.
    let turn = ONE_REGISTRY_AT_A_TIME
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
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
        .args(["--var", &format!("SIGNING_KEY:{SIGNING_KEY}")])
        .current_dir(registry_dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("wrangler starts");
    let url = format!("http://127.0.0.1:{port}");
    let registry = LocalRegistry {
        child,
        url,
        state,
        _turn: turn,
    };
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
    guion_trusting(directory, registry, token, arguments, PUBLIC_KEY)
}

fn guion_trusting(
    directory: &Path,
    registry: &LocalRegistry,
    token: Option<&str>,
    arguments: &[&str],
    public_key: &str,
) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_guion"));
    command
        .args(arguments)
        .current_dir(directory)
        .env("GUION_REGISTRY_URL", &registry.url)
        .env("GUION_REGISTRY_PUBLIC_KEY", public_key)
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

#[test]
fn update_moves_a_registry_package_to_the_newest_release_its_range_allows() {
    if !wrangler_available() {
        eprintln!(
            "SKIPPED: wrangler is not installed; scripts/verify-package-registry.sh requires it"
        );
        return;
    }
    let registry = start_registry("registry-update");
    let alice = new_publisher(&registry, "alice");
    let work = std::env::temp_dir().join(format!("guion-update-work-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).unwrap();
    let library = work.join("step-tools");
    let publish = |version: &str, factor: u32| {
        write_library(
            &library,
            "step-tools",
            version,
            "",
            &format!("pub fn step(value: Int) -> Int:\n    return value * {factor}\n"),
        );
        let published = guion(&library, &registry, Some(&alice), &["publish"]);
        assert!(published.status.success(), "{}", text(&published));
    };
    publish("1.0.0", 2);

    // With no range given, `add` records a caret on the chosen version.
    let app = work.join("app");
    new_project(&app, "pkg:step-tools/src/lib.gu", "step(10)");
    let added = guion(&app, &registry, None, &["add", "step-tools"]);
    assert!(added.status.success(), "{}", text(&added));
    let manifest = std::fs::read_to_string(app.join("guion.toml")).unwrap();
    assert!(manifest.contains("registry = \"^1.0.0\""), "{manifest}");

    // Nothing newer yet: update keeps 1.0.0.
    let unchanged = guion(&app, &registry, None, &["update"]);
    assert!(unchanged.status.success(), "{}", text(&unchanged));
    assert!(std::fs::read_to_string(app.join("guion.lock"))
        .unwrap()
        .contains("version = \"1.0.0\""));

    // A compatible release is picked up; a major release is not.
    publish("1.3.0", 3);
    publish("2.0.0", 4);
    let updated = guion(&app, &registry, None, &["update"]);
    assert!(updated.status.success(), "{}", text(&updated));
    assert!(text(&updated).contains("was 1.0.0"), "{}", text(&updated));
    let lock = std::fs::read_to_string(app.join("guion.lock")).unwrap();
    assert!(lock.contains("version = \"1.3.0\""), "{lock}");
    let manifest = std::fs::read_to_string(app.join("guion.toml")).unwrap();
    assert!(manifest.contains("registry = \"^1.0.0\""), "{manifest}");
    let run = guion(&app, &registry, None, &["run", "src/main.gu"]);
    assert!(run.status.success(), "{}", text(&run));
    assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), "30");

    // Yanking the installed release does not force a downgrade: the locked 1.3.0 stays eligible.
    let yanked = guion(
        &library,
        &registry,
        Some(&alice),
        &["yank", "step-tools", "1.3.0"],
    );
    assert!(yanked.status.success(), "{}", text(&yanked));
    let after_yank = guion(&app, &registry, None, &["update"]);
    assert!(after_yank.status.success(), "{}", text(&after_yank));
    assert!(std::fs::read_to_string(app.join("guion.lock"))
        .unwrap()
        .contains("version = \"1.3.0\""));

    // An explicit range is kept as written and allows the major release.
    let wide = work.join("wide");
    new_project(&wide, "pkg:step-tools/src/lib.gu", "step(10)");
    let added = guion(
        &wide,
        &registry,
        None,
        &["add", "step-tools", "--version", "=1.0.0"],
    );
    assert!(added.status.success(), "{}", text(&added));
    let manifest = std::fs::read_to_string(wide.join("guion.toml")).unwrap();
    std::fs::write(
        wide.join("guion.toml"),
        manifest.replace("registry = \"=1.0.0\"", "registry = \">=1.0, <3.0\""),
    )
    .unwrap();
    let updated = guion(&wide, &registry, None, &["update"]);
    assert!(updated.status.success(), "{}", text(&updated));
    assert!(std::fs::read_to_string(wide.join("guion.lock"))
        .unwrap()
        .contains("version = \"2.0.0\""));
    let _ = std::fs::remove_dir_all(&work);
}

#[test]
fn token_rotate_and_revoke_retire_the_old_token() {
    if !wrangler_available() {
        eprintln!(
            "SKIPPED: wrangler is not installed; scripts/verify-package-registry.sh requires it"
        );
        return;
    }
    let registry = start_registry("registry-tokens");
    let first = new_publisher(&registry, "alice");
    let work = std::env::temp_dir().join(format!("guion-token-work-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).unwrap();
    let library = work.join("token-lib");
    write_library(
        &library,
        "token-lib",
        "1.0.0",
        "",
        "pub fn one() -> Int:\n    return 1\n",
    );

    let rotated = guion(&work, &registry, Some(&first), &["token", "rotate"]);
    assert!(rotated.status.success(), "{}", text(&rotated));
    let second = String::from_utf8_lossy(&rotated.stdout)
        .lines()
        .last()
        .unwrap()
        .trim()
        .to_owned();
    assert!(second.starts_with("gpt_") && second != first, "{second}");

    let refused = guion(&library, &registry, Some(&first), &["publish"]);
    assert!(!refused.status.success());
    assert!(
        text(&refused).contains("E_AUTH_INVALID"),
        "{}",
        text(&refused)
    );
    assert!(!text(&refused).contains(&first));
    let published = guion(&library, &registry, Some(&second), &["publish"]);
    assert!(published.status.success(), "{}", text(&published));

    // The log is signed: a client that trusts a different key refuses to install from it.
    let app = work.join("app");
    new_project(&app, "pkg:token-lib/src/lib.gu", "one()");
    let wrong_key = "3".repeat(64);
    let untrusted = guion_trusting(&app, &registry, None, &["add", "token-lib"], &wrong_key);
    assert!(!untrusted.status.success());
    assert!(
        text(&untrusted).contains("not signed by the registry's key"),
        "{}",
        text(&untrusted)
    );
    // Trusting several keys (as during a key rotation) accepts an entry signed by any of them.
    let trusted = guion_trusting(
        &app,
        &registry,
        None,
        &["add", "token-lib"],
        &format!("{wrong_key}, {PUBLIC_KEY}"),
    );
    assert!(trusted.status.success(), "{}", text(&trusted));

    let revoked = guion(&work, &registry, Some(&second), &["token", "revoke"]);
    assert!(revoked.status.success(), "{}", text(&revoked));
    assert!(!text(&revoked).contains(&second));
    write_library(
        &library,
        "token-lib",
        "1.1.0",
        "",
        "pub fn one() -> Int:\n    return 1\n",
    );
    let after = guion(&library, &registry, Some(&second), &["publish"]);
    assert!(!after.status.success());

    let usage = guion(&work, &registry, None, &["token"]);
    assert!(!usage.status.success());
    assert!(text(&usage).contains("guion token <rotate|revoke>"));
    let _ = std::fs::remove_dir_all(&work);
}
