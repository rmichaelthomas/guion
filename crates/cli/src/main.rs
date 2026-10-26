use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    env, fs,
    path::{Component, Path},
    process::ExitCode,
    time::{Duration, Instant},
};
mod bytecode_ir;
mod compiler_driver;
#[cfg(feature = "python-interop")]
mod dependencies;
mod native_dependencies;
mod native_packages;
mod source_run_cache;
use guion_model::{Artifact, BuildMetadata, Op, ResourceLimits, Value};
use guion_runtime::{EffectKind, EffectOutcome, HostPolicy, Runtime};

fn main() -> ExitCode {
    let args = env::args().skip(1).collect();
    match execute(args) {
        Ok(code) => code,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::from(1)
        }
    }
}

fn execute(args: Vec<String>) -> Result<ExitCode, String> {
    if args.first().is_some_and(|command| command == "cache") {
        return execute_cache_command(&args);
    }
    let cli_started = Instant::now();
    let phase_timings_enabled = env::var_os("GUION_PHASE_TIMINGS").is_some();
    let seed_verify_started = Instant::now();
    compiler_driver::verify_embedded_seed()
        .map_err(|error| render_diagnostic("<compiler-seed>", &error))?;
    let initial_seed_verify_ns = seed_verify_started.elapsed().as_nanos();
    let Some(command) = args.first().map(String::as_str) else {
        return Err(
            "usage: guion <new|check|fmt|lint|test|run|build|package|verify-package|add|sync|update|inspect|doc|doctor|install> [path]"
                .into(),
        );
    };
    if command == "--version" || command == "version" {
        println!(
            "guion {} (artifact format {})",
            env!("CARGO_PKG_VERSION"),
            guion_model::Artifact::CURRENT_VERSION
        );
        return Ok(ExitCode::SUCCESS);
    }
    if command == "assemble-ir" {
        let Some(input) = args.get(1) else {
            return Err("usage: guion assemble-ir <input.gui.json> --output <output.gub>".into());
        };
        let output = args
            .iter()
            .position(|argument| argument == "--output" || argument == "-o")
            .and_then(|index| args.get(index + 1))
            .ok_or("usage: guion assemble-ir <input.gui.json> --output <output.gub>")?;
        bytecode_ir::assemble_file(input, output)?;
        println!("assembled {input} -> {output}");
        return Ok(ExitCode::SUCCESS);
    }
    if command == "doctor" {
        if args.iter().any(|argument| argument == "--json") {
            println!("{}", python_doctor_report());
            return Ok(ExitCode::SUCCESS);
        }
        #[cfg(feature = "python-interop")]
        let python = env::var_os("GUION_PYTHON")
            .map(|path| {
                format!(
                    "explicit interpreter: {}",
                    std::path::PathBuf::from(path).display()
                )
            })
            .unwrap_or_else(|| "not configured (set GUION_PYTHON to opt in)".into());
        #[cfg(not(feature = "python-interop"))]
        let python = "disabled (rebuild with --features python-interop to enable)";
        println!(
            "Guion {}\nHost: {}-{}\nPython sidecar: {python}\n{}",
            env!("CARGO_PKG_VERSION"),
            env::consts::OS,
            env::consts::ARCH,
            python_doctor_status()
        );
        return Ok(ExitCode::SUCCESS);
    }
    if command == "run"
        && args
            .iter()
            .any(|argument| argument == "--help" || argument == "-h")
    {
        println!("usage: guion run [--json] [--no-cache] [--file-root DIR] [--allow-http-host HOST] [--allow-clock] <source|artifact|project> [ARGS...]");
        return Ok(ExitCode::SUCCESS);
    }
    if command == "sync" {
        if args.iter().any(|argument| argument == "--locked") {
            if !args.iter().any(|argument| argument == "--offline") {
                return Err("native sync currently requires --locked --offline".into());
            }
            let project = std::env::current_dir()
                .map_err(|error| format!("cannot inspect current directory: {error}"))?;
            println!("{}", native_packages::sync_locked_offline(&project)?);
            return Ok(ExitCode::SUCCESS);
        }
        #[cfg(not(feature = "python-interop"))]
        {
            if !args.iter().any(|argument| argument == "--offline") {
                return Err("native sync requires `guion sync --locked --offline`".into());
            }
            let project = args
                .iter()
                .skip(1)
                .find(|argument| !argument.starts_with('-'))
                .map(Path::new)
                .map(Path::to_path_buf)
                .unwrap_or(
                    std::env::current_dir()
                        .map_err(|error| format!("cannot inspect current directory: {error}"))?,
                );
            println!("{}", native_packages::sync_locked_offline(&project)?);
            return Ok(ExitCode::SUCCESS);
        }
        #[cfg(feature = "python-interop")]
        {
            let update = args.iter().any(|argument| argument == "--update");
            let json_report = args.iter().any(|argument| argument == "--json");
            let Some(path) = args
                .iter()
                .skip(1)
                .find(|argument| argument.as_str() != "--update" && argument.as_str() != "--json")
            else {
                let message = "usage: guion sync [--json] [--update] <project>";
                if json_report {
                    print_sync_failure("E_SYNC_ARGUMENT", message);
                    return Ok(ExitCode::from(1));
                }
                return Err(message.into());
            };
            if !Path::new(path).is_dir() {
                let message = format!("project directory does not exist: {path}");
                if json_report {
                    print_sync_failure("E_PROJECT_NOT_FOUND", &message);
                    return Ok(ExitCode::from(1));
                }
                return Err(message);
            }
            let (_, root) = match resolve_target(path) {
                Ok(target) => target,
                Err(message) if json_report => {
                    print_sync_failure("E_SYNC_PROJECT", &message);
                    return Ok(ExitCode::from(1));
                }
                Err(message) => return Err(message),
            };
            match dependencies::sync(&root, update) {
                Ok(message) if json_report => {
                    let lock_path = project_lock_path(&root);
                    let bridge_lock = project_bridge_root(&root).join("uv.lock");
                    let lock_digest = file_sha256(&lock_path).ok();
                    let bridge_lock_digest = file_sha256(&bridge_lock).ok();
                    println!(
                        "{}",
                        serde_json::json!({
                            "schema_version": 1,
                            "kind": "guion_sync_report",
                            "status": "succeeded",
                            "project": root,
                            "updated": update,
                            "lock_sha256": lock_digest,
                            "bridge_lock_sha256": bridge_lock_digest,
                            "message": message,
                        })
                    );
                }
                Ok(message) => println!("{message}"),
                Err(error) if json_report => {
                    print_sync_failure(error.code, &error.message);
                    return Ok(ExitCode::from(1));
                }
                Err(error) => return Err(error.message),
            }
            return Ok(ExitCode::SUCCESS);
        }
    }
    if command == "new" {
        let Some(name) = args.get(1) else {
            return Err("usage: guion new <directory>".into());
        };
        let root = std::path::Path::new(name);
        if root.exists() {
            return Err(format!("project path already exists: {}", root.display()));
        }
        fs::create_dir_all(root.join("src")).map_err(|e| format!("cannot create project: {e}"))?;
        let project_name = project_name(root);
        let source = "fn main() -> Int:\n    print(\"Hello, Guion!\")\n    return 0\n\nfn test_starter() -> Unit:\n    assert(1 + 1 == 2)\n";
        fs::write(root.join("src/main.gu"), source)
            .map_err(|e| format!("cannot write starter source: {e}"))?;
        fs::write(
            root.join("guion.toml"),
            format!("[project]\nname = \"{project_name}\"\nversion = \"0.1.0\"\nentry = \"src/main.gu\"\n"),
        )
        .map_err(|e| format!("cannot write project manifest: {e}"))?;
        fs::write(
            root.join("README.md"),
            format!(
                "# {project_name}\n\nA small Guion project. Start with `guion check`, `guion test`, and `guion run src/main.gu`. Generate API documentation with `guion doc src/main.gu`; the language reference is included with your Guion installation.\n"
            ),
        )
        .map_err(|e| format!("cannot write project guide: {e}"))?;
        println!("created {}", root.display());
        return Ok(ExitCode::SUCCESS);
    }
    if command == "install" {
        let Some(package) = args.get(1) else {
            return Err("usage: guion install <package-directory> [--prefix DIRECTORY]".into());
        };
        let prefix = args
            .iter()
            .position(|argument| argument == "--prefix")
            .and_then(|index| args.get(index + 1))
            .map(Path::new)
            .map(Path::to_path_buf)
            .or_else(|| env::var_os("HOME").map(|home| Path::new(&home).join(".local")))
            .ok_or("cannot determine install prefix; pass --prefix DIRECTORY")?;
        let executable = install_application(Path::new(package), &prefix)?;
        println!("installed application at {}", executable.display());
        return Ok(ExitCode::SUCCESS);
    }
    if command == "add" {
        let Some(name) = args.get(1) else {
            return Err(
                "usage: guion add NAME --path PATH | --url URL --version VERSION --sha256 HASH"
                    .into(),
            );
        };
        let project = std::env::current_dir()
            .map_err(|error| format!("cannot inspect current directory: {error}"))?;
        let path_index = args.iter().position(|argument| argument == "--path");
        if let Some(index) = path_index {
            let path = args.get(index + 1).ok_or("--path requires a directory")?;
            println!(
                "{}",
                native_packages::add_path(&project, name, Path::new(path))?
            );
            return Ok(ExitCode::SUCCESS);
        }
        let get_option = |option: &str| {
            args.iter()
                .position(|argument| argument == option)
                .and_then(|index| args.get(index + 1))
        };
        if let (Some(url), Some(version), Some(sha256)) = (
            get_option("--url"),
            get_option("--version"),
            get_option("--sha256"),
        ) {
            println!(
                "{}",
                native_packages::add_url(&project, name, url, version, sha256)?
            );
            return Ok(ExitCode::SUCCESS);
        }
        return Err(
            "usage: guion add NAME --path PATH | --url URL --version VERSION --sha256 HASH".into(),
        );
    }
    if command == "update" {
        let project = std::env::current_dir()
            .map_err(|error| format!("cannot inspect current directory: {error}"))?;
        println!("{}", native_packages::update_paths(&project)?);
        return Ok(ExitCode::SUCCESS);
    }
    if command == "package" {
        if args.iter().any(|argument| argument == "--library") {
            let source = args
                .iter()
                .position(|argument| argument == "--library")
                .and_then(|index| args.get(index + 1))
                .ok_or("usage: guion package --library <source-dir> --output <directory>")?;
            let output = args
                .iter()
                .position(|argument| argument == "--output" || argument == "-o")
                .and_then(|index| args.get(index + 1))
                .ok_or("library package output directory is missing")?;
            let digest = native_packages::package_library(Path::new(source), Path::new(output))?;
            println!("packaged native library {} ({digest})", output);
            return Ok(ExitCode::SUCCESS);
        }
        let json_package = args.iter().any(|argument| argument == "--json");
        let project_index = if args.get(1).is_some_and(|argument| argument == "--json") {
            2
        } else {
            1
        };
        let Some(project) = args.get(project_index) else {
            let message = "usage: guion package [--json] <project-dir> [--output <directory>]";
            if json_package {
                print_cli_diagnostic("E_PACKAGE_ARGUMENT", message);
                return Ok(ExitCode::from(1));
            }
            return Err(message.into());
        };
        let project_path = std::path::Path::new(project);
        if !project_path.is_dir() {
            if json_package {
                print_cli_diagnostic(
                    "E_PROJECT_NOT_FOUND",
                    &format!("project directory does not exist: {project}"),
                );
                return Ok(ExitCode::from(1));
            }
            return Err("guion package requires a project directory".into());
        }
        let (_, root) = match resolve_target(project) {
            Ok(target) => target,
            Err(message) if json_package => {
                print_cli_diagnostic("E_PACKAGE_PROJECT", &message);
                return Ok(ExitCode::from(1));
            }
            Err(message) => return Err(message),
        };
        let output = if let Some(index) = args
            .iter()
            .position(|argument| argument == "--output" || argument == "-o")
        {
            let Some(output) = args.get(index + 1) else {
                let message = "package output directory is missing";
                if json_package {
                    print_cli_diagnostic("E_PACKAGE_ARGUMENT", message);
                    return Ok(ExitCode::from(1));
                }
                return Err(message.into());
            };
            std::path::PathBuf::from(output)
        } else {
            root.join("dist").join(project_name(&root))
        };
        if let Err(message) = package_project(&root, &output) {
            if json_package {
                if message.starts_with("package output already exists:") {
                    print_cli_diagnostic("E_PACKAGE_OUTPUT_EXISTS", &message);
                } else {
                    println!("{}", package_failure_diagnostic(&message));
                }
                return Ok(ExitCode::from(1));
            }
            return Err(message);
        }
        if json_package {
            let metadata_path = output.join("PACKAGE.json");
            let metadata_bytes = fs::read(&metadata_path).map_err(|error| {
                format!(
                    "cannot read package metadata {}: {error}",
                    metadata_path.display()
                )
            })?;
            let metadata: serde_json::Value = serde_json::from_slice(&metadata_bytes)
                .map_err(|error| format!("invalid package metadata: {error}"))?;
            println!(
                "{}",
                serde_json::json!({
                    "schema_version": 1,
                    "kind": "guion_package_report",
                    "status": "packaged",
                    "directory": output,
                    "package_schema_version": metadata["schema_version"],
                    "target": metadata["target"],
                    "artifact_sha256": metadata["artifact_sha256"],
                    "package_metadata_sha256": format!("{:x}", Sha256::digest(&metadata_bytes)),
                    "resource_count": metadata["resources"].as_array().map_or(0, Vec::len),
                    "python_runtime_included": !metadata["python_runtime"].is_null(),
                })
            );
        } else {
            println!("packaged {}", output.display());
        }
        return Ok(ExitCode::SUCCESS);
    }
    if command == "verify-package" {
        let json_verification = args.iter().any(|argument| argument == "--json");
        let path_index = if args.get(1).is_some_and(|argument| argument == "--json") {
            2
        } else {
            1
        };
        let Some(path) = args.get(path_index) else {
            let message = "usage: guion verify-package [--json] <package-directory>";
            if json_verification {
                print_cli_diagnostic("E_PACKAGE_ARGUMENT", message);
                return Ok(ExitCode::from(1));
            }
            return Err(message.into());
        };
        if let Err(message) = verify_package(Path::new(path)) {
            if json_verification {
                print_cli_diagnostic("E_PACKAGE_VERIFY_FAILED", &message);
                return Ok(ExitCode::from(1));
            }
            return Err(message);
        }
        if json_verification {
            let metadata_bytes = fs::read(Path::new(path).join("PACKAGE.json"))
                .map_err(|error| format!("cannot read package metadata: {error}"))?;
            let metadata: serde_json::Value = serde_json::from_slice(&metadata_bytes)
                .map_err(|error| format!("invalid package metadata: {error}"))?;
            println!(
                "{}",
                serde_json::json!({
                    "schema_version": 1,
                    "kind": "guion_package_verification_report",
                    "status": "verified",
                    "directory": path,
                    "package_schema_version": metadata["schema_version"],
                    "target": metadata["target"],
                    "package_metadata_sha256": format!("{:x}", Sha256::digest(&metadata_bytes)),
                })
            );
        } else {
            println!("verified package {}", Path::new(path).display());
        }
        return Ok(ExitCode::SUCCESS);
    }
    if ![
        "check", "fmt", "lint", "test", "run", "build", "inspect", "doc",
    ]
    .contains(&command)
    {
        return Err(format!("unknown command `{command}`\nusage: guion <new|check|fmt|lint|test|run|build|package|verify-package|add|sync|update|inspect|doc|doctor|install> [path]"));
    }
    let mut json_run = false;
    let mut no_source_cache = false;
    let mut file_root_overrides = Vec::new();
    let mut http_host_overrides = Vec::new();
    let mut clock_override = false;
    let json_inspect = command == "inspect" && args.iter().any(|argument| argument == "--json");
    let json_doc = command == "doc" && args.get(1).is_some_and(|arg| arg == "--json");
    let json_build = command == "build" && args.get(1).is_some_and(|arg| arg == "--json");
    let json_lint = command == "lint" && args.iter().any(|argument| argument == "--json");
    let json_check = command == "check" && args.iter().any(|argument| argument == "--json");
    let path_index = if command == "run" {
        let mut index = 1;
        while let Some(argument) = args.get(index).map(String::as_str) {
            match argument {
                "--json" if !json_run => {
                    json_run = true;
                    index += 1;
                }
                "--no-cache" => {
                    no_source_cache = true;
                    index += 1;
                }
                "--file-root" => {
                    let Some(root) = args.get(index + 1) else {
                        return Err("usage: guion run [--json] [--file-root DIR] <source|artifact|project> [ARGS...]".into());
                    };
                    file_root_overrides.push(root.clone());
                    index += 2;
                }
                "--allow-http-host" => {
                    let Some(host) = args.get(index + 1) else {
                        return Err("usage: guion run [--json] [--file-root DIR] [--allow-http-host HOST] [--allow-clock] <source|artifact|project> [ARGS...]".into());
                    };
                    http_host_overrides.push(host.clone());
                    index += 2;
                }
                "--allow-clock" => {
                    clock_override = true;
                    index += 1;
                }
                _ => break,
            }
        }
        index
    } else if (command == "fmt" && args.get(1).is_some_and(|arg| arg == "--check"))
        || json_doc
        || (json_check && args.get(1).is_some_and(|arg| arg == "--json"))
        || (json_lint && args.get(1).is_some_and(|arg| arg == "--json"))
        || json_build
        || (json_inspect && args.get(1).is_some_and(|arg| arg == "--json"))
    {
        2
    } else {
        1
    };
    let Some(path) = args.get(path_index) else {
        return Err(format!("usage: guion {command} <source.gu>"));
    };
    let (source_path, project_root) = resolve_target(path)?;
    let path = source_path.to_string_lossy().into_owned();
    let mut runtime_file_roots = Vec::with_capacity(file_root_overrides.len());
    for root in &file_root_overrides {
        let root = Path::new(root);
        if !root.is_dir() {
            return Err(format!(
                "file-access root is not a directory: {}",
                root.display()
            ));
        }
        runtime_file_roots.push(root.canonicalize().map_err(|error| {
            format!(
                "cannot resolve file-access root {}: {error}",
                root.display()
            )
        })?);
    }
    if command == "fmt" {
        let source =
            fs::read_to_string(&path).map_err(|error| format!("cannot read {path}: {error}"))?;
        compile_project(&source_path, &project_root)
            .map_err(|error| render_diagnostic(&path, &error))?;
        let formatted = format_source(&source);
        if args.iter().any(|arg| arg == "--check") {
            if formatted != source {
                return Err(format!("{path} needs formatting"));
            }
            println!("formatted {path}");
        } else {
            fs::write(&path, formatted).map_err(|error| format!("cannot write {path}: {error}"))?;
            println!("formatted {path}");
        }
        return Ok(ExitCode::SUCCESS);
    }
    if command == "lint" {
        let source =
            fs::read_to_string(&path).map_err(|error| format!("cannot read {path}: {error}"))?;
        if let Err(error) = compile_project(&source_path, &project_root) {
            if json_lint {
                println!("{}", render_diagnostic_json(&path, &error));
                return Ok(ExitCode::from(1));
            }
            return Err(render_diagnostic(&path, &error));
        }
        let diagnostics = lint_source(&source, &source_path, &project_root);
        if json_lint {
            println!(
                "{}",
                serde_json::json!({
                    "schema_version": 1,
                    "kind": "guion_lint_report",
                    "source": path,
                    "status": "passed",
                    "diagnostics": diagnostics,
                })
            );
        } else if diagnostics.is_empty() {
            println!("linted {path} (no warnings)");
        } else {
            for diagnostic in &diagnostics {
                eprintln!(
                    "{}:{}:{}: warning[{}]: {}",
                    path,
                    diagnostic["line"],
                    diagnostic["column"],
                    diagnostic["code"],
                    diagnostic["message"]
                );
            }
            println!("linted {path} ({} warnings)", diagnostics.len());
        }
        return Ok(ExitCode::SUCCESS);
    }
    let mut source_cache_status = "not_applicable";
    let mut compiler_phase_timings = None;
    let mut source_compile_or_cache_ns = None;
    let mut input_artifact_decode_ns = None;
    let artifact = if command == "run" || command == "inspect" {
        let bytes = fs::read(&path).map_err(|e| format!("cannot read {path}: {e}"))?;
        if path.ends_with(".gub") || bytes.starts_with(b"VRTA") {
            let decode_started = Instant::now();
            let decoded = Artifact::from_bytes(&bytes);
            input_artifact_decode_ns = Some(decode_started.elapsed().as_nanos());
            match decoded {
                Ok(artifact) => artifact,
                Err(error) if json_run || json_inspect => {
                    println!(
                        "{}",
                        serde_json::json!({
                            "schema_version": 1,
                            "kind": "guion_diagnostic",
                            "diagnostic": {
                                "code": error.code,
                                "message": error.message,
                                "source": path
                            }
                        })
                    );
                    return Ok(ExitCode::from(1));
                }
                Err(error) => {
                    return Err(format!("{path}: error[{}]: {}", error.code, error.message))
                }
            }
        } else {
            String::from_utf8(bytes)
                .map_err(|e| format!("{path}: source is not valid UTF-8: {e}"))?;
            let compilation = if command == "run" {
                let compile_started = Instant::now();
                let result = compile_project_for_run(
                    &source_path,
                    &project_root,
                    no_source_cache,
                    &runtime_file_roots,
                    &http_host_overrides,
                    clock_override,
                );
                source_compile_or_cache_ns = Some(compile_started.elapsed().as_nanos());
                result.map(|(artifact, cache_status, timings)| {
                    source_cache_status = cache_status;
                    compiler_phase_timings = timings;
                    artifact
                })
            } else {
                compile_project(&source_path, &project_root)
            };
            match compilation {
                Ok(artifact) => artifact,
                Err(error) if json_run || json_inspect => {
                    println!("{}", render_diagnostic_json(&path, &error));
                    return Ok(ExitCode::from(1));
                }
                Err(error) => return Err(render_diagnostic(&path, &error)),
            }
        }
    } else {
        fs::read_to_string(&path).map_err(|e| format!("cannot read {path}: {e}"))?;
        match compile_project(&source_path, &project_root) {
            Ok(artifact) => artifact,
            Err(error) if json_check || json_run || json_build || json_lint => {
                println!("{}", render_diagnostic_json(&path, &error));
                return Ok(ExitCode::from(1));
            }
            Err(error) => return Err(render_diagnostic(&path, &error)),
        }
    };
    match command {
        "check" => {
            if json_check {
                println!(
                    "{}",
                    serde_json::json!({
                        "schema_version": 1,
                        "kind": "guion_check_report",
                        "source": path,
                        "status": "passed",
                        "function_count": artifact.functions.len()
                    })
                );
            } else {
                println!("checked {} ({} functions)", path, artifact.functions.len());
            }
        }
        "test" => {
            let json_report = args.iter().any(|argument| argument == "--json");
            if !run_tests(&path, &artifact, &project_root, json_report)? {
                return Ok(ExitCode::from(1));
            }
        }
        "build" => {
            let output =
                if let Some(i) = args.iter().position(|arg| arg == "-o" || arg == "--output") {
                    args.get(i + 1)
                        .cloned()
                        .ok_or("build output path is missing")?
                } else {
                    source_path
                        .with_extension("gub")
                        .to_string_lossy()
                        .into_owned()
                };
            let mut artifact = artifact;
            artifact.build_metadata = Some(build_metadata(
                runtime_policy(&project_root)?,
                &artifact,
                &project_root,
            )?);
            let bytes = artifact
                .to_bytes()
                .map_err(|e| format!("error[{}]: {}", e.code, e.message))?;
            fs::write(&output, &bytes).map_err(|e| format!("cannot write {output}: {e}"))?;
            if json_build {
                println!(
                    "{}",
                    serde_json::json!({
                        "schema_version": 1,
                        "kind": "guion_build_report",
                        "source": path,
                        "artifact": output,
                        "artifact_format": artifact.format_version,
                        "artifact_size_bytes": bytes.len(),
                        "artifact_sha256": format!("{:x}", Sha256::digest(&bytes)),
                        "source_sha256": artifact.compiled_source_sha256,
                        "compiler_version": env!("CARGO_PKG_VERSION"),
                        "target": format!("{}-{}", env::consts::OS, env::consts::ARCH),
                    })
                );
            } else {
                println!(
                    "built {output} (artifact format {})",
                    artifact.format_version
                );
            }
        }
        "inspect" => {
            if json_inspect {
                println!(
                    "{}",
                    inspection_report(&path, &artifact, &project_root, path.ends_with(".gub"))
                );
                return Ok(ExitCode::SUCCESS);
            }
            let effects_report =
                inspection_report(&path, &artifact, &project_root, path.ends_with(".gub"));
            println!("capabilities (static reachability; not a sandbox):");
            if let Some(functions) = effects_report["functions"].as_object() {
                for function in &artifact.functions {
                    let entry = &functions[&function.name];
                    let effects = entry["effects"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(serde_json::Value::as_str)
                        .collect::<Vec<_>>();
                    println!(
                        "  {}{}: {}",
                        function.name,
                        if entry["entry_reachable"] == true {
                            " (entry-reachable)"
                        } else {
                            ""
                        },
                        if effects.is_empty() {
                            "no known host effects".to_owned()
                        } else {
                            effects.join(", ")
                        }
                    );
                }
            }
            if !artifact.externals.is_empty() {
                println!("  Python declarations are opaque external effects; declared types do not prove behavior.");
            }
            println!(
                "artifact format: {}\nentry: {}\nsource: {}",
                artifact.format_version, artifact.entry, artifact.source_name
            );
            if let Some(metadata) = &artifact.build_metadata {
                let limits = &metadata.resource_limits;
                println!(
                    "metadata schema: {}\ncompiler: {}\ntarget: {}\nsource sha256: {}\npython dependency locks: {}\nresource limits:\n  instruction_limit: {}\n  allocation_limit_bytes: {}\n  call_depth_limit: {}\n  output_limit_bytes: {}\n  max_integer_digits: {}\n  max_text_bytes: {}\n  max_collection_items: {}\n  max_parallelism: {}\n  max_file_bytes: {}",
                    metadata.metadata_schema_version,
                    metadata.compiler_version,
                    metadata.target,
                    metadata.source_sha256.as_deref().unwrap_or("absent (legacy artifact)"),
                    metadata
                        .dependency_lock_sha256
                        .as_deref()
                        .unwrap_or("absent"),
                    limits.instruction_limit,
                    limits.allocation_limit_bytes,
                    limits.call_depth_limit,
                    limits.output_limit_bytes,
                    limits.max_integer_digits,
                    limits.max_text_bytes,
                    limits.max_collection_items,
                    limits.max_parallelism,
                    limits.max_file_bytes,
                );
            } else {
                println!("build metadata: absent (legacy or unconfigured artifact)");
            }
            for f in &artifact.functions {
                println!(
                    "fn {}({}) -> {}; {} instructions",
                    f.name,
                    f.parameters
                        .iter()
                        .map(display_type)
                        .collect::<Vec<_>>()
                        .join(", "),
                    display_type(&f.result),
                    f.code.len()
                );
            }
            for external in &artifact.externals {
                println!(
                    "extern python {}:{} as {} (opaque external effect)",
                    external.module, external.callable, external.name
                );
            }
        }
        "doc" if json_doc => {
            println!("{}", api_report(&path, &project_root, &artifact));
        }
        "doc" => {
            print_api_report_human(&api_report(&path, &project_root, &artifact));
        }
        "run" => {
            #[cfg(feature = "python-interop")]
            let managed_interpreter = dependencies::interpreter_for_locked_run(&project_root)?;
            let runtime = Runtime::new();
            let program_args = args.iter().skip(path_index + 1).cloned().collect();
            let mut policy = runtime_policy(&project_root)?
                .with_file_access(project_root.clone(), true, true)
                .with_arguments(program_args);
            for host in http_host_overrides {
                policy
                    .allowed_http_hosts
                    .insert(normalize_http_host(&host)?);
            }
            if clock_override {
                policy = policy.with_clock_access();
            }
            for root in runtime_file_roots {
                policy = policy.with_additional_file_root(root);
            }
            #[cfg(feature = "python-interop")]
            if let Some(interpreter) = managed_interpreter {
                policy = policy.with_python(interpreter, std::time::Duration::from_secs(15));
            } else if let Some(interpreter) = env::var_os("GUION_PYTHON") {
                policy = policy.with_python(interpreter, std::time::Duration::from_secs(15));
            }
            let runtime_load_started = Instant::now();
            let mut instance = match runtime.load(artifact.clone(), policy) {
                Ok(instance) => instance,
                Err(error) if json_run => {
                    println!(
                        "{}",
                        serde_json::json!({
                            "schema_version": 1,
                            "kind": "guion_run_report",
                            "source": path,
                            "status": "failed",
                            "program_stdout": "",
                            "host_stderr": "",
                            "python_native_image_observation": "not_used",
                            "python_native_images": [],
                            "diagnostic": runtime_diagnostic_json(&path, &artifact, &error)
                        })
                    );
                    return Ok(ExitCode::from(1));
                }
                Err(error) => return Err(render_runtime(&path, &artifact, &error)),
            };
            let runtime_load_ns = runtime_load_started.elapsed().as_nanos();
            let program_started = Instant::now();
            let result = instance.invoke("main", vec![]);
            let program_execute_ns = program_started.elapsed().as_nanos();
            let program_stdout = instance.take_output();
            let program_stderr = instance.take_program_stderr();
            let host_stderr = instance.take_host_stderr();
            let python_native_image_observation =
                instance.python_native_image_observation().to_owned();
            let python_native_images = instance.python_native_images().to_vec();
            let native_backend = serde_json::json!({
                "compiled_functions": instance.native_function_names(),
                "compilations": instance.native_compilations(),
                "bytecode_instructions": instance.native_bytecode_instructions(),
                "bytecode_blocks": instance.native_bytecode_blocks(),
                "cranelift_ir_blocks": instance.native_cranelift_ir_blocks(),
                "cranelift_ir_instructions": instance.native_cranelift_ir_instructions(),
                "compile_profile": instance.native_compile_profile(),
                "calls": instance.native_calls(),
                "deoptimizations": instance.native_deoptimizations()
            });
            let effects = instance
                .effect_report()
                .iter()
                .map(|effect| {
                    let kind = match effect.kind {
                        EffectKind::ProgramOutput => "program_output",
                        EffectKind::ProgramErrorOutput => "program_error_output",
                        EffectKind::FileRead => "file_read",
                        EffectKind::FileWrite => "file_write",
                        EffectKind::HttpRequest => "http_request",
                        EffectKind::PythonCall => "python_call",
                        EffectKind::EnvironmentRead => "environment_read",
                        EffectKind::ClockRead => "clock_read",
                    };
                    let outcome = match effect.outcome {
                        EffectOutcome::Started => "started",
                        EffectOutcome::Returned => "returned",
                        EffectOutcome::ReturnedError => "returned_error",
                        EffectOutcome::Interrupted => "interrupted",
                    };
                    serde_json::json!({
                        "kind": kind,
                        "target": effect.target.as_str(),
                        "target_truncated": effect.target_truncated,
                        "outcome": outcome
                    })
                })
                .collect::<Vec<_>>();
            let omitted_effects = instance.omitted_effects();
            instance.close();
            if json_run {
                let (status, exit_code, diagnostic) = match result {
                    Ok(Value::Unit) => ("passed", 0, None),
                    Ok(Value::Int(value)) => match value.to_string().parse::<u8>() {
                        Ok(code) => ("passed", code, None),
                        Err(_) => (
                            "failed",
                            1,
                            Some(serde_json::json!({
                                "code": "E_MAIN_EXIT_CODE",
                                "message": "main returned an exit code outside 0..255",
                                "source": path
                            })),
                        ),
                    },
                    Ok(_) => (
                        "failed",
                        1,
                        Some(serde_json::json!({
                            "code": "E_MAIN_RETURN",
                            "message": "main must return Int or Unit",
                            "source": path
                        })),
                    ),
                    Err(error) => (
                        "failed",
                        1,
                        Some(runtime_diagnostic_json(&path, &artifact, &error)),
                    ),
                };
                let mut report = serde_json::json!({
                    "schema_version": 1,
                    "kind": "guion_run_report",
                    "source": path,
                    "status": status,
                    "program_stdout": program_stdout,
                    "program_stderr": program_stderr,
                    "host_stderr": host_stderr,
                    "effects": effects,
                    "omitted_effects": omitted_effects,
                    "python_native_image_observation": python_native_image_observation,
                    "python_native_images": python_native_images,
                    "source_cache": source_cache_status,
                    "native_backend": native_backend,
                    "exit_code": exit_code,
                    "diagnostic": diagnostic
                });
                if phase_timings_enabled {
                    report["phase_timings_ns"] = serde_json::json!({
                        "cli_to_report_excluding_process_startup_ns": cli_started.elapsed().as_nanos(),
                        "initial_embedded_seed_validation_ns": initial_seed_verify_ns,
                        "input_artifact_decode_ns": input_artifact_decode_ns,
                        "source_compile_or_cache_ns": source_compile_or_cache_ns,
                        "compiler_pipeline": compiler_phase_timings.as_ref().map(compiler_phase_report),
                        "artifact_runtime_load_ns": runtime_load_ns,
                        "program_execute_ns": program_execute_ns,
                        "native_compilation": native_backend["compile_profile"],
                    });
                }
                println!("{}", report);
                return Ok(ExitCode::from(exit_code));
            }
            print!("{program_stdout}");
            eprint!("{program_stderr}");
            eprint!("{host_stderr}");
            let result = result.map_err(|e| render_runtime(&path, &artifact, &e))?;
            let code = match result {
                Value::Unit => 0,
                Value::Int(value) => value
                    .to_string()
                    .parse::<u8>()
                    .map_err(|_| "main returned an exit code outside 0..255".to_owned())?,
                _ => return Err("main must return Int or Unit".into()),
            };
            return Ok(ExitCode::from(code));
        }
        _ => unreachable!(),
    }
    Ok(ExitCode::SUCCESS)
}

fn execute_cache_command(args: &[String]) -> Result<ExitCode, String> {
    if args.get(1).map(String::as_str) != Some("clean") || args.len() > 3 {
        return Err("usage: guion cache clean [project]".into());
    }
    let project = if let Some(path) = args.get(2) {
        Path::new(path).to_path_buf()
    } else {
        env::current_dir().map_err(|error| format!("cannot inspect current directory: {error}"))?
    };
    let project_text = project
        .to_str()
        .ok_or("project path is not valid Unicode")?;
    let (_, project_root) = resolve_target(project_text)?;
    let removed_bytes = source_run_cache::clean_project_cache(&project_root)?;
    println!(
        "cleaned Guion source cache for {} ({} bytes removed)",
        project_root.display(),
        removed_bytes
    );
    Ok(ExitCode::SUCCESS)
}

fn package_project(root: &std::path::Path, output: &std::path::Path) -> Result<(), String> {
    use std::path::Path;

    if cfg!(not(unix)) {
        return Err("current-host packaging currently requires a Unix host".into());
    }
    match fs::symlink_metadata(output) {
        Ok(_) => {
            return Err(format!(
                "package output already exists: {}; choose a new directory",
                output.display()
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(format!(
                "cannot inspect package output {}: {error}",
                output.display()
            ));
        }
    }
    let root = root
        .canonicalize()
        .map_err(|error| format!("cannot resolve project root: {error}"))?;
    let application = project_identity(&root)?;
    let manifest_path = project_manifest_path(&root);
    let manifest_text = fs::read_to_string(&manifest_path)
        .map_err(|error| format!("cannot read {}: {error}", manifest_path.display()))?;
    let manifest: toml::Value = toml::from_str(&manifest_text)
        .map_err(|error| format!("{}: invalid TOML: {error}", manifest_path.display()))?;
    let has_python_project = manifest.get("python").is_some();
    if has_python_project && !cfg!(feature = "python-interop") {
        return Err("this project requires optional Python interoperability; rebuild with --features python-interop".into());
    }
    let resources = package_resources(&manifest, &root)?;
    let (entry, _) = resolve_target(root.to_str().ok_or("project path is not valid Unicode")?)?;
    let entry_text = entry.to_string_lossy().into_owned();
    let mut artifact =
        compile_project(&entry, &root).map_err(|error| render_diagnostic(&entry_text, &error))?;
    if !has_python_project && !artifact.externals.is_empty() {
        return Err(
            "this project declares Python calls but has no managed [python] project to bundle"
                .into(),
        );
    }
    artifact.build_metadata = Some(build_metadata(runtime_policy(&root)?, &artifact, &root)?);
    let artifact_bytes = artifact
        .to_bytes()
        .map_err(|error| format!("error[{}]: {}", error.code, error.message))?;

    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)
        .map_err(|error| format!("cannot create package parent {}: {error}", parent.display()))?;
    let stage = parent.join(format!(
        ".guion-package-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    fs::create_dir(&stage)
        .map_err(|error| format!("cannot create package staging directory: {error}"))?;
    let build = (|| {
        fs::write(stage.join("app.gub"), &artifact_bytes)
            .map_err(|error| format!("cannot write packaged artifact: {error}"))?;
        fs::copy(&manifest_path, stage.join("guion.toml"))
            .map_err(|error| format!("cannot copy project manifest: {error}"))?;
        let executable = env::current_exe()
            .map_err(|error| format!("cannot locate the Guion runtime executable: {error}"))?;
        fs::copy(&executable, stage.join("guion"))
            .map_err(|error| format!("cannot copy the Guion runtime: {error}"))?;
        let rust_notices = copy_rust_notices(executable.parent().unwrap(), &stage)?;
        let (runtime_sha256, runtime_size_bytes) = sha256_path(&stage.join("guion"))?;
        #[cfg(feature = "python-interop")]
        let python_bundle = dependencies::bundle_python(&root, &stage)?;
        #[cfg(feature = "python-interop")]
        let python_runtime = python_bundle
            .as_ref()
            .map(|bundle| {
                serde_json::json!({
                    "version": bundle.version,
                    "uv_version": bundle.uv_version,
                    "uv_sha256": bundle.uv_sha256,
                    "uv_lock_sha256": bundle.lock_sha256,
                    "python_runtime_sha256": bundle.python_sha256,
                    "python_runtime_size_bytes": bundle.python_size_bytes,
                    "python_cache_sha256": bundle.python_cache_sha256,
                    "python_cache_size_bytes": bundle.python_cache_size_bytes,
                    "dependency_report_sha256": bundle.dependency_report_sha256,
                })
            })
            .unwrap_or(serde_json::Value::Null);
        #[cfg(not(feature = "python-interop"))]
        let python_runtime = serde_json::Value::Null;
        let mut resource_records = Vec::with_capacity(resources.len());
        for (source, relative) in &resources {
            let destination = stage.join(relative);
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent)
                    .map_err(|error| format!("cannot create resource directory: {error}"))?;
            }
            fs::copy(source, &destination).map_err(|error| {
                format!(
                    "cannot copy package resource {}: {error}",
                    relative.display()
                )
            })?;
            let (digest, size_bytes) = sha256_path(&destination)?;
            resource_records.push(serde_json::json!({
                "path": relative.to_string_lossy(),
                "sha256": digest,
                "size_bytes": size_bytes,
            }));
        }
        let native_report_sha256 = native_dependencies::write_report(&stage)?;
        let launcher = "#!/bin/sh\nset -eu\nHERE=$(CDPATH= cd -- \"$(dirname -- \"$0\")\" && pwd)\nif [ \"${GUION_VERIFY_PACKAGE:-0}\" = \"1\" ]; then\n  \"$HERE/guion\" verify-package \"$HERE\" >/dev/null\nfi\nif [ -x \"$HERE/toolchains/uv\" ]; then\n  export GUION_UV=\"$HERE/toolchains/uv\"\n  export UV_PYTHON_INSTALL_DIR=\"$HERE/python\"\n  if [ -w \"$HERE/.uv-cache\" ]; then\n    export UV_CACHE_DIR=\"$HERE/.uv-cache\"\n  else\n    UV_CACHE_DIR=$(mktemp -d \"${TMPDIR:-/tmp}/guion-uv-cache.XXXXXX\")\n    export UV_CACHE_DIR\n    trap 'rm -rf \"$UV_CACHE_DIR\"' EXIT\n  fi\n  export UV_OFFLINE=1\nfi\n\"$HERE/guion\" run --file-root \"${GUION_FILE_ROOT:-$PWD}\" \"$HERE/app.gub\" \"$@\"\n";
        fs::write(stage.join("run"), launcher)
            .map_err(|error| format!("cannot write package launcher: {error}"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = fs::metadata(stage.join("run"))
                .map_err(|error| format!("cannot inspect package launcher: {error}"))?
                .permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(stage.join("run"), permissions)
                .map_err(|error| format!("cannot mark package launcher executable: {error}"))?;
        }
        fs::write(
            stage.join("PACKAGE.txt"),
            "Guion application package\n\nRun with: ./run [arguments]\nCheck it with: ./guion verify-package .\nSet GUION_VERIFY_PACKAGE=1 to make the launcher verify the package before running. Full verification hashes the bundled runtime, resources, Python environment, and native images, so it can add noticeable startup time for large bundles. Verification hashes detect accidental corruption; they do not authenticate the publisher. The package contains a source-free Guion artifact, the matching host runtime, its project policy, and the resources listed in guion.toml [package].resources. NATIVE-DEPENDENCIES.json records macOS Mach-O dependencies and wheel images; linux-dependencies.json records Linux ELF dependencies, runtime resolutions, and required glibc versions. Both reports include architectures, hashes, and available distribution provenance. Python-assisted projects also include pinned managed CPython, uv, locked bridge project, offline dependency cache, Python distribution metadata, and CPython/uv/Tcl/Tk license notices. Prepared Guion distributions also contribute target-specific Rust dependency inventory and license files. The inventory does not assess legal compatibility. On macOS, `guion run --json` reports newly resident native images loaded by Python calls; transient loads between snapshots may not appear. The package is verified for macOS ARM64 or Debian 12-compatible GNU/Linux ARM64 with glibc. Run it on its named target; other operating systems, architectures, Linux distributions, and libc implementations are unverified.\n",
        )
        .map_err(|error| format!("cannot write package instructions: {error}"))?;
        let package_metadata = serde_json::json!({
            "schema_version": 2,
            "application": application,
            "compiler_version": env!("CARGO_PKG_VERSION"),
            "target": format!("{}-{}", env::consts::OS, env::consts::ARCH),
            "artifact_sha256": format!("{:x}", Sha256::digest(&artifact_bytes)),
            "runtime_sha256": runtime_sha256,
            "runtime_size_bytes": runtime_size_bytes,
            "rust_notices": rust_notices.as_ref().map(|notices| serde_json::json!({
                "inventory_sha256": notices.inventory_sha256,
                "files_sha256": notices.files_sha256,
                "files_size_bytes": notices.files_size_bytes,
            })),
            "native_dependency_report_sha256": native_report_sha256,
            "source_sha256": artifact.build_metadata.as_ref().and_then(|metadata| metadata.source_sha256.as_deref()),
            "python_runtime": python_runtime,
            "resources": resource_records,
        });
        let package_metadata = serde_json::to_vec_pretty(&package_metadata)
            .map_err(|error| format!("cannot encode package metadata: {error}"))?;
        fs::write(stage.join("PACKAGE.json"), package_metadata)
            .map_err(|error| format!("cannot write package metadata: {error}"))?;
        Ok::<(), String>(())
    })();
    if let Err(error) = build {
        let _ = fs::remove_dir_all(&stage);
        return Err(error);
    }
    if let Err(error) = publish_package_stage(&stage, output) {
        let _ = fs::remove_dir_all(&stage);
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            return Err(format!(
                "package output already exists: {}; choose a new directory",
                output.display()
            ));
        }
        return Err(format!(
            "cannot install package at {}: {error}",
            output.display()
        ));
    }
    Ok(())
}

fn install_application(package: &Path, prefix: &Path) -> Result<std::path::PathBuf, String> {
    verify_package(package)?;
    let package = package
        .canonicalize()
        .map_err(|error| format!("cannot resolve package directory: {error}"))?;
    let metadata: serde_json::Value = serde_json::from_slice(
        &fs::read(package.join("PACKAGE.json"))
            .map_err(|error| format!("cannot read package metadata: {error}"))?,
    )
    .map_err(|error| format!("invalid package metadata: {error}"))?;
    let application = metadata.get("application").ok_or(
        "package metadata has no application identity; repackage it with this Guion version",
    )?;
    let name = application
        .get("name")
        .and_then(serde_json::Value::as_str)
        .ok_or("package application name is missing")?;
    let id = application
        .get("id")
        .and_then(serde_json::Value::as_str)
        .ok_or("package application id is missing")?;
    let version = application
        .get("version")
        .and_then(serde_json::Value::as_str)
        .ok_or("package application version is missing")?;
    if safe_application_id(name) != id
        || id.is_empty()
        || !version.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '+' | '_')
        })
        || version.is_empty()
        || matches!(version, "." | "..")
    {
        return Err("package application identity contains an unsafe path component".into());
    }
    let prefix = prefix.to_path_buf();
    let app_parent = prefix.join("share/guion/apps").join(id);
    let bin = prefix.join("bin");
    fs::create_dir_all(&app_parent)
        .map_err(|error| format!("cannot create application install directory: {error}"))?;
    fs::create_dir_all(&bin)
        .map_err(|error| format!("cannot create command install directory: {error}"))?;
    let executable = bin.join(id);
    if let Ok(existing) = fs::read_to_string(&executable) {
        if !existing.contains("# managed by guion") {
            return Err(format!(
                "refusing to replace existing command {}",
                executable.display()
            ));
        }
    }
    let destination = app_parent.join(version);
    if destination.exists() {
        let (existing_hash, _) = sha256_tree(&destination)?;
        let (source_hash, _) = sha256_tree(&package)?;
        if existing_hash != source_hash {
            return Err(format!(
                "application {id} version {version} is already installed with different contents"
            ));
        }
    } else {
        let stage = app_parent.join(format!(
            ".{version}.stage-{}-{}",
            std::process::id(),
            unique_nonce()
        ));
        copy_package_contents(&package, &stage)?;
        if let Err(error) = fs::rename(&stage, &destination) {
            let _ = fs::remove_dir_all(&stage);
            return Err(format!("cannot publish installed application: {error}"));
        }
    }
    let temporary_launcher = bin.join(format!(".{id}.guion-{}.tmp", std::process::id()));
    let launcher = format!(
        "#!/bin/sh\n# managed by guion\nset -eu\nHERE=$(CDPATH= cd -- \"$(dirname -- \"$0\")\" && pwd)\nexec \"$HERE/../share/guion/apps/{id}/{version}/run\" \"$@\"\n"
    );
    fs::write(&temporary_launcher, launcher)
        .map_err(|error| format!("cannot write app launcher: {error}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(&temporary_launcher)
            .map_err(|error| format!("cannot inspect app launcher: {error}"))?
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&temporary_launcher, permissions)
            .map_err(|error| format!("cannot mark app launcher executable: {error}"))?;
    }
    if let Err(error) = fs::rename(&temporary_launcher, &executable) {
        let _ = fs::remove_file(&temporary_launcher);
        return Err(format!("cannot publish app launcher: {error}"));
    }
    Ok(executable)
}

