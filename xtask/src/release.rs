//! Python-free core distribution assembly and audit.
//!
//! The release pipeline intentionally lives in the Rust task runner.  Shell
//! `tar`, `otool`, and `ldd` are used only as platform inspection utilities;
//! they do not decide package membership or integrity.
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Output},
    time::{SystemTime, UNIX_EPOCH},
};

const LINUX_TARGET: &str = "aarch64-unknown-linux-gnu";
const MAC_TARGET: &str = "aarch64-apple-darwin";
const LLVM_EXCEPTION_LICENSE: &str = "distribution/licenses/Apache-2.0_WITH_LLVM-exception.txt";
const RELEASE_DOCS: &[&str] = &[
    "getting-started.md",
    "architecture.md",
    "reference.md",
    "compatibility-and-migrations.md",
    "status.md",
    "self-hosted-compiler.md",
    "agents/start-here.md",
    "agents/recipes.md",
];

#[derive(Clone, Debug)]
pub struct Check {
    pub id: String,
    pub status: String,
    pub command: Vec<String>,
    pub exit_status: Option<i32>,
    pub output: String,
}

fn check(id: &str, status: &str, command: Vec<String>, code: Option<i32>, output: String) -> Check {
    Check {
        id: id.into(),
        status: status.into(),
        command,
        exit_status: code,
        output,
    }
}

fn run(root: &Path, program: &str, args: &[&str]) -> Result<Output, String> {
    Command::new(program)
        .current_dir(root)
        .args(args)
        .output()
        .map_err(|error| format!("start {program} {}: {error}", args.join(" ")))
}

fn run_inherited(
    root: &Path,
    program: &str,
    args: &[&str],
) -> Result<std::process::ExitStatus, String> {
    Command::new(program)
        .current_dir(root)
        .args(args)
        .status()
        .map_err(|error| format!("start {program} {}: {error}", args.join(" ")))
}

fn sha256(path: &Path) -> Result<String, String> {
    let mut file =
        fs::File::open(path).map_err(|error| format!("open {}: {error}", path.display()))?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|error| format!("read {}: {error}", path.display()))?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn copy_tree(source: &Path, destination: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(source)
        .map_err(|error| format!("inspect {}: {error}", source.display()))?;
    if metadata.file_type().is_symlink() {
        return Err(format!(
            "release input contains symlink: {}",
            source.display()
        ));
    }
    if metadata.is_dir() {
        fs::create_dir_all(destination)
            .map_err(|error| format!("create {}: {error}", destination.display()))?;
        let mut entries = fs::read_dir(source)
            .map_err(|error| format!("read {}: {error}", source.display()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("read {}: {error}", source.display()))?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            copy_tree(&entry.path(), &destination.join(entry.file_name()))?;
        }
        return Ok(());
    }
    if !metadata.is_file() {
        return Err(format!("unsupported release input: {}", source.display()));
    }
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("create {}: {error}", parent.display()))?;
    }
    fs::copy(source, destination).map_err(|error| {
        format!(
            "copy {} to {}: {error}",
            source.display(),
            destination.display()
        )
    })?;
    Ok(())
}

fn files(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut pending = vec![root.to_path_buf()];
    let mut result = Vec::new();
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory)
            .map_err(|error| format!("read {}: {error}", directory.display()))?
        {
            let entry = entry.map_err(|error| format!("read release entry: {error}"))?;
            let path = entry.path();
            let meta = fs::symlink_metadata(&path)
                .map_err(|error| format!("inspect {}: {error}", path.display()))?;
            if meta.file_type().is_symlink() {
                return Err(format!("release tree contains symlink: {}", path.display()));
            }
            if meta.is_dir() {
                pending.push(path);
            } else if meta.is_file() {
                result.push(path);
            }
        }
    }
    result.sort();
    Ok(result)
}

