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
        &[
            "add",
            "fetcher",
            "--path",
            library.to_str().unwrap(),
            "--approve-capabilities",
        ],
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
        message.contains("files: +modes [\"write\"]")
            && message.contains("processes: +commands [\"git\"]")
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

#[test]
fn a_project_with_approved_capabilities_refuses_a_path_package_that_asks_for_more() {
    let dir = scratch("lib-approved");
    let library = dir.join("fetcher");
    let project = dir.join("app");
    write_library(
        &library,
        "\n[capabilities]\nfiles = [\"read\"]\nprocesses = [\"git\"]\n",
        READS_A_FILE,
    );
    std::fs::create_dir_all(project.join("src")).unwrap();
    std::fs::write(
        project.join("guion.toml"),
        "[project]\nname = \"app\"\nversion = \"0.1.0\"\nentry = \"src/main.gu\"\n\n[approved-capabilities.fetcher]\nfiles = [\"read\"]\n",
    )
    .unwrap();
    std::fs::write(
        project.join("src/main.gu"),
        "fn main() -> Int:\n    return 0\n",
    )
    .unwrap();
    let refused = guion(
        &project,
        &["add", "fetcher", "--path", library.to_str().unwrap()],
    );
    assert!(!refused.status.success());
    assert!(
        text(&refused).contains("processes: +commands [\"git\"]"),
        "{}",
        text(&refused)
    );
    assert!(!project.join("guion.lock").exists());
    let approved = guion(
        &project,
        &[
            "add",
            "fetcher",
            "--path",
            library.to_str().unwrap(),
            "--approve-capabilities",
        ],
    );
    assert!(approved.status.success(), "{}", text(&approved));
    let manifest = std::fs::read_to_string(project.join("guion.toml")).unwrap();
    assert!(manifest.contains("git"), "{manifest}");
    let _ = std::fs::remove_dir_all(&dir);
}

const UPLOADS_LIB: &str = "import \"std:http_body\"\nimport \"std:http_response\"\nimport \"std:time\"\n\npub fn handle_upload(request: Request) -> Result[Response, HttpFailure]:\n    let limits = BodyLimits(1048576, 1048576, 16, 16, 1048576)\n    let parsed = multipart_parse(request, \"uploads\", limits)?\n    return Ok(response_text(200, \"stored\"))\n\npub fn stamp() -> Int:\n    return unix_time_ms()\n";

fn write_uploads(root: &Path) {
    write_library(
        root,
        "\n[capabilities]\nfiles = [\"read\", \"write\"]\nclock = true\n",
        UPLOADS_LIB,
    );
    std::fs::write(
        root.join("guion.toml"),
        std::fs::read_to_string(root.join("guion.toml"))
            .unwrap()
            .replace("fetcher", "uploads"),
    )
    .unwrap();
}

fn json_of(output: &Output) -> serde_json::Value {
    serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("not JSON ({error}): {}", text(output)))
}

fn function_effects(report: &serde_json::Value, key_in_functions: &str) -> Vec<String> {
    let entry = match &report["functions"] {
        serde_json::Value::Array(items) => items
            .iter()
            .find(|f| f["name"] == key_in_functions)
            .unwrap_or_else(|| panic!("no function {key_in_functions}"))
            .clone(),
        other => other[key_in_functions].clone(),
    };
    entry["effects"]
        .as_array()
        .unwrap_or_else(|| panic!("no effects for {key_in_functions}"))
        .iter()
        .map(|e| e.as_str().unwrap().to_owned())
        .collect()
}

fn package_set(report: &serde_json::Value, package: &str, key: &str) -> Vec<String> {
    report["package_effects"][package][key]
        .as_array()
        .unwrap_or_else(|| panic!("no package_effects.{package}.{key}: {report}"))
        .iter()
        .map(|e| e.as_str().unwrap().to_owned())
        .collect()
}

fn upload_app(dir: &Path, manifest_tail: &str, source: &str) -> std::path::PathBuf {
    let project = dir.join("app");
    std::fs::create_dir_all(project.join("src")).unwrap();
    std::fs::write(
        project.join("guion.toml"),
        format!("[project]\nname = \"app\"\nversion = \"0.1.0\"\nentry = \"src/main.gu\"\n{manifest_tail}"),
    )
    .unwrap();
    std::fs::write(project.join("src/main.gu"), source).unwrap();
    project
}

const UPLOAD_APP: &str = "import \"pkg:uploads/src/lib.gu\"\nimport \"std:http_response\"\n\nfn route(request: Request) -> Result[Response, HttpFailure]:\n    return handle_upload(request)\n\nfn main() -> Int:\n    return stamp()\n";