fn unique_nonce() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

fn copy_package_contents(source: &Path, destination: &Path) -> Result<(), String> {
    fs::create_dir(destination)
        .map_err(|error| format!("cannot create app staging directory: {error}"))?;
    let result = (|| {
        for entry in fs::read_dir(source)
            .map_err(|error| format!("cannot read package directory: {error}"))?
        {
            let entry = entry.map_err(|error| format!("cannot read package entry: {error}"))?;
            let kind = entry
                .file_type()
                .map_err(|error| format!("cannot inspect package entry: {error}"))?;
            let target = destination.join(entry.file_name());
            if kind.is_dir() {
                copy_package_contents(&entry.path(), &target)?;
            } else if kind.is_file() {
                fs::copy(entry.path(), &target)
                    .map_err(|error| format!("cannot copy package file: {error}"))?;
            } else {
                return Err(format!(
                    "package contains an unsupported filesystem entry: {}",
                    entry.path().display()
                ));
            }
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(destination);
    }
    result
}

fn compile_project(
    entry: &std::path::Path,
    project_root: &std::path::Path,
) -> Result<Artifact, guion_model::Diagnostic> {
    let package_roots = if project_manifest_path(project_root).is_file() {
        native_packages::load_locked_roots(project_root).map_err(|message| {
            guion_model::Diagnostic::new(
                "E_PACKAGE_LOCK",
                message,
                guion_model::Span {
                    source_id: 0,
                    start: 0,
                    end: 0,
                    line: 1,
                    column: 1,
                },
            )
        })?
    } else {
        std::collections::BTreeMap::new()
    };
    let package_roots = package_roots.into_iter().collect();
    compiler_driver::compile_project_with_roots(
        entry,
        project_root,
        compiler_driver::standard_library_root().as_deref(),
        &package_roots,
    )
}

fn compile_project_for_run(
    entry: &Path,
    project_root: &Path,
    no_cache: bool,
    file_roots: &[std::path::PathBuf],
    http_hosts: &[String],
    clock_override: bool,
) -> Result<
    (
        Artifact,
        &'static str,
        Option<compiler_driver::CompilerPhaseTimings>,
    ),
    guion_model::Diagnostic,
> {
    let package_roots = if project_manifest_path(project_root).is_file() {
        native_packages::load_locked_roots(project_root).map_err(|message| {
            guion_model::Diagnostic::new(
                "E_PACKAGE_LOCK",
                message,
                guion_model::Span {
                    source_id: 0,
                    start: 0,
                    end: 0,
                    line: 1,
                    column: 1,
                },
            )
        })?
    } else {
        std::collections::BTreeMap::new()
    }
    .into_iter()
    .collect::<std::collections::HashMap<_, _>>();
    let stdlib_root = compiler_driver::standard_library_root();
    let mut file_roots = file_roots
        .iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let mut http_hosts = http_hosts.to_vec();
    file_roots.sort();
    http_hosts.sort();
    let runtime_identity = serde_json::json!({
        "file_roots": file_roots,
        "http_hosts": http_hosts,
        "clock": clock_override,
    })
    .to_string();
    let compiler_identity = compiler_driver::cache_identity();
    let mut compiler_timings = None;
    let (artifact, cache_status) = source_run_cache::load_or_compile(
        source_run_cache::SourceRunCacheContext {
            entry,
            project_root,
            stdlib_root: stdlib_root.as_deref(),
            package_roots: &package_roots,
            compiler_identity: &compiler_identity,
            runtime_identity: &runtime_identity,
            no_cache,
        },
        || {
            compiler_driver::compile_project_with_roots_profiled(
                entry,
                project_root,
                stdlib_root.as_deref(),
                &package_roots,
            )
            .map(|(artifact, timings)| {
                compiler_timings = Some(timings);
                artifact
            })
        },
    )?;
    Ok((artifact, cache_status, compiler_timings))
}

fn compiler_phase_report(timings: &compiler_driver::CompilerPhaseTimings) -> serde_json::Value {
    serde_json::json!({
        "project_inputs_ns": timings.project_inputs_ns,
        "seed_validation_ns": timings.seed_validation_ns,
        "compiler_runtime_load_ns": timings.compiler_runtime_load_ns,
        "self_hosted_compile_including_import_graph_read_ns": timings.self_hosted_compile_ns,
        "emitted_artifact_decode_ns": timings.emitted_artifact_decode_ns,
        "source_graph_finalize_ns": timings.source_graph_finalize_ns,
        "compiler_native_compilations": timings.compiler_native_compilations,
        "compiler_native_compile_profile": timings.compiler_native_compile_profile,
    })
}

#[cfg(feature = "python-interop")]
fn python_doctor_report() -> String {
    dependencies::doctor_report().to_string()
}

#[cfg(not(feature = "python-interop"))]
fn python_doctor_report() -> String {
    serde_json::json!({
        "schema_version": 1,
        "kind": "guion_doctor_report",
        "python_interop": "disabled",
        "message": "Python interoperability is optional; rebuild with --features python-interop to enable it"
    })
    .to_string()
}

#[cfg(feature = "python-interop")]
fn python_doctor_status() -> String {
    dependencies::doctor_status()
}

#[cfg(not(feature = "python-interop"))]
fn python_doctor_status() -> String {
    "Python interoperability is disabled in this core build".into()
}

#[cfg(target_os = "macos")]
fn publish_package_stage(stage: &std::path::Path, output: &std::path::Path) -> std::io::Result<()> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};

    let stage = CString::new(stage.as_os_str().as_bytes()).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "package stage path contains NUL",
        )
    })?;
    let output = CString::new(output.as_os_str().as_bytes()).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "package output path contains NUL",
        )
    })?;
    // RENAME_EXCL makes destination validation and publication one kernel operation.
    let result = unsafe {
        libc::renameatx_np(
            libc::AT_FDCWD,
            stage.as_ptr(),
            libc::AT_FDCWD,
            output.as_ptr(),
            libc::RENAME_EXCL,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(target_os = "linux")]
fn publish_package_stage(stage: &std::path::Path, output: &std::path::Path) -> std::io::Result<()> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};

    let stage = CString::new(stage.as_os_str().as_bytes()).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "package stage path contains NUL",
        )
    })?;
    let output = CString::new(output.as_os_str().as_bytes()).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "package output path contains NUL",
        )
    })?;
    // RENAME_NOREPLACE makes destination validation and publication one kernel operation.
    let result = unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            stage.as_ptr(),
            libc::AT_FDCWD,
            output.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn publish_package_stage(
    _stage: &std::path::Path,
    _output: &std::path::Path,
) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "atomic no-replace package publication is currently supported only on macOS",
    ))
}