fn collect_licenses(root: &Path, target: &str, stage: &Path) -> Result<usize, String> {
    let output = run(
        root,
        "cargo",
        &[
            "metadata",
            "--manifest-path",
            "Cargo.toml",
            "--offline",
            "--format-version",
            "1",
            "--filter-platform",
            target,
        ],
    )?;
    if !output.status.success() {
        return Err(format!(
            "cargo metadata failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let metadata: Value = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("parse cargo metadata: {error}"))?;
    let packages = metadata["packages"]
        .as_array()
        .ok_or("cargo metadata omitted packages")?;
    let mut dependencies = packages
        .iter()
        .filter(|package| package["source"].is_string())
        .collect::<Vec<_>>();
    dependencies.sort_by_key(|package| {
        (
            package["name"].as_str().unwrap_or("").to_lowercase(),
            package["version"].as_str().unwrap_or("").to_owned(),
        )
    });
    let licenses = stage.join("share/guion/licenses/rust");
    fs::create_dir_all(&licenses)
        .map_err(|error| format!("create {}: {error}", licenses.display()))?;
    let mut inventory = format!(
        "# Rust dependency licenses\n\nTarget: {target}\nResolved third-party packages: {}\n\n",
        dependencies.len()
    );
    let mut missing = Vec::new();
    for package in &dependencies {
        let name = package["name"].as_str().unwrap_or("unknown");
        let version = package["version"].as_str().unwrap_or("unknown");
        let license = package["license"]
            .as_str()
            .unwrap_or("see copied license file");
        let manifest = package["manifest_path"]
            .as_str()
            .ok_or("cargo package missing manifest path")?;
        let source = Path::new(manifest)
            .parent()
            .ok_or("cargo manifest has no parent")?;
        let mut candidates = Vec::new();
        for entry in fs::read_dir(source)
            .map_err(|error| format!("read crate {}: {error}", source.display()))?
        {
            let entry = entry.map_err(|error| format!("read crate license entry: {error}"))?;
            let path = entry.path();
            let leaf = entry.file_name().to_string_lossy().to_ascii_lowercase();
            if path.is_file()
                && ["license", "licence", "copying", "notice"]
                    .iter()
                    .any(|prefix| leaf.starts_with(prefix))
            {
                candidates.push(path);
            }
        }
        let nested = source.join("LICENSES");
        if nested.is_dir() {
            candidates.extend(files(&nested)?);
        }
        candidates.sort();
        candidates.dedup();
        if candidates.is_empty() {
            if license == "Apache-2.0 WITH LLVM-exception"
                && root.join(LLVM_EXCEPTION_LICENSE).is_file()
            {
                candidates.push(root.join(LLVM_EXCEPTION_LICENSE));
            } else {
                missing.push(format!("{name} {version} ({license})"));
                continue;
            }
        }
        let folder = licenses.join(format!("{name}-{version}"));
        fs::create_dir_all(&folder)
            .map_err(|error| format!("create {}: {error}", folder.display()))?;
        let mut copied = 0;
        for candidate in candidates {
            let target_path = folder.join(candidate.file_name().ok_or("license file has no name")?);
            fs::copy(&candidate, &target_path)
                .map_err(|error| format!("copy license {}: {error}", candidate.display()))?;
            copied += 1;
        }
        inventory.push_str(&format!(
            "- {name} {version} — `{license}` ({copied} license/notice file(s))\n"
        ));
    }
    if !missing.is_empty() {
        return Err(format!(
            "dependency license files are missing: {}",
            missing.join(", ")
        ));
    }
    fs::write(
        stage.join("share/guion/licenses/RUST-DEPENDENCIES.md"),
        inventory,
    )
    .map_err(|error| format!("write license inventory: {error}"))?;
    let native = stage.join("share/guion/licenses/native");
    fs::create_dir_all(&native).map_err(|error| format!("create native licenses: {error}"))?;
    copy_tree(
        &root.join("distribution/native-components.lock.json"),
        &stage.join("share/guion/native-components.lock.json"),
    )?;
    copy_tree(
        &root.join("distribution/licenses/UPSTREAM-NOTICES.md"),
        &native.join("UPSTREAM-NOTICES.md"),
    )?;
    copy_tree(
        &root.join("distribution/licenses/libical-LGPL-2.1.txt"),
        &native.join("libical-LGPL-2.1.txt"),
    )?;
    Ok(dependencies.len())
}

fn safe_archive_listing(text: &str, expected_root: &str) -> Result<usize, String> {
    let mut seen = std::collections::BTreeSet::new();
    for line in text.lines() {
        let name = line.strip_suffix('/').unwrap_or(line);
        if name.is_empty()
            || name.starts_with('/')
            || name.contains('\\')
            || name.as_bytes().contains(&0)
        {
            return Err(format!("unsafe release archive path: {line:?}"));
        }
        let parts = name.split('/').collect::<Vec<_>>();
        if parts
            .iter()
            .any(|part| part.is_empty() || *part == "." || *part == "..")
            || parts.first() != Some(&expected_root)
        {
            return Err(format!(
                "release archive path is outside {expected_root}: {line:?}"
            ));
        }
        if !seen.insert(name.to_owned()) {
            return Err(format!("duplicate release archive path: {line}"));
        }
    }
    if seen.is_empty() || !seen.contains(expected_root) {
        return Err("release archive is empty or has no package root".into());
    }
    Ok(seen.len())
}

fn verify_package_manifest(package: &Path, expected_target: &str) -> Result<(), String> {
    let manifest_path = package.join("share/guion/release-manifest.json");
    let manifest: Value = serde_json::from_slice(
        &fs::read(&manifest_path)
            .map_err(|error| format!("read {}: {error}", manifest_path.display()))?,
    )
    .map_err(|error| format!("parse release manifest: {error}"))?;
    if manifest["target"].as_str() != Some(expected_target) {
        return Err(format!(
            "release target mismatch: expected {expected_target}, found {}",
            manifest["target"]
        ));
    }
    let entries = manifest["files"]
        .as_object()
        .ok_or("release manifest has no file map")?;
    for (relative, expected) in entries {
        let relative_path = Path::new(relative);
        if relative_path
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
        {
            return Err(format!("unsafe path in release manifest: {relative}"));
        }
        let path = package.join(relative_path);
        if !path.starts_with(package) || !path.is_file() {
            return Err(format!("release file is missing or unsafe: {relative}"));
        }
        let actual = sha256(&path)?;
        if expected.as_str() != Some(actual.as_str()) {
            return Err(format!("release file digest mismatch: {relative}"));
        }
    }
    if entries.is_empty() {
        return Err("release manifest contains no files".into());
    }
    Ok(())
}

fn os_floor(root: &Path, target: &str, binary: &Path) -> Result<String, String> {
    let path = binary.to_str().ok_or("release binary path is not UTF-8")?;
    if target == MAC_TARGET {
        let output = run(root, "otool", &["-l", path])?;
        if !output.status.success() {
            return Err(format!(
                "otool could not inspect deployment target: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            let fields = line.split_whitespace().collect::<Vec<_>>();
            if fields.first() == Some(&"minos") && fields.len() > 1 {
                return Ok(format!("macOS {} (LC_BUILD_VERSION)", fields[1]));
            }
        }
        return Err("Mach-O image contains no LC_BUILD_VERSION minimum OS".into());
    }
    let output = run(root, "readelf", &["--version-info", "--wide", path])?;
    if !output.status.success() {
        return Err(format!(
            "readelf could not inspect glibc requirements: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let mut versions = std::collections::BTreeSet::new();
    for token in String::from_utf8_lossy(&output.stdout).split(|character: char| {
        !(character.is_ascii_alphanumeric() || character == '_' || character == '.')
    }) {
        if let Some(version) = token.strip_prefix("GLIBC_") {
            if version
                .chars()
                .next()
                .is_some_and(|character| character.is_ascii_digit())
            {
                versions.insert(version.to_owned());
            }
        }
    }
    let maximum = versions
        .iter()
        .max_by_key(|version| {
            version
                .split('.')
                .map(|part| part.parse::<u32>().unwrap_or(0))
                .collect::<Vec<_>>()
        })
        .cloned()
        .unwrap_or_else(|| "not-dynamic".into());
    let parsed = maximum
        .split('.')
        .map(|part| part.parse::<u32>().unwrap_or(0))
        .collect::<Vec<_>>();
    if parsed.len() >= 2 && (parsed[0] > 2 || (parsed[0] == 2 && parsed[1] > 36)) {
        return Err(format!(
            "release requires GLIBC_{maximum}, above Debian 12's 2.36 floor"
        ));
    }
    Ok(format!(
        "Debian 12 compatible; maximum ELF GLIBC requirement {maximum}"
    ))
}

fn target_info() -> (&'static str, &'static str) {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => (MAC_TARGET, "macos-aarch64"),
        ("linux", "aarch64") => (LINUX_TARGET, "debian-aarch64"),
        _ => ("unsupported", "unsupported"),
    }
}

fn set_readonly(root: &Path) -> Result<(), String> {
    for path in files(root)? {
        let mut permissions = fs::metadata(&path)
            .map_err(|error| format!("stat {}: {error}", path.display()))?
            .permissions();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            permissions.set_mode(if path.ends_with("bin/guion") {
                0o555
            } else {
                0o444
            });
        }
        fs::set_permissions(&path, permissions)
            .map_err(|error| format!("protect {}: {error}", path.display()))?;
    }
    let mut directories = vec![root.to_path_buf()];
    let mut all = Vec::new();
    while let Some(directory) = directories.pop() {
        all.push(directory.clone());
        for entry in fs::read_dir(&directory)
            .map_err(|error| format!("read {}: {error}", directory.display()))?
        {
            let entry = entry.map_err(|error| format!("read release directory: {error}"))?;
            if entry
                .file_type()
                .map_err(|error| format!("stat release path: {error}"))?
                .is_dir()
            {
                directories.push(entry.path());
            }
        }
    }
    all.sort_by_key(|path| std::cmp::Reverse(path.components().count()));
    for path in all {
        let mut permissions = fs::metadata(&path)
            .map_err(|error| format!("stat {}: {error}", path.display()))?
            .permissions();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            permissions.set_mode(0o555);
        }
        fs::set_permissions(&path, permissions)
            .map_err(|error| format!("protect {}: {error}", path.display()))?;
    }
    Ok(())
}

fn set_writable(root: &Path) -> Result<(), String> {
    if !root.exists() {
        return Ok(());
    }
    for path in files(root)? {
        let mut permissions = fs::metadata(&path)
            .map_err(|error| format!("stat {}: {error}", path.display()))?
            .permissions();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            permissions.set_mode(0o644);
        }
        fs::set_permissions(&path, permissions)
            .map_err(|error| format!("allow cleanup {}: {error}", path.display()))?;
    }
    let mut directories = vec![root.to_path_buf()];
    while let Some(directory) = directories.pop() {
        let mut permissions = fs::metadata(&directory)
            .map_err(|error| format!("stat {}: {error}", directory.display()))?
            .permissions();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            permissions.set_mode(0o755);
        }
        fs::set_permissions(&directory, permissions)
            .map_err(|error| format!("allow cleanup {}: {error}", directory.display()))?;
        for entry in fs::read_dir(&directory)
            .map_err(|error| format!("read {}: {error}", directory.display()))?
        {
            let entry = entry.map_err(|error| format!("read cleanup path: {error}"))?;
            if entry
                .file_type()
                .map_err(|error| format!("stat cleanup path: {error}"))?
                .is_dir()
            {
                directories.push(entry.path());
            }
        }
    }
    Ok(())
}

fn release_binary_path(
    root: &Path,
    target: &str,
    cargo_target_directory: Option<&Path>,
) -> PathBuf {
    let target_root = cargo_target_directory
        .map(Path::to_path_buf)
        .unwrap_or_else(|| root.join("target"));
    if target == MAC_TARGET {
        target_root.join("release/guion")
    } else {
        target_root.join(target).join("release/guion")
    }
}

fn release_target(root: &Path, target: &str, slug: &str) -> Result<Vec<Check>, String> {
    let cargo_target_directory = std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from);
    let build_args = if target == MAC_TARGET {
        vec![
            "build",
            "--locked",
            "--offline",
            "--release",
            "-p",
            "guion-cli",
            "--bin",
            "guion",
        ]
    } else {
        vec![
            "build",
            "--locked",
            "--offline",
            "--release",
            "--target",
            target,
            "-p",
            "guion-cli",
            "--bin",
            "guion",
        ]
    };
    eprintln!("building release CLI for {target}");
    let build_status = run_inherited(root, "cargo", &build_args)?;
    if !build_status.success() {
        return Ok(vec![check(
            "release-binary-build",
            "incomplete",
            std::iter::once("cargo".into())
                .chain(build_args.iter().map(|value| (*value).into()))
                .collect(),
            build_status.code(),
            "release build failed; see streamed cargo diagnostics".into(),
        )]);
    }
    let binary = release_binary_path(root, target, cargo_target_directory.as_deref());
    if !binary.is_file() {
        return Err(format!(
            "release build did not produce {}",
            binary.display()
        ));
    }
    let output = root.join(format!("target/completion/releases/{slug}"));
    fs::create_dir_all(&output).map_err(|error| format!("create {}: {error}", output.display()))?;
    let current = output.join("current");
    let previous = output.join("previous");
    if current.exists() {
        if previous.exists() {
            set_writable(&previous)?;
            fs::remove_dir_all(&previous)
                .map_err(|error| format!("remove old recovery package: {error}"))?;
        }
        fs::rename(&current, &previous)
            .map_err(|error| format!("retain prior package: {error}"))?;
    }
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let stage = output.join(format!(".stage-{nonce}"));
    fs::create_dir_all(stage.join("bin"))
        .map_err(|error| format!("create package stage: {error}"))?;
    let package_name = format!("guion-{slug}");
    let result = (|| {
        fs::copy(&binary, stage.join("bin/guion"))
            .map_err(|error| format!("copy release binary: {error}"))?;
        let share = stage.join("share/guion");
        fs::create_dir_all(share.join("stdlib"))
            .map_err(|error| format!("create package standard library: {error}"))?;
        copy_tree(&root.join("language/stdlib"), &share.join("stdlib"))?;
        copy_tree(
            &root.join("distribution/README.md"),
            &share.join("README.md"),
        )?;
        copy_tree(&root.join("LICENSE"), &share.join("LICENSE"))?;
        copy_tree(
            &root.join("THIRD-PARTY-NOTICES.md"),
            &share.join("THIRD-PARTY-NOTICES.md"),
        )?;
        for doc in RELEASE_DOCS {
            let path = root.join("docs").join(doc);
            if path.is_file() {
                copy_tree(&path, &share.join("docs").join(doc))?;
            }
        }
        copy_tree(
            &root.join("language/self_host/compiler.gub"),
            &share.join("compiler.gub"),
        )?;
        copy_tree(
            &root.join("language/self_host/compiler-seed.json"),
            &share.join("compiler-seed.json"),
        )?;
        copy_tree(
            &root.join("language/artifact-format.md"),
            &share.join("artifact-format.md"),
        )?;
        for doc in ["host-api.md", "spec.md"] {
            copy_tree(
                &root.join("language").join(doc),
                &share.join("language").join(doc),
            )?;
        }
        copy_tree(
            &root.join("distribution/install.sh"),
            &stage.join("install.sh"),
        )?;
        let license_count = collect_licenses(root, target, &stage)?;
        let mut hashes = BTreeMap::new();
        for path in files(&stage)? {
            let relative = path
                .strip_prefix(&stage)
                .map_err(|_| "package path escaped stage")?
                .to_string_lossy()
                .replace('\\', "/");
            hashes.insert(relative, sha256(&path)?);
        }
        let seed_hash = sha256(&share.join("compiler.gub"))?;
        let runtime_hash = sha256(&stage.join("bin/guion"))?;
        let stdlib_files = files(&share.join("stdlib"))?;
        let mut stdlib_hash = Sha256::new();
        for path in stdlib_files {
            let relative = path
                .strip_prefix(share.join("stdlib"))
                .map_err(|_| "stdlib path escaped package")?
                .to_string_lossy();
            stdlib_hash.update((relative.len() as u64).to_le_bytes());
            stdlib_hash.update(relative.as_bytes());
            stdlib_hash.update(
                fs::read(&path).map_err(|error| format!("read {}: {error}", path.display()))?,
            );
            stdlib_hash.update([0]);
        }
        let minimum_os = os_floor(root, target, &stage.join("bin/guion"))?;
        let manifest = json!({
            "schema_version": 1, "product": "Guion core", "target": target,
            "release_id": format!("{}-{slug}-{}", env!("CARGO_PKG_VERSION"), &runtime_hash[..12]),
            "minimum_os": minimum_os,
            "architecture": "aarch64", "python_or_uv_required": false,
            "compiler_seed_sha256": seed_hash, "runtime_binary_sha256": runtime_hash,
            "stdlib_sha256": format!("{:x}", stdlib_hash.finalize()), "files": hashes,
            "native_components": "native-components.lock.json",
            "sqlite": "statically linked bundled SQLite; loadable extensions disabled",
            "libical": "statically linked; built-in timezone data enabled",
            "release_dependencies": license_count
        });
        fs::write(
            share.join("release-manifest.json"),
            serde_json::to_vec_pretty(&manifest)
                .map_err(|error| format!("encode release manifest: {error}"))?,
        )
        .map_err(|error| format!("write release manifest: {error}"))?;
        let archive = output.join(format!("{package_name}.tar.gz"));
        let package_root = output.join(&package_name);
        if package_root.exists() {
            set_writable(&package_root)?;
            fs::remove_dir_all(&package_root)
                .map_err(|error| format!("remove old archive staging tree: {error}"))?;
        }
        fs::rename(&stage, &package_root)
            .map_err(|error| format!("prepare named release root: {error}"))?;
        verify_package_manifest(&package_root, target)?;
        let args = [
            "-czf",
            archive.to_str().ok_or("archive path is not UTF-8")?,
            "-C",
            output.to_str().ok_or("release path is not UTF-8")?,
            package_root
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or("package root is not UTF-8")?,
        ];
        let packed = run(root, "tar", &args)?;
        if !packed.status.success() {
            return Err(format!(
                "tar archive failed: {}",
                String::from_utf8_lossy(&packed.stderr)
            ));
        }
        let listed = run(
            root,
            "tar",
            &["-tzf", archive.to_str().ok_or("archive path is not UTF-8")?],
        )?;
        if !listed.status.success() {
            return Err(format!(
                "cannot list release archive: {}",
                String::from_utf8_lossy(&listed.stderr)
            ));
        }
        let count = safe_archive_listing(
            &String::from_utf8_lossy(&listed.stdout),
            package_root
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or("package root is not UTF-8")?,
        )?;
        fs::rename(&package_root, &current)
            .map_err(|error| format!("publish release package: {error}"))?;
        Ok((license_count, count, archive, manifest))
    })();
    let (license_count, archive_entries, archive, manifest) = match result {
        Ok(values) => values,
        Err(error) => {
            let _ = fs::remove_dir_all(&stage);
            let _ = fs::remove_dir_all(output.join(&package_name));
            return Err(error);
        }
    };
    let mut checks = vec![
        check("release-binary-build", "passed", std::iter::once("cargo".into()).chain(build_args.iter().map(|value| (*value).into())).collect(), Some(0), format!("built {} ({})", binary.display(), manifest["release_id"].as_str().unwrap_or("release"))),
        check("release-package-contents", "passed", vec!["cargo xtask release".into()], Some(0), format!("compiler seed, standard library, docs, native component manifest, installer, {} dependency license sets; archive has {archive_entries} safe members", license_count)),
        check("release-archive-audit", "passed", vec!["Rust path/type audit + tar listing".into()], Some(0), format!("{} ({} bytes, sha256 {})", archive.display(), fs::metadata(&archive).map(|meta| meta.len()).unwrap_or(0), sha256(&archive)?)),
    ];
    let candidate = output.join("relocated ✓ package");
    if candidate.exists() {
        set_writable(&candidate)?;
        fs::remove_dir_all(&candidate)
            .map_err(|error| format!("remove prior relocation candidate: {error}"))?;
    }
    copy_tree(&current, &candidate)?;
    verify_package_manifest(&candidate, target)?;
    let tampered = output.join("tampered-package");
    if tampered.exists() {
        set_writable(&tampered)?;
        fs::remove_dir_all(&tampered)
            .map_err(|error| format!("remove old tamper fixture: {error}"))?;
    }
    copy_tree(&current, &tampered)?;
    let mut seed = fs::read(tampered.join("share/guion/compiler.gub"))
        .map_err(|error| format!("read tamper fixture: {error}"))?;
    if seed.is_empty() {
        return Err("embedded compiler seed package is empty".into());
    }
    seed[0] ^= 1;
    fs::write(tampered.join("share/guion/compiler.gub"), seed)
        .map_err(|error| format!("write tamper fixture: {error}"))?;
    let tamper_rejected = verify_package_manifest(&tampered, target).is_err();
    fs::remove_dir_all(&tampered).map_err(|error| format!("remove tamper fixture: {error}"))?;
    checks.push(check(
        "release-tamper-rejected",
        if tamper_rejected { "passed" } else { "failed" },
        vec!["Rust release manifest verification".into()],
        Some(if tamper_rejected { 0 } else { 1 }),
        "modified compiler seed rejected by SHA-256 manifest verification".into(),
    ));
    if !tamper_rejected {
        return Ok(checks);
    }
    let other_target = if target == MAC_TARGET {
        LINUX_TARGET
    } else {
        MAC_TARGET
    };
    let mismatch_rejected = verify_package_manifest(&current, other_target).is_err();
    checks.push(check(
        "release-dependency-target-mismatch",
        if mismatch_rejected {
            "passed"
        } else {
            "failed"
        },
        vec!["Rust release target verification".into()],
        Some(if mismatch_rejected { 0 } else { 1 }),
        format!("package built for a different target ({other_target}) rejected"),
    ));
    if !mismatch_rejected {
        return Ok(checks);
    }
    let candidate_bin = candidate.join("bin/guion");
    set_readonly(&candidate)?;
    let smoke = Command::new(&candidate_bin)
        .arg("--version")
        .current_dir(std::env::temp_dir())
        .env("PATH", "/nonexistent")
        .env_remove("GUION_STDLIB_ROOT")
        .output();
    let smoke = match smoke {
        Ok(output) => output,
        Err(error) => return Err(format!("relocated clean-room launch failed: {error}")),
    };
    let smoke_text = format!(
        "{}{}",
        String::from_utf8_lossy(&smoke.stdout),
        String::from_utf8_lossy(&smoke.stderr)
    );
    let version_ok = smoke.status.success() && smoke_text.to_ascii_lowercase().contains("guion");
    checks.push(check(
        "source-free-readonly-relocation",
        if version_ok { "passed" } else { "failed" },
        vec![candidate_bin.display().to_string(), "--version".into()],
        smoke.status.code(),
        format!(
            "PATH=/nonexistent; package tree read-only; cwd={}; {smoke_text}",
            std::env::temp_dir().display()
        ),
    ));
    if !version_ok {
        return Ok(checks);
    }
    let installer_prefix = output.join("install-prefix ✓");
    if installer_prefix.exists() {
        set_writable(&installer_prefix)?;
        fs::remove_dir_all(&installer_prefix)
            .map_err(|error| format!("remove old install prefix: {error}"))?;
    }
    let install_script = current.join("install.sh");
    let install = Command::new("sh")
        .arg(&install_script)
        .arg(&installer_prefix)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .map_err(|error| format!("start release installer: {error}"))?;
    let installed_command = installer_prefix.join("bin/guion");
    let install_smoke = Command::new(&installed_command)
        .arg("--version")
        .env("PATH", "/usr/bin:/bin")
        .env_remove("GUION_STDLIB_ROOT")
        .output()
        .map_err(|error| format!("launch installed command: {error}"))?;
    let installed_ok = install.status.success()
        && install_smoke.status.success()
        && String::from_utf8_lossy(&install_smoke.stdout).contains("guion");
    checks.push(check(
        "release-installer-clean-room",
        if installed_ok { "passed" } else { "failed" },
        vec![
            "sh".into(),
            install_script.display().to_string(),
            installer_prefix.display().to_string(),
        ],
        install_smoke.status.code(),
        format!(
            "installer and installed command use PATH=/usr/bin:/bin (no Python/uv path); {}{}{}",
            String::from_utf8_lossy(&install.stdout),
            String::from_utf8_lossy(&install_smoke.stdout),
            String::from_utf8_lossy(&install_smoke.stderr)
        ),
    ));
    if !installed_ok {
        return Ok(checks);
    }
    let caller_data = output.join("caller-data ✓");
    fs::create_dir_all(&caller_data)
        .map_err(|error| format!("create caller data directory: {error}"))?;
    let new_project = Command::new(&candidate_bin)
        .args(["new", "example app"])
        .current_dir(&caller_data)
        .env("PATH", "/nonexistent")
        .env_remove("GUION_STDLIB_ROOT")
        .output()
        .map_err(|error| format!("launch installed builder: {error}"))?;
    let source_file = caller_data.join("example app/src/main.gu");
    let caller_ok = new_project.status.success() && source_file.is_file();
    checks.push(check(
        "readonly-install-writable-caller-data",
        if caller_ok { "passed" } else { "failed" },
        vec![
            candidate_bin.display().to_string(),
            "new".into(),
            "example app".into(),
        ],
        new_project.status.code(),
        format!(
            "read-only install; writable project created at {}; {}{}",
            source_file.display(),
            String::from_utf8_lossy(&new_project.stdout),
            String::from_utf8_lossy(&new_project.stderr)
        ),
    ));
    if !caller_ok {
        return Ok(checks);
    }
    let build_check = Command::new(&candidate_bin)
        .arg("check")
        .arg(&source_file)
        .current_dir(&caller_data)
        .env("PATH", "/nonexistent")
        .env_remove("GUION_STDLIB_ROOT")
        .output()
        .map_err(|error| format!("compile relocated project: {error}"))?;
    checks.push(check(
        "bundled-stdlib-from-relocated-install",
        if build_check.status.success() {
            "passed"
        } else {
            "failed"
        },
        vec![
            candidate_bin.display().to_string(),
            "check".into(),
            source_file.display().to_string(),
        ],
        build_check.status.code(),
        format!(
            "{}{}",
            String::from_utf8_lossy(&build_check.stdout),
            String::from_utf8_lossy(&build_check.stderr)
        ),
    ));
    if !build_check.status.success() {
        return Ok(checks);
    }
    let inspect_program = if target == MAC_TARGET { "otool" } else { "ldd" };
    let inspect_args = if target == MAC_TARGET {
        vec!["-L".into(), candidate_bin.display().to_string()]
    } else {
        vec![candidate_bin.display().to_string()]
    };
    let inspection = Command::new(inspect_program).args(&inspect_args).output();
    match inspection {
        Ok(output) if output.status.success() => {
            let dependencies = String::from_utf8_lossy(&output.stdout).to_string();
            let static_native = !dependencies.to_ascii_lowercase().contains("ical")
                && !dependencies.to_ascii_lowercase().contains("sqlite");
            checks.push(check(
                "dynamic-library-audit",
                if static_native { "passed" } else { "failed" },
                std::iter::once(inspect_program.into())
                    .chain(inspect_args)
                    .collect(),
                output.status.code(),
                dependencies,
            ));
        }
        Ok(output) => checks.push(check(
            "dynamic-library-audit",
            "failed",
            std::iter::once(inspect_program.into())
                .chain(inspect_args)
                .collect(),
            output.status.code(),
            String::from_utf8_lossy(&output.stderr).to_string(),
        )),
        Err(error) => checks.push(check(
            "dynamic-library-audit",
            "incomplete",
            std::iter::once(inspect_program.into())
                .chain(inspect_args)
                .collect(),
            None,
            error.to_string(),
        )),
    }
    verify_package_manifest(&current, target)?;
    checks.push(check(
        "release-hashes-stable-after-execution",
        "passed",
        vec!["Rust release manifest verification".into()],
        Some(0),
        "immutable current package hashes remain unchanged after relocation and launches".into(),
    ));
    set_writable(&candidate)?;
    fs::remove_dir_all(&candidate)
        .map_err(|error| format!("remove relocation test package: {error}"))?;
    fs::remove_dir_all(&caller_data)
        .map_err(|error| format!("remove caller data fixture: {error}"))?;
    set_writable(&installer_prefix)?;
    fs::remove_dir_all(&installer_prefix)
        .map_err(|error| format!("remove installer test prefix: {error}"))?;
    Ok(checks)
}

pub fn run_release(root: &Path, _args: &[String]) -> Vec<Check> {
    let (target, slug) = target_info();
    if target == "unsupported" {
        return vec![check(
            "release-target",
            "incomplete",
            vec!["cargo xtask release".into()],
            None,
            format!(
                "release packages support {MAC_TARGET} and {LINUX_TARGET}, host is {}-{}",
                std::env::consts::OS,
                std::env::consts::ARCH
            ),
        )];
    }
    match release_target(root, target, slug) {
        Ok(checks) => checks,
        Err(error) => vec![check(
            "release-package-audit",
            "failed",
            vec!["cargo xtask release".into()],
            Some(1),
            error,
        )],
    }
}

#[cfg(test)]
mod tests {
    use super::{
        collect_licenses, release_binary_path, safe_archive_listing, LINUX_TARGET, MAC_TARGET,
        RELEASE_DOCS,
    };
    use std::{fs, path::Path};

    #[test]
    fn release_license_inventory_uses_retained_libical_license_after_cleanup() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let stage = std::env::temp_dir().join(format!(
            "guion-release-license-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&stage).unwrap();

        let result = collect_licenses(root, MAC_TARGET, &stage);
        let license = stage.join("share/guion/licenses/native/libical-LGPL-2.1.txt");
        let license_contents = fs::read_to_string(&license).ok();
        let _ = fs::remove_dir_all(&stage);

        assert!(
            result.is_ok(),
            "release license collection failed: {result:?}"
        );
        assert!(
            license_contents
                .as_deref()
                .is_some_and(|text| text.starts_with("GNU LESSER GENERAL PUBLIC LICENSE\n")),
            "complete native libical license was not packaged"
        );
    }

    #[test]
    fn release_binary_path_respects_cargo_target_directory() {
        let root = Path::new("/repo");
        let override_root = Path::new("/tmp/guion-target");
        assert_eq!(
            release_binary_path(root, MAC_TARGET, Some(override_root)),
            override_root.join("release/guion")
        );
        assert_eq!(
            release_binary_path(root, LINUX_TARGET, Some(override_root)),
            override_root.join(LINUX_TARGET).join("release/guion")
        );
        assert_eq!(
            release_binary_path(root, LINUX_TARGET, None),
            root.join("target").join(LINUX_TARGET).join("release/guion")
        );
    }

    #[test]
    fn release_listing_rejects_traversal_and_wrong_roots() {
        assert!(safe_archive_listing("guion/bin/guion\nguion/bin/guion\n", "guion").is_err());
        assert!(safe_archive_listing("guion/../escape\n", "guion").is_err());
        assert!(safe_archive_listing("other/file\n", "guion").is_err());
    }
    #[test]
    fn release_listing_accepts_created_package_layout() {
        assert_eq!(
            safe_archive_listing("guion/\nguion/bin/guion\n", "guion").unwrap(),
            2
        );
    }

    #[test]
    fn release_package_selects_current_guion_documentation() {
        for required in [
            "getting-started.md",
            "architecture.md",
            "reference.md",
            "compatibility-and-migrations.md",
            "status.md",
            "agents/start-here.md",
            "agents/recipes.md",
        ] {
            assert!(
                RELEASE_DOCS.contains(&required),
                "release package is missing current Guion document {required}"
            );
        }
        assert!(
            !RELEASE_DOCS.contains(&"completion-report.md"),
            "the archived Veritas completion report must not ship as current Guion documentation"
        );
    }
}