#[test]
fn effects_cross_package_boundaries() {
    let dir = scratch("f2-effects");
    let library = dir.join("uploads");
    write_uploads(&library);
    let project = upload_app(
        &dir,
        "\n[clock]\nenabled = true\n\n[approved-capabilities.uploads]\nfiles = [\"read\", \"write\"]\nclock = true\n",
        UPLOAD_APP,
    );
    let added = guion(
        &project,
        &["add", "uploads", "--path", library.to_str().unwrap()],
    );
    assert!(added.status.success(), "{}", text(&added));

    // The upload handler's file write is only visible through the package, and it is attributed to
    // the package function and to the application function that calls it.
    let doc = json_of(&guion(&project, &["doc", "--json", "src/main.gu"]));
    assert_eq!(
        function_effects(&doc, "handle_upload"),
        ["filesystem.write"]
    );
    assert_eq!(function_effects(&doc, "route"), ["filesystem.write"]);
    assert_eq!(function_effects(&doc, "stamp"), ["clock.read"]);
    assert_eq!(function_effects(&doc, "main"), ["clock.read"]);
    assert_eq!(
        package_set(&doc, "uploads", "effects"),
        ["clock.read", "filesystem.write"]
    );
    assert!(package_set(&doc, "uploads", "beyond_approved").is_empty());

    // `guion inspect` of the built artifact agrees with `guion doc`.
    let built = guion(&project, &["build", "src/main.gu", "-o", "app.gub"]);
    assert!(built.status.success(), "{}", text(&built));
    let inspected = json_of(&guion(&project, &["inspect", "--json", "app.gub"]));
    for name in ["handle_upload", "route", "stamp", "main"] {
        assert_eq!(
            function_effects(&inspected, name),
            function_effects(&doc, name),
            "{name}"
        );
    }
    assert_eq!(
        package_set(&inspected, "uploads", "effects"),
        package_set(&doc, "uploads", "effects")
    );
    assert_eq!(inspected["grant_assessment"]["clock_read"], "present");
    let human = text(&guion(&project, &["inspect", "app.gub"]));
    assert!(
        human.contains("package uploads: clock.read, filesystem.write"),
        "{human}"
    );

    // Removing the file grant from the app's manifest changes the reported set, and the capability
    // check at check, build and run refuses the program.
    std::fs::write(
        project.join("guion.toml"),
        "[project]\nname = \"app\"\nversion = \"0.1.0\"\nentry = \"src/main.gu\"\n\n[clock]\nenabled = true\n\n[approved-capabilities.uploads]\nfiles = [\"read\"]\nclock = true\n",
    )
    .unwrap();
    let doc = json_of(&guion(&project, &["doc", "--json", "src/main.gu"]));
    assert_eq!(
        package_set(&doc, "uploads", "beyond_approved"),
        ["filesystem.write (files = [\"write\"])"]
    );
    for command in [
        vec!["check", "src/main.gu"],
        vec!["build", "src/main.gu", "-o", "again.gub"],
        vec!["run", "src/main.gu"],
    ] {
        let refused = guion(&project, &command);
        assert!(!refused.status.success(), "{command:?}");
        assert!(
            text(&refused).contains("uploads: filesystem.write (files = [\"write\"])"),
            "{command:?}: {}",
            text(&refused)
        );
    }
    assert!(!project.join("again.gub").exists());

    // With no approval table at all nothing is approved: the program does not build, and the
    // report lists everything the package reaches as beyond approval.
    std::fs::write(
        project.join("guion.toml"),
        "[project]\nname = \"app\"\nversion = \"0.1.0\"\nentry = \"src/main.gu\"\n",
    )
    .unwrap();
    let refused = guion(&project, &["build", "src/main.gu", "-o", "app.gub"]);
    assert!(!refused.status.success(), "{}", text(&refused));
    assert!(
        text(&refused).contains("uploads: filesystem.write (files = [\"write\"])")
            && text(&refused).contains("uploads: clock.read (clock = true)"),
        "{}",
        text(&refused)
    );
    let doc = json_of(&guion(&project, &["doc", "--json", "src/main.gu"]));
    assert_eq!(
        package_set(&doc, "uploads", "beyond_approved"),
        [
            "filesystem.write (files = [\"write\"])",
            "clock.read (clock = true)"
        ]
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn effects_reached_through_another_package_are_attributed_and_checked() {
    let dir = scratch("f2-nested");
    let uploads = dir.join("uploads");
    let gateway = dir.join("gateway");
    write_uploads(&uploads);
    std::fs::create_dir_all(gateway.join("src")).unwrap();
    std::fs::write(
        gateway.join("guion.toml"),
        format!(
            "[package]\nname = \"gateway\"\nversion = \"1.0.0\"\nentry = \"src/lib.gu\"\nexports = [\"src/lib.gu\"]\n\n[dependencies.uploads]\nversion = \"1.0.0\"\npath = {:?}\n",
            "../uploads"
        ),
    )
    .unwrap();
    std::fs::write(
        gateway.join("src/lib.gu"),
        "import \"pkg:uploads/src/lib.gu\"\nimport \"std:http_response\"\n\npub fn front_door(request: Request) -> Result[Response, HttpFailure]:\n    return handle_upload(request)\n",
    )
    .unwrap();
    let project = upload_app(
        &dir,
        "\n[approved-capabilities.uploads]\nfiles = [\"read\", \"write\"]\nclock = true\n\n[approved-capabilities.gateway]\nfiles = [\"write\"]\n",
        "import \"pkg:gateway/src/lib.gu\"\nimport \"std:http_response\"\n\nfn route(request: Request) -> Result[Response, HttpFailure]:\n    return front_door(request)\n\nfn main() -> Int:\n    return 0\n",
    );
    let added = guion(
        &project,
        &["add", "gateway", "--path", gateway.to_str().unwrap()],
    );
    assert!(added.status.success(), "{}", text(&added));
    let doc = json_of(&guion(&project, &["doc", "--json", "src/main.gu"]));
    assert_eq!(function_effects(&doc, "front_door"), ["filesystem.write"]);
    assert_eq!(function_effects(&doc, "route"), ["filesystem.write"]);
    assert_eq!(
        package_set(&doc, "gateway", "effects"),
        ["filesystem.write"]
    );
    assert!(package_set(&doc, "gateway", "beyond_approved").is_empty());
    assert!(guion(&project, &["check", "src/main.gu"]).status.success());

    // Approving gateway for nothing is not enough: its effect comes from the package it imports.
    std::fs::write(
        project.join("guion.toml"),
        "[project]\nname = \"app\"\nversion = \"0.1.0\"\nentry = \"src/main.gu\"\n\n[approved-capabilities.uploads]\nfiles = [\"read\", \"write\"]\nclock = true\n\n[approved-capabilities.gateway]\nclock = true\n",
    )
    .unwrap();
    let refused = guion(&project, &["check", "src/main.gu"]);
    assert!(!refused.status.success());
    assert!(
        text(&refused).contains("gateway: filesystem.write (files = [\"write\"])"),
        "{}",
        text(&refused)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn add_path_writes_a_relative_path_so_the_manifest_is_portable() {
    let dir = scratch("lib-portable");
    let library = dir.join("fetcher");
    let project = dir.join("app");
    write_library(&library, "", READS_A_FILE);
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
    let manifest = std::fs::read_to_string(project.join("guion.toml")).unwrap();
    assert!(manifest.contains("path = \"../fetcher\""), "{manifest}");
    let lock = std::fs::read_to_string(project.join("guion.lock")).unwrap();
    assert!(lock.contains("source = \"../fetcher\""), "{lock}");
    assert!(!manifest.contains(dir.to_str().unwrap()), "{manifest}");
    assert!(!lock.contains(dir.to_str().unwrap()), "{lock}");
    // `update` resolves the relative path against the project, not the working directory.
    let updated = guion(&project, &["update"]);
    assert!(updated.status.success(), "{}", text(&updated));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_project_with_no_approval_table_approves_nothing() {
    let dir = scratch("lib-default-deny");
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
    let refused = guion(
        &project,
        &["add", "fetcher", "--path", library.to_str().unwrap()],
    );
    assert!(!refused.status.success());
    assert!(
        text(&refused).contains("files: +modes [\"read\"]")
            && text(&refused).contains("--approve-capabilities"),
        "{}",
        text(&refused)
    );
    let approved = guion(
        &project,
        &[
            "add",
            "fetcher",
            "--path",
            library.to_str().unwrap(),
            "--approve-capabilities",
        ],
    );
    assert!(approved.status.success(), "{}", text(&approved));
    let manifest = std::fs::read_to_string(project.join("guion.toml")).unwrap();
    assert!(
        manifest.contains("[approved-capabilities.fetcher]"),
        "{manifest}"
    );
    // A package that declares nothing needs no approval.
    let quiet = dir.join("quiet");
    write_library(&quiet, "", "pub fn one() -> Int:\n    return 1\n");
    std::fs::write(
        quiet.join("guion.toml"),
        "[package]\nname = \"quiet\"\nversion = \"1.0.0\"\nentry = \"src/lib.gu\"\nexports = [\"src/lib.gu\"]\n",
    )
    .unwrap();
    let added = guion(
        &project,
        &["add", "quiet", "--path", quiet.to_str().unwrap()],
    );
    assert!(added.status.success(), "{}", text(&added));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn sqlite_is_reported_as_file_access() {
    let dir = scratch("sqlite-effects");
    std::fs::write(
        dir.join("main.gu"),
        "import \"std:sqlite\"\n\nfn open_it(path: Text) -> Bool:\n    return is_ok(sqlite_open(path))\n\nfn main() -> Int:\n    return 0\n",
    )
    .unwrap();
    let out = guion(&dir, &["doc", "--json", "main.gu"]);
    assert!(out.status.success(), "{}", text(&out));
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let effects: Vec<String> = report["functions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == "open_it")
        .map(|f| {
            f["effects"]
                .as_array()
                .unwrap()
                .iter()
                .map(|e| e.as_str().unwrap().to_owned())
                .collect()
        })
        .unwrap();
    assert!(
        effects.contains(&"filesystem.read".to_owned())
            && effects.contains(&"filesystem.write".to_owned()),
        "{effects:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