struct RustNoticeBundle {
    inventory_sha256: String,
    files_sha256: String,
    files_size_bytes: u64,
}

fn copy_rust_notices(
    distribution_root: &Path,
    package_root: &Path,
) -> Result<Option<RustNoticeBundle>, String> {
    let source = distribution_root.join("licenses");
    let inventory = source.join("RUST-DEPENDENCIES.md");
    let source_files = source.join("rust");
    if !inventory.exists() && !source_files.exists() {
        return Ok(None);
    }
    let inventory_is_regular = fs::symlink_metadata(&inventory)
        .map(|metadata| metadata.file_type().is_file())
        .unwrap_or(false);
    let source_files_are_directory = fs::symlink_metadata(&source_files)
        .map(|metadata| metadata.file_type().is_dir())
        .unwrap_or(false);
    if !inventory_is_regular || !source_files_are_directory {
        return Err("Guion distribution has an incomplete Rust dependency notice set".into());
    }
    let destination = package_root.join("licenses");
    fs::create_dir_all(&destination)
        .map_err(|error| format!("cannot create app license directory: {error}"))?;
    fs::copy(&inventory, destination.join("RUST-DEPENDENCIES.md"))
        .map_err(|error| format!("cannot copy Rust dependency inventory: {error}"))?;
    copy_notice_tree(&source_files, &destination.join("rust"))?;
    let (inventory_sha256, _) = sha256_path(&destination.join("RUST-DEPENDENCIES.md"))?;
    let (files_sha256, files_size_bytes) = sha256_tree(&destination.join("rust"))?;
    Ok(Some(RustNoticeBundle {
        inventory_sha256,
        files_sha256,
        files_size_bytes,
    }))
}

