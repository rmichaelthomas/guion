use std::path::Path;
use std::process::{Command, Output};

fn guion(directory: &Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(arguments)
        .current_dir(directory)
        .output()
        .expect("guion starts")
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("guion-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn new_lib_creates_a_library_that_checks_tests_and_documents() {
    let dir = scratch("lib-new");
    let created = guion(&dir, &["new", "--lib", "my-lib"]);
    assert!(created.status.success(), "{}", text(&created));
    let library = dir.join("my-lib");
    let manifest = std::fs::read_to_string(library.join("guion.toml")).unwrap();
    assert!(manifest.contains("[package]") && manifest.contains("name = \"my-lib\""));

    let checked = guion(&library, &["check", "tests.gu"]);
    assert!(checked.status.success(), "{}", text(&checked));
    assert!(
        text(&checked).contains("library my-lib 0.1.0"),
        "{}",
        text(&checked)
    );

    let tested = guion(&library, &["test", "tests.gu"]);
    assert!(tested.status.success(), "{}", text(&tested));
    let documented = guion(&library, &["doc", "tests.gu"]);
    assert!(text(&documented).contains("greet"), "{}", text(&documented));

    // A library whose only module exposes nothing public fails the surface check.
    std::fs::write(
        library.join("src/lib.gu"),
        "fn hidden() -> Int:\n    return 1\n",
    )
    .unwrap();
    std::fs::write(
        library.join("tests.gu"),
        "import \"src/lib.gu\"\n\nfn main() -> Int:\n    return 0\n",
    )
    .unwrap();
    let hidden = guion(&library, &["check", "tests.gu"]);
    assert!(!hidden.status.success());
    assert!(
        text(&hidden).contains("no `pub` items"),
        "{}",
        text(&hidden)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn new_lib_enforces_the_name_rules() {
    let dir = scratch("lib-names");
    for (name, expected) in [
        ("Upper", "must be lowercase"),
        ("regex", "reserved"),
        ("guion-tools", "reserved"),
        ("9lives", "start with a letter"),
    ] {
        let output = guion(&dir, &["new", "--lib", name]);
        assert!(!output.status.success(), "{name}");
        assert!(
            text(&output).contains(expected),
            "{name}: {}",
            text(&output)
        );
        assert!(!dir.join(name).exists(), "{name} must not be created");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

fn write_library(root: &Path, capabilities: &str, body: &str) {
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("guion.toml"),
        format!("[package]\nname = \"fetcher\"\nversion = \"1.0.0\"\nentry = \"src/lib.gu\"\nexports = [\"src/lib.gu\"]\n{capabilities}"),
    )
    .unwrap();
    std::fs::write(root.join("src/lib.gu"), body).unwrap();
    std::fs::write(
        root.join("tests.gu"),
        "import \"src/lib.gu\"\n\nfn main() -> Int:\n    return 0\n",
    )
    .unwrap();
}

const READS_A_FILE: &str = "pub fn load(path: Text) -> Bool:\n    return is_ok(read_text(path))\n";

#[test]
fn undeclared_capability_use_fails_check_and_declaring_it_passes() {
    let dir = scratch("lib-caps");
    let library = dir.join("fetcher");
    write_library(&library, "", READS_A_FILE);
    let undeclared = guion(&library, &["check", "tests.gu"]);
    assert!(!undeclared.status.success());
    assert!(
        text(&undeclared).contains("filesystem.read")
            && text(&undeclared).contains("files = [\"read\"]"),
        "{}",
        text(&undeclared)
    );

    write_library(
        &library,
        "\n[capabilities]\nfiles = [\"read\"]\n",
        READS_A_FILE,
    );
    let declared = guion(&library, &["check", "tests.gu"]);
    assert!(declared.status.success(), "{}", text(&declared));

    write_library(
        &library,
        "\n[capabilities]\nfiles = [\"read\"]\nclock = true\n",
        READS_A_FILE,
    );
    let extra = guion(&library, &["check", "tests.gu"]);
    assert!(extra.status.success(), "{}", text(&extra));
    assert!(
        text(&extra).contains("never uses `clock.read`"),
        "{}",
        text(&extra)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn hosts_named_in_the_code_must_be_declared() {
    let dir = scratch("lib-hosts");
    let library = dir.join("fetcher");
    let two_hosts = "pub fn today() -> Bool:\n    return is_ok(http_get(\"https://api.weather.example/today\"))\n\npub fn backup() -> Bool:\n    return is_ok(http_get(\"https://Mirror.Example.org/x\"))\n";
    write_library(
        &library,
        "\n[capabilities]\nnetwork = [\"api.weather.example\"]\n",
        two_hosts,
    );
    let missing = guion(&library, &["check", "tests.gu"]);
    assert!(!missing.status.success());
    assert!(
        text(&missing).contains("`backup` contacts `mirror.example.org`"),
        "{}",
        text(&missing)
    );

    write_library(
        &library,
        "\n[capabilities]\nnetwork = [\"api.weather.example\", \"mirror.example.org\"]\n",
        two_hosts,
    );
    let declared = guion(&library, &["check", "tests.gu"]);
    assert!(declared.status.success(), "{}", text(&declared));
    assert!(!text(&declared).contains("cannot be confirmed"));

    // A host built at run time cannot be confirmed, which check reports without failing.
    write_library(
        &library,
        "\n[capabilities]\nnetwork = [\"api.weather.example\"]\n",
        "pub fn fetch(url: Text) -> Bool:\n    return is_ok(http_get(url))\n",
    );
    let dynamic = guion(&library, &["check", "tests.gu"]);
    assert!(dynamic.status.success(), "{}", text(&dynamic));
    assert!(
        text(&dynamic).contains("cannot be confirmed"),
        "{}",
        text(&dynamic)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_update_that_widens_capabilities_needs_explicit_approval() {
    let dir = scratch("lib-widen");
    let library = dir.join("fetcher");
    let project = dir.join("app");
    write_library(
        &library,
        "\n[capabilities]\nfiles = [\"read\"]\n",
        READS_A_FILE,
    );
    std::fs::create_dir_all(project.join("src")).unwrap();
    std::fs::write(
        project.join("guion.toml"),
        "[project]\nname = \"app\"\nversion = \"0.1.0\"\nentry = \"src/main.gu\"\n",
    )
    .unwrap();
    std::fs::write(
        project.join("src/main.gu"),
        "fn main() -> Int:\n    return 0\n",
    )
    .unwrap();
    let added = guion(
        &project,
        &["add", "fetcher", "--path", library.to_str().unwrap()],
    );
    assert!(added.status.success(), "{}", text(&added));
    let lock = std::fs::read_to_string(project.join("guion.lock")).unwrap();
    assert!(lock.contains("capabilities"), "{lock}");

    // A new library version asks for more.
    write_library(
        &library,
        "\n[capabilities]\nfiles = [\"read\", \"write\"]\nprocesses = [\"git\"]\n",
        READS_A_FILE,
    );
    let refused = guion(&project, &["update"]);
    assert!(!refused.status.success());
    let message = text(&refused);
    assert!(
        message.contains("widens package capabilities")
            && message.contains("file access `write`")
            && message.contains("command `git`")
            && message.contains("--approve-capabilities"),
        "{message}"
    );
    let still_locked = std::fs::read_to_string(project.join("guion.lock")).unwrap();
    assert_eq!(
        lock, still_locked,
        "a refused update must leave the lock unchanged"
    );

    let approved = guion(&project, &["update", "--approve-capabilities"]);
    assert!(approved.status.success(), "{}", text(&approved));
    let widened = std::fs::read_to_string(project.join("guion.lock")).unwrap();
    assert!(widened.contains("git"), "{widened}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn built_artifacts_record_the_packages_they_link() {
    let dir = scratch("lib-identity");
    let library = dir.join("fetcher");
    let project = dir.join("app");
    write_library(
        &library,
        "",
        "pub fn double(value: Int) -> Int:\n    return value * 2\n",
    );
    std::fs::create_dir_all(project.join("src")).unwrap();
    std::fs::write(
        project.join("guion.toml"),
        "[project]\nname = \"app\"\nversion = \"0.1.0\"\nentry = \"src/main.gu\"\n",
    )
    .unwrap();
    std::fs::write(
        project.join("src/main.gu"),
        "import \"pkg:fetcher/src/lib.gu\"\n\nfn main() -> Int:\n    return double(21)\n",
    )
    .unwrap();
    let added = guion(
        &project,
        &["add", "fetcher", "--path", library.to_str().unwrap()],
    );
    assert!(added.status.success(), "{}", text(&added));
    let built = guion(&project, &["build", "src/main.gu", "-o", "app.gub"]);
    assert!(built.status.success(), "{}", text(&built));

    let artifact =
        guion_model::Artifact::from_bytes(&std::fs::read(project.join("app.gub")).unwrap())
            .unwrap();
    assert_eq!(
        artifact.format_version,
        guion_model::Artifact::CURRENT_VERSION
    );
    let packages = &artifact.build_metadata.expect("metadata").packages;
    assert_eq!(packages.len(), 1, "{packages:?}");
    assert_eq!(packages[0].name, "fetcher");
    assert_eq!(packages[0].version, "1.0.0");
    let lock = std::fs::read_to_string(project.join("guion.lock")).unwrap();
    assert!(
        lock.contains(&packages[0].sha256),
        "the artifact hash must be the locked hash"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