fn copy_notice_tree(source: &Path, destination: &Path) -> Result<(), String> {
    fs::create_dir_all(destination)
        .map_err(|error| format!("cannot create Rust notice directory: {error}"))?;
    for entry in fs::read_dir(source)
        .map_err(|error| format!("cannot inspect Rust notice directory: {error}"))?
    {
        let entry = entry.map_err(|error| format!("cannot inspect Rust notice entry: {error}"))?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let kind = entry
            .file_type()
            .map_err(|error| format!("cannot inspect Rust notice entry: {error}"))?;
        if kind.is_dir() {
            copy_notice_tree(&source_path, &destination_path)?;
        } else if kind.is_file() {
            fs::copy(&source_path, &destination_path)
                .map_err(|error| format!("cannot copy Rust notice file: {error}"))?;
        } else {
            return Err(format!(
                "unsupported file type in Rust notice set: {}",
                source_path.display()
            ));
        }
    }
    Ok(())
}

fn sha256_path(path: &std::path::Path) -> Result<(String, u64), String> {
    use std::io::Read;

    let mut file = fs::File::open(path).map_err(|error| {
        format!(
            "cannot read {} for package metadata: {error}",
            path.display()
        )
    })?;
    let mut digest = Sha256::new();
    let mut size_bytes = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| format!("cannot hash {}: {error}", path.display()))?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
        size_bytes = size_bytes
            .checked_add(read as u64)
            .ok_or_else(|| format!("package resource {} is too large to report", path.display()))?;
    }
    Ok((format!("{:x}", digest.finalize()), size_bytes))
}

fn sha256_tree(root: &Path) -> Result<(String, u64), String> {
    fn visit(root: &Path, path: &Path, digest: &mut Sha256, size: &mut u64) -> Result<(), String> {
        let relative = path.strip_prefix(root).unwrap_or(path);
        digest.update(relative.to_string_lossy().as_bytes());
        digest.update([0]);
        let metadata = fs::symlink_metadata(path)
            .map_err(|error| format!("cannot inspect package entry {}: {error}", path.display()))?;
        let kind = metadata.file_type();
        if kind.is_symlink() {
            digest.update(b"L");
            let target = fs::read_link(path)
                .map_err(|error| format!("cannot read package link {}: {error}", path.display()))?;
            digest.update(target.to_string_lossy().as_bytes());
        } else if kind.is_dir() {
            digest.update(b"D");
            let mut entries = fs::read_dir(path)
                .map_err(|error| {
                    format!("cannot scan package directory {}: {error}", path.display())
                })?
                .map(|entry| entry.map(|entry| entry.path()))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| format!("cannot scan package directory: {error}"))?;
            entries.sort();
            for entry in entries {
                visit(root, &entry, digest, size)?;
            }
        } else if kind.is_file() {
            digest.update(b"F");
            let bytes = fs::read(path)
                .map_err(|error| format!("cannot read package file {}: {error}", path.display()))?;
            digest.update(&bytes);
            *size = size
                .checked_add(bytes.len() as u64)
                .ok_or_else(|| "package directory is too large to report".to_owned())?;
        } else {
            return Err(format!(
                "unsupported special package entry: {}",
                path.display()
            ));
        }
        digest.update([0xff]);
        Ok(())
    }
    let mut digest = Sha256::new();
    let mut size = 0;
    visit(root, root, &mut digest, &mut size)?;
    Ok((format!("{:x}", digest.finalize()), size))
}

fn verify_package(directory: &Path) -> Result<(), String> {
    let root = fs::canonicalize(directory).map_err(|error| {
        format!(
            "cannot open package directory {}: {error}",
            directory.display()
        )
    })?;
    let manifest_path = root.join("PACKAGE.json");
    let metadata: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest_path).map_err(|error| {
            format!(
                "cannot read package report {}: {error}",
                manifest_path.display()
            )
        })?)
        .map_err(|error| {
            format!(
                "invalid package report {}: {error}",
                manifest_path.display()
            )
        })?;
    let package_schema = metadata["schema_version"].as_u64();
    if !matches!(package_schema, Some(1 | 2)) {
        return Err(
            "unsupported package report schema; rebuild with a compatible Guion CLI".into(),
        );
    }
    let expected_target = format!("{}-{}", env::consts::OS, env::consts::ARCH);
    if metadata["target"].as_str() != Some(&expected_target) {
        return Err(format!(
            "package target mismatch: this host is {expected_target}, package targets {}",
            metadata["target"].as_str().unwrap_or("an unknown target")
        ));
    }
    verify_package_file(
        &root,
        "app.gub",
        metadata["artifact_sha256"].as_str(),
        None,
        "artifact",
    )?;
    verify_package_file(
        &root,
        "guion",
        metadata["runtime_sha256"].as_str(),
        metadata["runtime_size_bytes"].as_u64(),
        "Guion runtime",
    )?;
    if let Some(notices) = metadata
        .get("rust_notices")
        .filter(|value| !value.is_null())
    {
        verify_package_file(
            &root,
            "licenses/RUST-DEPENDENCIES.md",
            notices["inventory_sha256"].as_str(),
            None,
            "Rust dependency inventory",
        )?;
        let rust_root = package_file(&root, "licenses/rust", "Rust dependency notices")?;
        if !rust_root.is_dir() {
            return Err(
                "package verification failed: Rust dependency notice directory is missing".into(),
            );
        }
        let (digest, size) = sha256_tree(&rust_root)?;
        if Some(digest.as_str()) != notices["files_sha256"].as_str()
            || Some(size) != notices["files_size_bytes"].as_u64()
        {
            return Err(
                "package verification failed: Rust dependency notice integrity check failed".into(),
            );
        }
    }
    let resources = metadata["resources"]
        .as_array()
        .ok_or("invalid package report: resources must be an array")?;
    for resource in resources {
        let path = resource["path"]
            .as_str()
            .ok_or("invalid package report: resource path is missing")?;
        let digest = resource["sha256"].as_str();
        let size = resource["size_bytes"].as_u64();
        verify_package_file(&root, path, digest, size, "resource")?;
    }
    if let Some(expected) = metadata["native_dependency_report_sha256"].as_str() {
        verify_package_file(
            &root,
            "NATIVE-DEPENDENCIES.json",
            Some(expected),
            None,
            "native dependency report",
        )?;
        let native_report: serde_json::Value = serde_json::from_slice(
            &fs::read(root.join("NATIVE-DEPENDENCIES.json"))
                .map_err(|error| format!("cannot read native dependency report: {error}"))?,
        )
        .map_err(|error| format!("invalid native dependency report: {error}"))?;
        if native_report["unresolved"]
            .as_array()
            .is_none_or(|items| !items.is_empty())
        {
            return Err(
                "package verification failed: native dependency report is incomplete".into(),
            );
        }
        let images = native_report["images"]
            .as_array()
            .ok_or("invalid native dependency report: images must be an array")?;
        for image in images {
            let path = image["path"]
                .as_str()
                .ok_or("invalid native dependency report: image path is missing")?;
            let owner = image["owner"]
                .as_object()
                .ok_or("invalid native dependency report: image owner is missing")?;
            let owner_kind = owner
                .get("kind")
                .and_then(serde_json::Value::as_str)
                .ok_or("invalid native dependency report: image owner kind is missing")?;
            if path.contains("site-packages") && owner_kind != "python_distribution" {
                return Err(format!(
                    "package verification failed: native Python image has no distribution owner: {path}"
                ));
            }
            verify_package_file(
                &root,
                path,
                image["sha256"].as_str(),
                image["size_bytes"].as_u64(),
                "native image",
            )?;
        }
        if let Some(cached_images) = native_report
            .get("cached_images")
            .and_then(serde_json::Value::as_array)
        {
            for image in cached_images {
                let path = image["path"]
                    .as_str()
                    .ok_or("invalid native dependency report: cached image path is missing")?;
                let owner = image["owner"]
                    .as_object()
                    .ok_or("invalid native dependency report: cached image owner is missing")?;
                if !path.starts_with(".uv-cache/")
                    || !path.contains("/archive-v")
                    || owner.get("kind").and_then(serde_json::Value::as_str)
                        != Some("python_distribution")
                    || owner.get("environment").and_then(serde_json::Value::as_str)
                        != Some("offline-wheel-cache")
                {
                    return Err(format!(
                        "package verification failed: cached native image provenance is invalid: {path}"
                    ));
                }
                let name = owner
                    .get("name")
                    .and_then(serde_json::Value::as_str)
                    .filter(|name| !name.is_empty())
                    .ok_or(
                        "invalid native dependency report: cached image owner name is missing",
                    )?;
                let version = owner
                    .get("version")
                    .and_then(serde_json::Value::as_str)
                    .filter(|version| !version.is_empty())
                    .ok_or(
                        "invalid native dependency report: cached image owner version is missing",
                    )?;
                let record = owner
                    .get("record")
                    .and_then(serde_json::Value::as_str)
                    .ok_or(
                        "invalid native dependency report: cached owner RECORD path is missing",
                    )?;
                if !record.starts_with(".uv-cache/") || !record.ends_with(".dist-info/RECORD") {
                    return Err(format!(
                        "package verification failed: cached native owner RECORD path is invalid for {name} {version}"
                    ));
                }
                if !package_file(&root, record, "cached Python distribution RECORD")?.is_file() {
                    return Err(format!(
                        "package verification failed: cached Python distribution RECORD is missing for {name} {version}"
                    ));
                }
                verify_package_file(
                    &root,
                    path,
                    image["sha256"].as_str(),
                    image["size_bytes"].as_u64(),
                    "cached native image",
                )?;
            }
        }
    }
    if !metadata["python_runtime"].is_null() {
        #[cfg(not(feature = "python-interop"))]
        {
            return Err(
                "verifying this package requires the optional `python-interop` feature".into(),
            );
        }
        #[cfg(feature = "python-interop")]
        {
            let python = &metadata["python_runtime"];
            verify_package_file(
                &root,
                "toolchains/uv",
                python["uv_sha256"].as_str(),
                None,
                "bundled uv executable",
            )?;
            verify_package_file(
                &root,
                ".guion/python-bridge/uv.lock",
                python["uv_lock_sha256"].as_str(),
                None,
                "Python dependency lock",
            )?;
            let python_root = package_file(&root, "python", "Python runtime")?;
            if !python_root.is_dir() {
                return Err(
                    "package verification failed: Python runtime directory is missing".into(),
                );
            }
            let (digest, size) = dependencies::sha256_tree(&python_root)?;
            if Some(digest.as_str()) != python["python_runtime_sha256"].as_str()
                || Some(size) != python["python_runtime_size_bytes"].as_u64()
            {
                return Err(
                    "package verification failed: Python runtime integrity check failed".into(),
                );
            }
            if package_schema.unwrap_or(1) >= 2 {
                let cache_root =
                    package_file(&root, ".uv-cache", "offline Python dependency cache")?;
                if !cache_root.is_dir() {
                    return Err(
                        "package verification failed: offline Python dependency cache is missing"
                            .into(),
                    );
                }
                let (cache_digest, cache_size) =
                    dependencies::sha256_python_cache_payload(&cache_root)?;
                if Some(cache_digest.as_str()) != python["python_cache_sha256"].as_str()
                    || Some(cache_size) != python["python_cache_size_bytes"].as_u64()
                {
                    return Err(
                        "package verification failed: offline Python cache integrity check failed"
                            .into(),
                    );
                }
            }
            verify_package_file(
                &root,
                "PYTHON-DEPENDENCIES.json",
                python["dependency_report_sha256"].as_str(),
                None,
                "Python dependency report",
            )?;
            let dependency_report: serde_json::Value = serde_json::from_slice(
                &fs::read(root.join("PYTHON-DEPENDENCIES.json"))
                    .map_err(|error| format!("cannot read Python dependency report: {error}"))?,
            )
            .map_err(|error| format!("invalid Python dependency report: {error}"))?;
            let packages = dependency_report["packages"]
                .as_array()
                .ok_or("invalid Python dependency report: packages must be an array")?;
            let native_report: serde_json::Value = serde_json::from_slice(
                &fs::read(root.join("NATIVE-DEPENDENCIES.json"))
                    .map_err(|error| format!("cannot read native dependency report: {error}"))?,
            )
            .map_err(|error| format!("invalid native dependency report: {error}"))?;
            for image in native_report["images"]
                .as_array()
                .ok_or("invalid native dependency report: images must be an array")?
            {
                let path = image["path"].as_str().unwrap_or_default();
                if !path.contains("site-packages") {
                    continue;
                }
                let owner = &image["owner"];
                let name = owner["name"]
                    .as_str()
                    .ok_or("invalid native dependency report: Python owner name is missing")?;
                let version = owner["version"]
                    .as_str()
                    .ok_or("invalid native dependency report: Python owner version is missing")?;
                let environment = owner["environment"].as_str().ok_or(
                    "invalid native dependency report: Python owner environment is missing",
                )?;
                let package = packages.iter().find(|package| {
                package["name"].as_str() == Some(name)
                    && package["version"].as_str() == Some(version)
                    && package["environment"].as_str() == Some(environment)
            }).ok_or_else(|| format!(
                "package verification failed: native image owner {name} {version} is absent from Python dependency report"
            ))?;
                if owner["license_status"] != package["license_status"]
                    || owner["license_files"] != package["license_files"]
                {
                    return Err(format!(
                    "package verification failed: native image owner license metadata disagrees with Python dependency report for {name} {version}"
                ));
                }
                let record = owner["record"].as_str().ok_or(
                    "invalid native dependency report: Python owner RECORD path is missing",
                )?;
                if !package_file(&root, record, "Python distribution RECORD")?.is_file() {
                    return Err(format!(
                    "package verification failed: Python distribution RECORD is not a regular file: {record}"
                ));
                }
            }
            for package in packages {
                let files = package["license_files"]
                    .as_array()
                    .ok_or("invalid Python dependency report: license_files must be an array")?;
                for file in files {
                    verify_package_file(
                        &root,
                        file["path"]
                            .as_str()
                            .ok_or("invalid Python dependency report: license path is missing")?,
                        file["sha256"].as_str(),
                        file["size_bytes"].as_u64(),
                        "Python dependency license",
                    )?;
                }
            }
            let runtime_components = dependency_report["runtime_components"]
                .as_array()
                .ok_or("invalid Python dependency report: runtime_components must be an array")?;
            for component in runtime_components {
                let files = component["license_files"].as_array().ok_or(
                    "invalid Python dependency report: runtime license_files must be an array",
                )?;
                for file in files {
                    verify_package_file(
                        &root,
                        file["path"].as_str().ok_or(
                            "invalid Python dependency report: runtime license path is missing",
                        )?,
                        file["sha256"].as_str(),
                        file["size_bytes"].as_u64(),
                        "Python runtime license",
                    )?;
                }
            }
        }
    }
    Ok(())
}

fn package_file(root: &Path, relative: &str, label: &str) -> Result<std::path::PathBuf, String> {
    let relative_path = Path::new(relative);
    if relative_path.as_os_str().is_empty()
        || relative_path.is_absolute()
        || relative_path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(format!("invalid {label} path in package report"));
    }
    let path = root.join(relative_path);
    let canonical = fs::canonicalize(&path).map_err(|error| {
        format!("package verification failed: {label} is missing or unreadable: {error}")
    })?;
    if !canonical.starts_with(root) {
        return Err(format!(
            "package verification failed: {label} escapes the package directory"
        ));
    }
    Ok(canonical)
}

fn verify_package_file(
    root: &Path,
    relative: &str,
    expected_digest: Option<&str>,
    expected_size: Option<u64>,
    label: &str,
) -> Result<(), String> {
    let path = package_file(root, relative, label)?;
    if !path.is_file() {
        return Err(format!(
            "package verification failed: {label} is not a regular file"
        ));
    }
    let (actual_digest, actual_size) = sha256_path(&path)?;
    if Some(actual_digest.as_str()) != expected_digest
        || expected_size.is_some_and(|size| size != actual_size)
    {
        return Err(format!(
            "package verification failed: {label} integrity check failed"
        ));
    }
    Ok(())
}

fn package_resources(
    manifest: &toml::Value,
    root: &std::path::Path,
) -> Result<Vec<(std::path::PathBuf, std::path::PathBuf)>, String> {
    use std::collections::BTreeSet;
    use std::path::{Component, Path};

    let Some(package) = manifest.get("package") else {
        return Ok(Vec::new());
    };
    let package = package
        .as_table()
        .ok_or("guion.toml [package] must be a table")?;
    if let Some(key) = package.keys().find(|key| key.as_str() != "resources") {
        return Err(format!("unsupported guion.toml [package] field `{key}`"));
    }
    let Some(resources) = package.get("resources") else {
        return Ok(Vec::new());
    };
    let resources = resources
        .as_array()
        .ok_or("guion.toml [package].resources must be an array of project-relative file paths")?;
    let mut seen = BTreeSet::new();
    let mut result = Vec::with_capacity(resources.len());
    let reserved = [
        "app.gub",
        "guion.toml",
        "guion.lock",
        "guion",
        "app.vrb",
        "veritas.toml",
        "veritas.lock",
        "veritas",
        "run",
        "PACKAGE.txt",
        "PACKAGE.json",
        "PYTHON-DEPENDENCIES.json",
        "NATIVE-DEPENDENCIES.json",
        "python",
        "licenses",
        ".uv-cache",
        ".guion",
        ".veritas",
        "toolchains",
    ];
    for value in resources {
        let raw = value
            .as_str()
            .ok_or("guion.toml [package].resources entries must be strings")?;
        let relative = Path::new(raw);
        if relative.as_os_str().is_empty()
            || relative.is_absolute()
            || relative
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
        {
            return Err(format!(
                "package resource `{raw}` must be a relative path without `.` or `..` components"
            ));
        }
        if reserved.iter().any(|name| {
            let reserved = Path::new(name);
            relative.starts_with(reserved) || reserved.starts_with(relative)
        }) {
            return Err(format!(
                "package resource `{raw}` uses a path reserved by the package runtime"
            ));
        }
        if !seen.insert(relative.to_path_buf()) {
            return Err(format!("duplicate package resource `{raw}`"));
        }
        let source = root
            .join(relative)
            .canonicalize()
            .map_err(|error| format!("package resource `{raw}` cannot be resolved: {error}"))?;
        if !source.starts_with(root) || !source.is_file() {
            return Err(format!(
                "package resource `{raw}` must be a file inside the project root"
            ));
        }
        result.push((source, relative.to_path_buf()));
    }
    Ok(result)
}

fn build_metadata(
    policy: HostPolicy,
    artifact: &Artifact,
    project_root: &std::path::Path,
) -> Result<BuildMetadata, String> {
    let source_sha256 = artifact
        .compiled_source_sha256
        .clone()
        .ok_or("compiler did not attach a source digest to this build")?;
    let dependency_lock_sha256 = dependency_lock_digest(project_root)?;
    Ok(BuildMetadata {
        metadata_schema_version: 1,
        compiler_version: env!("CARGO_PKG_VERSION").to_owned(),
        target: format!("{}-{}", env::consts::OS, env::consts::ARCH),
        source_sha256: Some(source_sha256),
        dependency_lock_sha256,
        resource_limits: ResourceLimits {
            instruction_limit: policy.instruction_limit,
            allocation_limit_bytes: policy.allocation_limit_bytes,
            call_depth_limit: policy.call_depth_limit,
            output_limit_bytes: policy.output_limit_bytes,
            max_integer_digits: policy.max_integer_digits,
            max_text_bytes: policy.max_text_bytes,
            max_collection_items: policy.max_collection_items,
            max_parallelism: policy.max_parallelism,
            max_file_bytes: policy.max_file_bytes,
        },
    })
}

fn dependency_lock_digest(root: &std::path::Path) -> Result<Option<String>, String> {
    let locks = [
        ("project lock", project_lock_path(root)),
        (
            "Python bridge lock",
            project_bridge_root(root).join("uv.lock"),
        ),
    ];
    let mut digest = Sha256::new();
    let mut found = false;
    for (identity, path) in locks {
        if path.is_file() {
            let bytes = fs::read(&path)
                .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
            digest.update(identity.as_bytes());
            digest.update([0]);
            digest.update((bytes.len() as u64).to_be_bytes());
            digest.update(bytes);
            found = true;
        }
    }
    Ok(found.then(|| format!("{:x}", digest.finalize())))
}

fn display_type(ty: &guion_model::Type) -> String {
    use guion_model::Type;
    match ty {
        Type::Int => "Int".into(),
        Type::Float64 => "Float64".into(),
        Type::Record(name) => name.clone(),
        Type::Bool => "Bool".into(),
        Type::Text => "Text".into(),
        Type::Bytes => "Bytes".into(),
        Type::Unit => "Unit".into(),
        Type::TaskError => "TaskError".into(),
        Type::Json => "Json".into(),
        Type::JsonError => "JsonError".into(),
        Type::FileError => "FileError".into(),
        Type::HttpError => "HttpError".into(),
        Type::NativeResource(name) => name.clone(),
        Type::List(item) => format!("List[{}]", display_type(item)),
        Type::Map(item) => format!("Map[Text, {}]", display_type(item)),
        Type::Option(item) => format!("Option[{}]", display_type(item)),
        Type::Result(ok, err) => format!("Result[{}, {}]", display_type(ok), display_type(err)),
    }
}

fn inspection_report(
    source: &str,
    artifact: &Artifact,
    project_root: &Path,
    artifact_without_project: bool,
) -> serde_json::Value {
    let mut direct_effects = vec![BTreeSet::<String>::new(); artifact.functions.len()];
    let mut calls = vec![BTreeSet::<usize>::new(); artifact.functions.len()];
    let mut python_calls = vec![BTreeSet::<usize>::new(); artifact.functions.len()];
    for (function_index, function) in artifact.functions.iter().enumerate() {
        for instruction in &function.code {
            match &instruction.op {
                Op::Call(target, _) => {
                    calls[function_index].insert(*target);
                }
                Op::ParallelMap(target) => {
                    direct_effects[function_index].insert("concurrency.parallel".into());
                    calls[function_index].insert(*target);
                }
                Op::Print => {
                    direct_effects[function_index].insert("output.write".into());
                }
                Op::PrintErr => {
                    direct_effects[function_index].insert("output.stderr".into());
                }
                Op::ReadText | Op::ReadBytes => {
                    direct_effects[function_index].insert("filesystem.read".into());
                }
                Op::WriteText | Op::WriteTextAtomic | Op::WriteBytes => {
                    direct_effects[function_index].insert("filesystem.write".into());
                }
                Op::Args | Op::ArgsFrom => {
                    direct_effects[function_index].insert("process.arguments".into());
                }
                Op::Environment => {
                    direct_effects[function_index].insert("process.environment".into());
                }
                Op::UnixTimeMs | Op::TimeTodayLocal => {
                    direct_effects[function_index].insert("clock.read".into());
                }
                Op::HttpGet
                | Op::HttpRequest
                | Op::HttpRequestWithTimeout
                | Op::HttpRequestBytes => {
                    direct_effects[function_index].insert("network.http".into());
                }
                Op::PythonCall(target, _) | Op::PythonCallKw(target, _, _) => {
                    direct_effects[function_index].insert("python.opaque".into());
                    python_calls[function_index].insert(*target);
                }
                _ => {}
            }
        }
    }
    let mut effects = direct_effects.clone();
    let mut changed = true;
    while changed {
        changed = false;
        for index in 0..artifact.functions.len() {
            let mut joined = effects[index].clone();
            for target in &calls[index] {
                if let Some(callee_effects) = effects.get(*target) {
                    joined.extend(callee_effects.iter().cloned());
                }
            }
            if joined != effects[index] {
                effects[index] = joined;
                changed = true;
            }
        }
    }
    let mut reachable = BTreeSet::new();
    let mut pending = artifact
        .functions
        .iter()
        .position(|function| function.name == artifact.entry)
        .into_iter()
        .collect::<Vec<_>>();
    while let Some(index) = pending.pop() {
        if !reachable.insert(index) {
            continue;
        }
        if let Some(targets) = calls.get(index) {
            pending.extend(targets.iter().copied());
        }
    }
    let functions = artifact
        .functions
        .iter()
        .enumerate()
        .map(|(index, function)| {
            let names = calls[index]
                .iter()
                .filter_map(|target| artifact.functions.get(*target))
                .map(|function| function.name.clone())
                .collect::<Vec<_>>();
            (
                function.name.clone(),
                serde_json::json!({
                "direct_effects": direct_effects[index].iter().collect::<Vec<_>>(),
                "effects": effects[index].iter().collect::<Vec<_>>(),
                "calls": names,
                "entry_reachable": reachable.contains(&index),
                }),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    let used_python = python_calls
        .iter()
        .flatten()
        .copied()
        .collect::<BTreeSet<_>>();
    let entry_network_grant_required = artifact
        .functions
        .iter()
        .position(|function| function.name == artifact.entry)
        .and_then(|index| effects.get(index))
        .is_some_and(|effects| effects.contains("network.http"));
    let entry_clock_grant_required = artifact
        .functions
        .iter()
        .position(|function| function.name == artifact.entry)
        .and_then(|index| effects.get(index))
        .is_some_and(|effects| effects.contains("clock.read"));
    let entry_index = artifact
        .functions
        .iter()
        .position(|function| function.name == artifact.entry);
    let required_effects = entry_index
        .and_then(|index| effects.get(index))
        .cloned()
        .unwrap_or_default();
    let manifest_path = project_manifest_path(project_root);
    let (manifest_grants, manifest_provenance) = if manifest_path.is_file() {
        match runtime_policy(project_root) {
            Ok(policy) => {
                let mut hosts = policy
                    .allowed_http_hosts
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>();
                hosts.sort();
                (
                    serde_json::json!({
                        "provenance": "guion.toml",
                        "network_hosts": hosts,
                        "clock_enabled": policy.allow_clock,
                    }),
                    "guion.toml",
                )
            }
            Err(error) => (
                serde_json::json!({
                    "provenance": "invalid_manifest",
                    "network_hosts": null,
                    "clock_enabled": null,
                    "diagnostic": error,
                }),
                "invalid_manifest",
            ),
        }
    } else if artifact_without_project {
        (
            serde_json::json!({
                "provenance": "artifact_without_project_manifest",
                "network_hosts": null,
                "clock_enabled": null,
            }),
            "artifact_without_project_manifest",
        )
    } else {
        (
            serde_json::json!({
                "provenance": "no_project_manifest",
                "network_hosts": null,
                "clock_enabled": null,
            }),
            "no_project_manifest",
        )
    };
    let network_assessment = if !entry_network_grant_required {
        "not_required"
    } else if manifest_provenance == "invalid_manifest" {
        "invalid_manifest"
    } else if manifest_provenance != "guion.toml" {
        "unknown_external_runner"
    } else if manifest_grants["network_hosts"]
        .as_array()
        .is_some_and(Vec::is_empty)
    {
        "missing"
    } else {
        "present_but_target_dynamic"
    };
    let clock_assessment = if !entry_clock_grant_required {
        "not_required"
    } else if manifest_provenance == "invalid_manifest" {
        "invalid_manifest"
    } else if manifest_provenance != "guion.toml" {
        "unknown_external_runner"
    } else if manifest_grants["clock_enabled"] == true {
        "present"
    } else {
        "missing"
    };
    let filesystem_assessment = |effect: &str| {
        if required_effects.contains(effect) {
            "unknown_external_runner"
        } else {
            "not_required"
        }
    };
    serde_json::json!({
        "schema_version": 1,
        "kind": "guion_inspection_report",
        "source": source,
        "artifact_format": artifact.format_version,
        "entry": artifact.entry,
        "capability_claim": "static_bytecode_reachability_not_sandbox",
        "analysis_scope": "reachable direct calls and parallel_map worker functions; host effects are categorized from bytecode operations",
        "required_effects": required_effects.iter().collect::<Vec<_>>(),
        "manifest_grants": manifest_grants,
        "grant_assessment": {
            "network_http": network_assessment,
            "clock_read": clock_assessment,
            "filesystem_read": filesystem_assessment("filesystem.read"),
            "filesystem_write": filesystem_assessment("filesystem.write"),
            "caller_cli_grants": "not_supplied_to_inspect",
        },
        "functions": functions,
        "python_calls": artifact.externals.iter().enumerate().map(|(index, external)| serde_json::json!({
            "name": external.name,
            "module": external.module,
            "callable": external.callable,
            "effect": "opaque_external",
            "reachable_from_entry": reachable.iter().any(|function| python_calls[*function].contains(&index)),
            "declaration_used": used_python.contains(&index),
            "declared_types_are_not_effect_proofs": true,
        })).collect::<Vec<_>>(),
        "network_grant": {
            "required_for_entry": entry_network_grant_required,
            "manifest_key": "[network].hosts",
            "run_option": "guion run --allow-http-host HOST",
            "grant_model": "exact hostname; network access is denied when no grant is present",
            "runtime_supplied_urls_require_an_explicit_hostname_grant": true,
        },
        "clock_grant": {
            "required_for_entry": entry_clock_grant_required,
            "manifest_key": "[clock].enabled",
            "run_option": "guion run --allow-clock",
            "grant_model": "wall-clock reads are denied unless explicitly enabled",
            "local_date_uses_host_process_time_zone": true,
        },
    })
}

fn api_report(source: &str, project_root: &Path, artifact: &Artifact) -> serde_json::Value {
    let package_roots = native_packages::load_locked_roots(project_root).unwrap_or_default();
    let standard_root = compiler_driver::standard_library_root();
    let source_texts = artifact
        .sources
        .iter()
        .filter_map(|identity| {
            let source_path = if let Some(relative) = identity.strip_prefix("std:") {
                standard_root.as_ref()?.join(relative)
            } else if let Some(package_path) = identity.strip_prefix("pkg:") {
                let (package, relative) = package_path.split_once('/')?;
                package_roots.get(package)?.join(relative)
            } else {
                project_root.join(identity)
            };
            let source_path = source_module_path(&source_path);
            Some((identity.clone(), fs::read_to_string(source_path).ok()?))
        })
        .collect::<Vec<_>>();
    let declarations = source_texts
        .iter()
        .flat_map(|(identity, text)| {
            function_declarations(text)
                .into_iter()
                .map(move |(name, parameters, public)| (identity.clone(), name, parameters, public))
        })
        .collect::<Vec<_>>();
    let inspected = inspection_report(source, artifact, project_root, false);
    let functions = artifact.functions.iter().map(|function| {
        let (module_id, base_name) = if let Some(rest) = function.name.strip_prefix("@module/") {
            let (module_id, name) = rest.split_once('/').unwrap_or(("", rest));
            (module_id.parse::<usize>().ok(), name)
        } else { (None, function.name.as_str()) };
        let selected = module_id.and_then(|index| artifact.sources.get(index)).and_then(|identity| {
            declarations.iter().find(|(source, name, _, _)| source == identity && name == base_name)
        }).or_else(|| declarations.iter().find(|(_, name, _, public)| name == base_name && (*public || module_id.is_none())));
        let parameter_names = selected.map(|(_, _, names, _)| names.clone()).unwrap_or_default();
        let parameters = function.parameters.iter().enumerate().map(|(index, ty)| {
            serde_json::json!({
                "name": parameter_names.get(index).cloned().unwrap_or_else(|| format!("arg{}", index + 1)),
                "type": display_type(ty),
            })
        }).collect::<Vec<_>>();
        let effects = inspected["functions"].get(&function.name).and_then(|entry| entry["effects"].as_array()).cloned().unwrap_or_default();
        serde_json::json!({
            "name": function.name,
            "source": selected.map(|(source, _, _, _)| source.as_str()),
            "parameters": parameters,
            "result": display_type(&function.result),
            "effects": effects,
            "profile_requirements": if artifact.externals.is_empty() { Vec::<String>::new() } else { vec!["python-interop".to_owned()] },
        })
    }).collect::<Vec<_>>();
    let required_stdlib = artifact
        .sources
        .iter()
        .filter_map(|identity| {
            identity
                .strip_prefix("std:")
                .map(|module| module.trim_end_matches(".gu").to_owned())
        })
        .collect::<BTreeSet<_>>();
    let available_stdlib = standard_root
        .as_deref()
        .map(list_stdlib_modules)
        .unwrap_or_default();
    let packages = artifact
        .sources
        .iter()
        .filter_map(|identity| {
            identity
                .strip_prefix("pkg:")
                .and_then(|name| name.split_once('/').map(|(package, _)| package.to_owned()))
        })
        .collect::<BTreeSet<_>>();
    serde_json::json!({
        "schema_version": 2,
        "kind": "guion_api_report",
        "source": source,
        "functions": functions,
        "python_functions": artifact.externals.iter().map(|external| serde_json::json!({
            "name": external.name,
            "module": external.module,
            "callable": external.callable,
            "parameters": external.parameter_names.iter().zip(&external.parameters).map(|(name, ty)| serde_json::json!({"name": name, "type": display_type(ty)})).collect::<Vec<_>>(),
            "result": display_type(&external.result),
            "effect": "opaque_python_call",
            "profile_requirement": "python-interop",
        })).collect::<Vec<_>>(),
        "records": artifact.records.iter().map(|record| serde_json::json!({
            "name": record.name,
            "fields": record.fields.iter().map(|field| serde_json::json!({"name": field.name, "type": display_type(&field.ty)})).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "standard_library": {
            "profile": "core",
            "available_modules": available_stdlib,
            "required_modules": required_stdlib,
        },
        "native_packages": packages,
        "optional_profiles": if artifact.externals.is_empty() { Vec::<String>::new() } else { vec!["python-interop".to_owned()] },
        "capabilities": inspected["network_grant"],
    })
}

fn function_declarations(source: &str) -> Vec<(String, Vec<String>, bool)> {
    let mut declarations = Vec::new();
    for line in source.lines() {
        let trimmed = line.trim_start();
        let (signature, public) = if let Some(signature) = trimmed.strip_prefix("pub fn ") {
            (signature, true)
        } else if let Some(signature) = trimmed.strip_prefix("fn ") {
            (signature, false)
        } else {
            continue;
        };
        let Some(open) = signature.find('(') else {
            continue;
        };
        let name = signature[..open].trim();
        let Some(close) = signature[open + 1..].find(')') else {
            continue;
        };
        let parameter_text = &signature[open + 1..open + 1 + close];
        let parameters = split_signature_parameters(parameter_text)
            .into_iter()
            .filter_map(|parameter| {
                parameter
                    .split_once(':')
                    .map(|(name, _)| name.trim().to_owned())
            })
            .collect();
        declarations.push((name.to_owned(), parameters, public));
    }
    declarations
}

fn split_signature_parameters(parameters: &str) -> Vec<&str> {
    let mut result = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (index, character) in parameters.char_indices() {
        match character {
            '[' => depth += 1,
            ']' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                result.push(parameters[start..index].trim());
                start = index + 1;
            }
            _ => {}
        }
    }
    if !parameters[start..].trim().is_empty() {
        result.push(parameters[start..].trim());
    }
    result
}

fn list_stdlib_modules(root: &Path) -> Vec<String> {
    fn visit(root: &Path, current: &Path, output: &mut Vec<String>) {
        let Ok(entries) = fs::read_dir(current) else {
            return;
        };
        let mut entries = entries.filter_map(Result::ok).collect::<Vec<_>>();
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                visit(root, &entry.path(), output);
            } else if kind.is_file()
                && entry
                    .path()
                    .extension()
                    .is_some_and(|extension| extension == "vr")
            {
                if let Ok(relative) = entry.path().strip_prefix(root) {
                    output.push(
                        relative
                            .with_extension("")
                            .to_string_lossy()
                            .replace('\\', "/"),
                    );
                }
            }
        }
    }
    let mut modules = Vec::new();
    visit(root, root, &mut modules);
    modules
}

fn print_api_report_human(report: &serde_json::Value) {
    println!("API for {}", report["source"].as_str().unwrap_or("source"));
    if let Some(functions) = report["functions"].as_array() {
        for function in functions {
            let parameters = function["parameters"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|parameter| {
                    format!(
                        "{}: {}",
                        parameter["name"].as_str().unwrap_or("arg"),
                        parameter["type"].as_str().unwrap_or("?")
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            let effects = function["effects"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(serde_json::Value::as_str)
                .collect::<Vec<_>>();
            println!(
                "fn {}({parameters}) -> {}{}",
                function["name"].as_str().unwrap_or("?"),
                function["result"].as_str().unwrap_or("?"),
                if effects.is_empty() {
                    String::new()
                } else {
                    format!("  effects: {}", effects.join(", "))
                }
            );
        }
    }
    println!("Standard library profile: core");
    for module in report["standard_library"]["available_modules"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
    {
        println!("  std:{module}");
    }
    if let Some(requirements) = report["optional_profiles"].as_array() {
        for requirement in requirements {
            println!(
                "Optional profile required: {}",
                requirement.as_str().unwrap_or("unknown")
            );
        }
    }
}

fn format_source(source: &str) -> String {
    let normalized = source.replace("\r\n", "\n").replace('\r', "\n");
    let mut lines = normalized
        .lines()
        .map(format_source_line)
        .collect::<Vec<_>>();
    while lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    if lines.is_empty() {
        String::new()
    } else {
        format!("{}\n", lines.join("\n"))
    }
}

fn format_source_line(line: &str) -> String {
    let indentation_len = line
        .bytes()
        .take_while(|byte| matches!(byte, b' ' | b'\t'))
        .count();
    let indentation = &line[..indentation_len];
    let body = &line[indentation_len..];
    if body.is_empty() {
        return String::new();
    }
    let characters = body.chars().collect::<Vec<_>>();
    let mut tokens: Vec<(String, u8)> = Vec::new();
    let mut comment = None;
    let mut cursor = 0;
    while cursor < characters.len() {
        let character = characters[cursor];
        if character.is_whitespace() {
            cursor += 1;
            continue;
        }
        if character == '#' {
            comment = Some(characters[cursor..].iter().collect::<String>());
            break;
        }
        if character == '"' {
            let start = cursor;
            cursor += 1;
            let mut escaped = false;
            while cursor < characters.len() {
                let current = characters[cursor];
                cursor += 1;
                if escaped {
                    escaped = false;
                } else if current == '\\' {
                    escaped = true;
                } else if current == '"' {
                    break;
                }
            }
            tokens.push((characters[start..cursor].iter().collect(), 2));
            continue;
        }
        if character.is_ascii_alphabetic() || character == '_' {
            let start = cursor;
            cursor += 1;
            while cursor < characters.len()
                && (characters[cursor].is_ascii_alphanumeric() || characters[cursor] == '_')
            {
                cursor += 1;
            }
            let text = characters[start..cursor].iter().collect::<String>();
            let kind = if matches!(text.as_str(), "and" | "or" | "not") {
                3
            } else {
                1
            };
            tokens.push((text, kind));
            continue;
        }
        if character.is_ascii_digit() {
            let start = cursor;
            cursor += 1;
            while cursor < characters.len() && characters[cursor].is_ascii_digit() {
                cursor += 1;
            }
            if cursor < characters.len() && characters[cursor] == '.' {
                cursor += 1;
                while cursor < characters.len() && characters[cursor].is_ascii_digit() {
                    cursor += 1;
                }
            }
            if cursor < characters.len() && matches!(characters[cursor], 'e' | 'E') {
                cursor += 1;
                if cursor < characters.len() && matches!(characters[cursor], '+' | '-') {
                    cursor += 1;
                }
                while cursor < characters.len() && characters[cursor].is_ascii_digit() {
                    cursor += 1;
                }
            }
            tokens.push((characters[start..cursor].iter().collect(), 1));
            continue;
        }
        let two = if cursor + 1 < characters.len() {
            [character, characters[cursor + 1]]
                .iter()
                .collect::<String>()
        } else {
            String::new()
        };
        if matches!(two.as_str(), "->" | "==" | "!=" | "<=" | ">=" | "//") {
            tokens.push((two, 3));
            cursor += 2;
        } else {
            tokens.push((
                character.to_string(),
                if "+-*/%=<>".contains(character) { 3 } else { 4 },
            ));
            cursor += 1;
        }
    }

    let mut formatted = String::from(indentation);
    let mut previous: Option<(String, u8)> = None;
    for (text, kind) in &tokens {
        if let Some((previous_text, previous_kind)) = previous.as_ref() {
            let needs_space = match text.as_str() {
                "," | ")" | "]" | "}" | "." | ":" => false,
                "(" => previous_text == "fn" || previous_text == "record",
                "[" => false,
                _ if matches!(text.as_str(), "and" | "or") => true,
                _ if *kind == 3 || *previous_kind == 3 => true,
                _ if previous_text == "," || previous_text == ":" => true,
                _ if previous_text == "(" || previous_text == "[" || previous_text == "." => false,
                "{" => true,
                _ if previous_text == "{" => true,
                _ => matches!((*previous_kind, *kind), (1, 1) | (1, 2) | (2, 1) | (2, 2)),
            };
            if needs_space && !formatted.chars().last().is_some_and(char::is_whitespace) {
                formatted.push(' ');
            }
        }
        formatted.push_str(text);
        previous = Some((text.clone(), *kind));
    }
    if let Some(comment) = comment {
        if !formatted.is_empty() && !formatted.chars().last().is_some_and(char::is_whitespace) {
            formatted.push(' ');
        }
        formatted.push_str(&comment);
    }
    formatted
}

fn lint_source(source: &str, source_path: &Path, project_root: &Path) -> Vec<serde_json::Value> {
    let identifiers = source_identifiers(source);
    let mut diagnostics = Vec::new();
    let lines = source.lines().collect::<Vec<_>>();
    for (line_index, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        let leading = line.len() - trimmed.len();
        if let Some(declaration) = trimmed.strip_prefix("let ") {
            if let Some(name) = declaration
                .split(|character: char| !character.is_ascii_alphanumeric() && character != '_')
                .next()
            {
                if !name.is_empty()
                    && identifiers
                        .iter()
                        .filter(|(identifier, _)| identifier == name)
                        .count()
                        == 1
                {
                    diagnostics.push(lint_diagnostic(
                        "W_UNUSED_LOCAL",
                        format!("local `{name}` is never used"),
                        line_index + 1,
                        leading + 5,
                    ));
                }
            }
        }
        if let Some(import_path) = import_path(trimmed) {
            if let Some(imported_source) =
                lint_import_source(import_path, source_path, project_root)
            {
                let exports = imported_source
                    .lines()
                    .filter_map(|import_line| {
                        let declaration = import_line
                            .trim_start()
                            .strip_prefix("pub fn ")
                            .or_else(|| import_line.trim_start().strip_prefix("pub record "))?;
                        let name = declaration
                            .split(|character: char| {
                                !character.is_ascii_alphanumeric() && character != '_'
                            })
                            .next()?;
                        (!name.is_empty()).then_some(name.to_owned())
                    })
                    .collect::<Vec<_>>();
                if !exports.is_empty()
                    && exports
                        .iter()
                        .all(|name| !identifiers.iter().any(|(identifier, _)| identifier == name))
                {
                    diagnostics.push(lint_diagnostic(
                        "W_UNUSED_IMPORT",
                        format!("import `{import_path}` provides no symbols used by this module"),
                        line_index + 1,
                        leading + 1,
                    ));
                }
            }
        }
    }
    let mut terminating_indent: Option<usize> = None;
    for (line_index, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some(terminating) = terminating_indent {
            if indent < terminating {
                terminating_indent = None;
            } else if indent == terminating
                && !trimmed.starts_with("else:")
                && !trimmed.starts_with("elif ")
            {
                diagnostics.push(lint_diagnostic(
                    "W_UNREACHABLE_STATEMENT",
                    "statement follows an unconditional exit".into(),
                    line_index + 1,
                    indent + 1,
                ));
                continue;
            } else if indent == terminating {
                terminating_indent = None;
            }
        }
        if trimmed.starts_with("return ")
            || trimmed == "return"
            || trimmed == "break"
            || trimmed == "continue"
        {
            terminating_indent = Some(indent);
        }
    }
    diagnostics.sort_by_key(|diagnostic| {
        (
            diagnostic["line"].as_u64().unwrap_or(0),
            diagnostic["column"].as_u64().unwrap_or(0),
            diagnostic["code"].as_str().unwrap_or("").to_owned(),
        )
    });
    diagnostics
}

fn lint_diagnostic(code: &str, message: String, line: usize, column: usize) -> serde_json::Value {
    serde_json::json!({"code": code, "message": message, "line": line, "column": column, "severity": "warning"})
}

fn source_identifiers(source: &str) -> Vec<(String, usize)> {
    let mut identifiers = Vec::new();
    for (line_index, line) in source.lines().enumerate() {
        let characters = line.chars().collect::<Vec<_>>();
        let mut cursor = 0;
        while cursor < characters.len() {
            if characters[cursor] == '#' {
                break;
            }
            if characters[cursor] == '"' {
                cursor += 1;
                let mut escaped = false;
                while cursor < characters.len() {
                    let current = characters[cursor];
                    cursor += 1;
                    if escaped {
                        escaped = false;
                    } else if current == '\\' {
                        escaped = true;
                    } else if current == '"' {
                        break;
                    }
                }
            } else if characters[cursor].is_ascii_alphabetic() || characters[cursor] == '_' {
                let start = cursor;
                cursor += 1;
                while cursor < characters.len()
                    && (characters[cursor].is_ascii_alphanumeric() || characters[cursor] == '_')
                {
                    cursor += 1;
                }
                identifiers.push((characters[start..cursor].iter().collect(), line_index + 1));
            } else {
                cursor += 1;
            }
        }
    }
    identifiers
}

fn import_path(line: &str) -> Option<&str> {
    let remainder = line.strip_prefix("import ")?;
    let start = remainder.find('"')? + 1;
    let end = start + remainder[start..].find('"')?;
    Some(&remainder[start..end])
}

fn lint_import_source(import: &str, source_path: &Path, project_root: &Path) -> Option<String> {
    let path = if let Some(relative) = import.strip_prefix("std:") {
        let root = compiler_driver::standard_library_root()?;
        root.join(relative)
    } else if let Some(package_path) = import.strip_prefix("pkg:") {
        let (package, relative) = package_path.split_once('/')?;
        let roots = native_packages::load_locked_roots(project_root).ok()?;
        roots.get(package)?.join(relative)
    } else {
        source_path.parent()?.join(import)
    };
    let path = source_module_path(&path);
    fs::read_to_string(path).ok()
}

fn source_module_path(path: &Path) -> std::path::PathBuf {
    if path.extension().is_some() {
        return path.to_path_buf();
    }
    let guion = path.with_extension("gu");
    let legacy = path.with_extension("vr");
    if guion.is_file() || !legacy.is_file() {
        guion
    } else {
        legacy
    }
}

fn normalize_http_host(input: &str) -> Result<String, String> {
    let host = input.trim().trim_end_matches('.').to_ascii_lowercase();
    let valid = !host.is_empty()
        && host.is_ascii()
        && host.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b':' | b'[' | b']')
        });
    if !valid {
        return Err(format!("invalid exact network host `{host}`"));
    }
    Ok(host)
}

fn runtime_policy(root: &std::path::Path) -> Result<HostPolicy, String> {
    let manifest_path = project_manifest_path(root);
    if !manifest_path.is_file() {
        return Ok(HostPolicy::default());
    }
    let text = fs::read_to_string(&manifest_path)
        .map_err(|error| format!("cannot read {}: {error}", manifest_path.display()))?;
    let manifest: toml::Value = toml::from_str(&text)
        .map_err(|error| format!("{}: invalid TOML: {error}", manifest_path.display()))?;
    let mut policy = HostPolicy::default();
    if let Some(runtime) = manifest.get("runtime") {
        let runtime = runtime
            .as_table()
            .ok_or("guion.toml [runtime] must be a table")?;
        let allowed = [
            "allocation_limit_bytes",
            "call_depth_limit",
            "instruction_limit",
            "max_collection_items",
            "max_file_bytes",
            "max_integer_digits",
            "max_parallelism",
            "max_text_bytes",
            "output_limit_bytes",
        ];
        if let Some(key) = runtime.keys().find(|key| !allowed.contains(&key.as_str())) {
            return Err(format!("unsupported guion.toml [runtime] field `{key}`"));
        }
        policy.instruction_limit =
            runtime_u64(runtime, "instruction_limit", policy.instruction_limit)?;
        policy.allocation_limit_bytes = runtime_usize(
            runtime,
            "allocation_limit_bytes",
            policy.allocation_limit_bytes,
        )?;
        policy.call_depth_limit =
            runtime_usize(runtime, "call_depth_limit", policy.call_depth_limit)?;
        policy.output_limit_bytes =
            runtime_usize(runtime, "output_limit_bytes", policy.output_limit_bytes)?;
        policy.max_integer_digits =
            runtime_usize(runtime, "max_integer_digits", policy.max_integer_digits)?;
        policy.max_text_bytes = runtime_usize(runtime, "max_text_bytes", policy.max_text_bytes)?;
        policy.max_collection_items =
            runtime_usize(runtime, "max_collection_items", policy.max_collection_items)?;
        policy.max_parallelism = runtime_usize(runtime, "max_parallelism", policy.max_parallelism)?;
        policy.max_file_bytes = runtime_usize(runtime, "max_file_bytes", policy.max_file_bytes)?;
    }
    if let Some(network) = manifest.get("network") {
        let network = network
            .as_table()
            .ok_or("guion.toml [network] must be a table")?;
        let allowed = [
            "hosts",
            "timeout_ms",
            "max_request_bytes",
            "max_response_bytes",
            "max_redirects",
        ];
        if let Some(key) = network.keys().find(|key| !allowed.contains(&key.as_str())) {
            return Err(format!("unsupported guion.toml [network] field `{key}`"));
        }
        let hosts = network
            .get("hosts")
            .and_then(toml::Value::as_array)
            .ok_or("guion.toml [network].hosts must be an array of exact host names")?;
        let hosts = hosts
            .iter()
            .map(|value| {
                let host = value
                    .as_str()
                    .ok_or("guion.toml [network].hosts entries must be strings")?;
                normalize_http_host(host)
            })
            .collect::<Result<Vec<_>, String>>()?;
        policy.http_timeout = Duration::from_millis(runtime_u64(
            network,
            "timeout_ms",
            policy.http_timeout.as_millis() as u64,
        )?);
        policy.max_http_request_bytes =
            runtime_usize(network, "max_request_bytes", policy.max_http_request_bytes)?;
        policy.max_http_response_bytes = runtime_usize(
            network,
            "max_response_bytes",
            policy.max_http_response_bytes,
        )?;
        policy.max_http_redirects =
            runtime_nonnegative_usize(network, "max_redirects", policy.max_http_redirects)?;
        if policy.max_http_redirects > 10 {
            return Err("guion.toml [network].max_redirects cannot exceed 10".into());
        }
        policy.allowed_http_hosts = hosts.into_iter().collect();
    }
    if let Some(environment) = manifest.get("environment") {
        let environment = environment
            .as_table()
            .ok_or("guion.toml [environment] must be a table")?;
        if let Some(key) = environment.keys().find(|key| key.as_str() != "variables") {
            return Err(format!(
                "unsupported guion.toml [environment] field `{key}`"
            ));
        }
        let variables = environment
            .get("variables")
            .and_then(toml::Value::as_array)
            .ok_or("guion.toml [environment].variables must be an array of names")?;
        for value in variables {
            let name = value
                .as_str()
                .ok_or("guion.toml [environment].variables entries must be strings")?;
            let mut bytes = name.bytes();
            let first_is_valid = bytes
                .next()
                .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_');
            if !first_is_valid || !bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_') {
                return Err(format!("invalid environment variable name `{name}`"));
            }
            policy.allowed_environment_variables.insert(name.to_owned());
        }
    }
    if let Some(clock) = manifest.get("clock") {
        let clock = clock
            .as_table()
            .ok_or("guion.toml [clock] must be a table")?;
        if let Some(key) = clock.keys().find(|key| key.as_str() != "enabled") {
            return Err(format!("unsupported guion.toml [clock] field `{key}`"));
        }
        policy.allow_clock = clock
            .get("enabled")
            .and_then(toml::Value::as_bool)
            .ok_or("guion.toml [clock].enabled must be a boolean")?;
    }
    Ok(policy)
}

fn runtime_usize(
    runtime: &toml::map::Map<String, toml::Value>,
    field: &str,
    default: usize,
) -> Result<usize, String> {
    let Some(value) = runtime.get(field) else {
        return Ok(default);
    };
    let value = value
        .as_integer()
        .and_then(|value| usize::try_from(value).ok())
        .filter(|value| *value > 0)
        .ok_or_else(|| format!("guion.toml [runtime].{field} must be a positive integer"))?;
    Ok(value)
}

fn runtime_u64(
    runtime: &toml::map::Map<String, toml::Value>,
    field: &str,
    default: u64,
) -> Result<u64, String> {
    let Some(value) = runtime.get(field) else {
        return Ok(default);
    };
    let value = value
        .as_integer()
        .and_then(|value| u64::try_from(value).ok())
        .filter(|value| *value > 0)
        .ok_or_else(|| format!("guion.toml [runtime].{field} must be a positive integer"))?;
    Ok(value)
}

fn runtime_nonnegative_usize(
    runtime: &toml::map::Map<String, toml::Value>,
    field: &str,
    default: usize,
) -> Result<usize, String> {
    let Some(value) = runtime.get(field) else {
        return Ok(default);
    };
    value
        .as_integer()
        .and_then(|value| usize::try_from(value).ok())
        .ok_or_else(|| format!("guion.toml [network].{field} must be a non-negative integer"))
}

fn resolve_target(input: &str) -> Result<(std::path::PathBuf, std::path::PathBuf), String> {
    use std::path::{Component, Path};
    let requested = Path::new(input);
    if !requested.is_dir() {
        let source = requested.to_path_buf();
        let resolved_source = source.canonicalize().ok();
        let source_parent = resolved_source
            .as_deref()
            .unwrap_or(&source)
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let root = source_parent
            .ancestors()
            .find(|ancestor| project_manifest_path(ancestor).is_file())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| source_parent.to_path_buf());
        return Ok((source, root));
    }
    let root = requested
        .canonicalize()
        .map_err(|error| format!("cannot resolve project directory {input}: {error}"))?;
    let manifest_path = project_manifest_path(&root);
    let manifest = fs::read_to_string(&manifest_path)
        .map_err(|error| format!("cannot read {}: {error}", manifest_path.display()))?;
    let document: toml::Value = toml::from_str(&manifest)
        .map_err(|error| format!("{}: invalid TOML: {error}", manifest_path.display()))?;
    let project = document
        .get("project")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| "guion.toml must contain a [project] table".to_owned())?;
    for field in ["name", "version", "entry"] {
        if project.get(field).and_then(toml::Value::as_str).is_none() {
            return Err(format!(
                "guion.toml [project] requires text field `{field}`"
            ));
        }
    }
    let entry = project["entry"].as_str().unwrap();
    let entry_path = Path::new(entry);
    if entry_path.is_absolute()
        || entry_path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err("project entry must be a relative path without `.` or `..` components".into());
    }
    let source = root
        .join(entry_path)
        .canonicalize()
        .map_err(|error| format!("project entry `{entry}` cannot be resolved: {error}"))?;
    if !source.starts_with(&root) || !source.is_file() {
        return Err(format!(
            "project entry `{entry}` is not a file inside the project root"
        ));
    }
    Ok((source, root))
}

fn project_name(path: &std::path::Path) -> String {
    let raw = path
        .file_name()
        .or_else(|| path.parent().and_then(std::path::Path::file_name))
        .unwrap_or_default()
        .to_string_lossy();
    safe_application_id(&raw)
}

fn safe_application_id(raw: &str) -> String {
    let mut result = String::new();
    let mut separator = false;
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() {
            if separator && !result.is_empty() {
                result.push('-');
            }
            result.push(ch.to_ascii_lowercase());
            separator = false;
        } else {
            separator = true;
        }
    }
    if result.is_empty() {
        "guion-project".into()
    } else {
        result
    }
}

// Read-only compatibility for pre-rename Veritas projects. New projects,
// packages, locks, and generated state use the Guion names exclusively.
fn project_manifest_path(root: &Path) -> std::path::PathBuf {
    let current = root.join("guion.toml");
    if current.is_file() {
        current
    } else {
        let legacy = root.join("veritas.toml");
        if legacy.is_file() {
            legacy
        } else {
            current
        }
    }
}

fn project_lock_path(root: &Path) -> std::path::PathBuf {
    let current = root.join("guion.lock");
    if current.is_file() {
        current
    } else {
        let legacy = root.join("veritas.lock");
        if legacy.is_file() {
            legacy
        } else {
            current
        }
    }
}

fn project_bridge_root(root: &Path) -> std::path::PathBuf {
    let current = root.join(".guion/python-bridge");
    let legacy = root.join(".veritas/python-bridge");
    if current.exists() || !legacy.exists() {
        current
    } else {
        legacy
    }
}

fn project_identity(root: &Path) -> Result<serde_json::Value, String> {
    let manifest_path = project_manifest_path(root);
    let manifest: toml::Value = toml::from_str(
        &fs::read_to_string(&manifest_path)
            .map_err(|error| format!("cannot read {}: {error}", manifest_path.display()))?,
    )
    .map_err(|error| format!("{}: invalid TOML: {error}", manifest_path.display()))?;
    let project = manifest
        .get("project")
        .and_then(toml::Value::as_table)
        .ok_or("guion.toml must contain a [project] table")?;
    let name = project
        .get("name")
        .and_then(toml::Value::as_str)
        .ok_or("guion.toml [project].name must be text")?;
    let version = project
        .get("version")
        .and_then(toml::Value::as_str)
        .ok_or("guion.toml [project].version must be text")?;
    let safe_version = version.chars().all(|character| {
        character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '+' | '_')
    });
    if !safe_version || version.is_empty() || version == "." || version == ".." {
        return Err("guion.toml [project].version must contain only letters, numbers, `.`, `-`, `+`, or `_`".into());
    }
    Ok(serde_json::json!({"name": name, "id": safe_application_id(name), "version": version}))
}

fn run_tests(
    path: &str,
    artifact: &Artifact,
    project_root: &std::path::Path,
    json_report: bool,
) -> Result<bool, String> {
    let tests = artifact
        .functions
        .iter()
        .filter(|function| function.name.starts_with("test_"))
        .collect::<Vec<_>>();
    let base_policy = runtime_policy(project_root)?.with_file_access(project_root, true, true);
    let mut failures = 0usize;
    let mut results = Vec::with_capacity(tests.len());
    for test in &tests {
        if !test.parameters.is_empty() || test.result != guion_model::Type::Unit {
            let message = "test functions must have no parameters and return Unit";
            if !json_report {
                eprintln!("FAILED {}: {message}", test.name);
            }
            results.push(serde_json::json!({
                "name": test.name,
                "status": "failed",
                "diagnostic": {
                    "code": "E_TEST_SIGNATURE",
                    "message": message,
                    "trace": []
                }
            }));
            failures += 1;
            continue;
        }
        #[cfg(feature = "python-interop")]
        let mut policy = base_policy.clone();
        #[cfg(not(feature = "python-interop"))]
        let policy = base_policy.clone();
        #[cfg(feature = "python-interop")]
        if let Some(interpreter) = env::var_os("GUION_PYTHON") {
            policy = policy.with_python(interpreter, std::time::Duration::from_secs(15));
        }
        let mut instance = Runtime::new()
            .load(artifact.clone(), policy)
            .map_err(|error| render_runtime(path, artifact, &error))?;
        match instance.invoke(&test.name, vec![]) {
            Ok(_) => {
                if !json_report {
                    println!("ok {}", test.name);
                }
                results.push(serde_json::json!({
                    "name": test.name,
                    "status": "passed"
                }));
            }
            Err(error) => {
                if !json_report {
                    eprintln!(
                        "FAILED {}: {}",
                        test.name,
                        render_runtime(path, artifact, &error)
                    );
                }
                let trace = error
                    .trace
                    .iter()
                    .map(|span| {
                        let source = artifact
                            .sources
                            .get(span.source_id as usize)
                            .map(String::as_str)
                            .unwrap_or(path);
                        serde_json::json!({
                            "source": source,
                            "line": span.line,
                            "column": span.column
                        })
                    })
                    .collect::<Vec<_>>();
                results.push(serde_json::json!({
                    "name": test.name,
                    "status": "failed",
                    "diagnostic": {
                        "code": error.code,
                        "message": error.message,
                        "trace": trace
                    }
                }));
                failures += 1;
            }
        }
        eprint!("{}", instance.take_host_stderr());
        instance.close();
    }
    if json_report {
        println!(
            "{}",
            serde_json::json!({
                "schema_version": 1,
                "kind": "guion_test_report",
                "source": path,
                "summary": {
                    "total": tests.len(),
                    "passed": tests.len().saturating_sub(failures),
                    "failed": failures
                },
                "tests": results
            })
        );
    }
    if failures > 0 {
        if json_report {
            return Ok(false);
        }
        return Err(format!("{failures} of {} tests failed", tests.len()));
    }
    if !json_report {
        println!("{} tests passed", tests.len());
    }
    Ok(true)
}

fn print_cli_diagnostic(code: &str, message: &str) {
    println!(
        "{}",
        serde_json::json!({
            "schema_version": 1,
            "kind": "guion_diagnostic",
            "diagnostic": {
                "code": code,
                "message": message,
            }
        })
    );
}

#[cfg(feature = "python-interop")]
fn print_sync_failure(code: &str, message: &str) {
    println!(
        "{}",
        serde_json::json!({
            "schema_version": 1,
            "kind": "guion_sync_report",
            "status": "failed",
            "diagnostic": {
                "code": code,
                "message": message,
            }
        })
    );
}

#[cfg(feature = "python-interop")]
fn file_sha256(path: &Path) -> Result<String, String> {
    let bytes =
        fs::read(path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn package_failure_diagnostic(message: &str) -> serde_json::Value {
    if let Some((location, detail)) = message.split_once(": error[") {
        if let Some((code, message)) = detail.split_once("]: ") {
            let mut location_parts = location.rsplitn(3, ':');
            let column = location_parts
                .next()
                .and_then(|part| part.parse::<u32>().ok());
            let line = location_parts
                .next()
                .and_then(|part| part.parse::<u32>().ok());
            let source = location_parts.next();
            if let (Some(column), Some(line), Some(source)) = (column, line, source) {
                return serde_json::json!({
                    "schema_version": 1,
                    "kind": "guion_diagnostic",
                    "diagnostic": {
                        "code": code,
                        "message": message,
                        "source": source,
                        "line": line,
                        "column": column,
                    }
                });
            }
        }
    }
    serde_json::json!({
        "schema_version": 1,
        "kind": "guion_diagnostic",
        "diagnostic": {
            "code": "E_PACKAGE_FAILED",
            "message": message,
        }
    })
}

fn render_diagnostic(path: &str, error: &guion_model::Diagnostic) -> String {
    let source = error.source_name.as_deref().unwrap_or(path);
    format!(
        "{source}:{}:{}: error[{}]: {}",
        error.span.line, error.span.column, error.code, error.message
    )
}

fn render_diagnostic_json(path: &str, error: &guion_model::Diagnostic) -> serde_json::Value {
    serde_json::json!({
        "schema_version": 1,
        "kind": "guion_diagnostic",
        "diagnostic": {
            "code": error.code,
            "message": error.message,
            "source": error.source_name.as_deref().unwrap_or(path),
            "line": error.span.line,
            "column": error.span.column
        }
    })
}

fn runtime_diagnostic_json(
    path: &str,
    artifact: &Artifact,
    error: &guion_model::RuntimeError,
) -> serde_json::Value {
    let trace = error
        .trace
        .iter()
        .map(|span| {
            let source = artifact
                .sources
                .get(span.source_id as usize)
                .map(String::as_str)
                .unwrap_or(path);
            serde_json::json!({
                "source": source,
                "line": span.line,
                "column": span.column
            })
        })
        .collect::<Vec<_>>();
    serde_json::json!({
        "code": error.code,
        "message": error.message,
        "trace": trace
    })
}

fn render_runtime(path: &str, artifact: &Artifact, error: &guion_model::RuntimeError) -> String {
    let source_for = |span: &guion_model::Span| {
        artifact
            .sources
            .get(span.source_id as usize)
            .map(String::as_str)
            .unwrap_or(path)
    };
    let where_ = error
        .trace
        .first()
        .map(|s| format!("{}:{}:{}: ", source_for(s), s.line, s.column))
        .unwrap_or_default();
    let frames = error
        .trace
        .iter()
        .skip(1)
        .map(|s| format!("\n  at {}:{}:{}", source_for(s), s.line, s.column))
        .collect::<String>();
    format!(
        "{where_}runtime error[{}]: {}{}",
        error.code, error.message, frames
    )
}

#[cfg(test)]
mod tests {
    use super::{copy_rust_notices, publish_package_stage};
    use std::fs;

    #[test]
    fn copies_and_records_rust_notices_from_the_runtime_distribution() {
        let root = std::env::temp_dir().join(format!(
            "guion rust notices 起点-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let distribution = root.join("distribution");
        let app = root.join("app");
        let licenses = distribution.join("licenses");
        fs::create_dir_all(licenses.join("rust/example-1.2.3")).unwrap();
        fs::write(licenses.join("RUST-DEPENDENCIES.md"), "# Rust notices\n").unwrap();
        fs::write(
            licenses.join("rust/example-1.2.3/LICENSE-MIT"),
            "example license\n",
        )
        .unwrap();
        fs::create_dir_all(&app).unwrap();

        let bundle = copy_rust_notices(&distribution, &app).unwrap().unwrap();

        assert_eq!(
            fs::read_to_string(app.join("licenses/RUST-DEPENDENCIES.md")).unwrap(),
            "# Rust notices\n"
        );
        assert_eq!(
            fs::read_to_string(app.join("licenses/rust/example-1.2.3/LICENSE-MIT")).unwrap(),
            "example license\n"
        );
        assert!(!bundle.inventory_sha256.is_empty());
        assert!(!bundle.files_sha256.is_empty());
        assert!(bundle.files_size_bytes > 0);
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn package_publication_never_replaces_a_destination_created_after_preflight() {
        let root = std::env::temp_dir().join(format!(
            "guion atomic package publication Ω-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let stage = root.join("staged package");
        let destination = root.join("final package");
        fs::create_dir_all(&stage).unwrap();
        fs::create_dir_all(&destination).unwrap();
        fs::write(stage.join("new"), "new package").unwrap();
        fs::write(destination.join("racer"), "another process won").unwrap();

        let error = publish_package_stage(&stage, &destination).unwrap_err();

        assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(
            fs::read_to_string(destination.join("racer")).unwrap(),
            "another process won"
        );
        assert_eq!(
            fs::read_to_string(stage.join("new")).unwrap(),
            "new package"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_package_publication_never_replaces_an_existing_destination() {
        let root = std::env::temp_dir().join(format!(
            "guion atomic package publication Ω-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let stage = root.join("staged package");
        let destination = root.join("final package");
        fs::create_dir_all(&stage).unwrap();
        fs::create_dir_all(&destination).unwrap();
        fs::write(stage.join("new"), "new package").unwrap();
        fs::write(destination.join("racer"), "another process won").unwrap();

        let error = publish_package_stage(&stage, &destination).unwrap_err();

        assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(
            fs::read_to_string(destination.join("racer")).unwrap(),
            "another process won"
        );
        assert_eq!(
            fs::read_to_string(stage.join("new")).unwrap(),
            "new package"
        );
        let _ = fs::remove_dir_all(root);
    }
}
