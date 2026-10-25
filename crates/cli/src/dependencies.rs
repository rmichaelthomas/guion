use sha2::{Digest, Sha256};
use std::{
    env, fs,
    io::Read,
    path::{Component, Path, PathBuf},
    process::{Command, Output},
};

const BRIDGE_DIRECTORY: &str = ".veritas/python-bridge";

#[derive(Clone, Debug)]
pub struct SyncError {
    pub code: &'static str,
    pub message: String,
}

impl SyncError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl From<String> for SyncError {
    fn from(message: String) -> Self {
        Self::new("E_SYNC_FAILED", message)
    }
}

impl From<&str> for SyncError {
    fn from(message: &str) -> Self {
        Self::new("E_SYNC_FAILED", message)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PythonProject {
    version: String,
    dependencies: Vec<String>,
}

pub struct PythonBundle {
    pub version: String,
    pub uv_version: String,
    pub uv_sha256: String,
    pub lock_sha256: String,
    pub python_sha256: String,
    pub python_size_bytes: u64,
    pub python_cache_sha256: String,
    pub python_cache_size_bytes: u64,
    pub dependency_report_sha256: String,
}

fn copy_python_install_contents(source: &Path, destination: &Path) -> Result<(), String> {
    if !source.is_dir() {
        return Err(format!(
            "managed Python install directory does not exist: {}",
            source.display()
        ));
    }
    let source_root = fs::canonicalize(source).map_err(|error| {
        format!(
            "cannot resolve managed Python install {}: {error}",
            source.display()
        )
    })?;
    copy_python_install_tree(&source_root, &source_root, destination, destination)
}

fn copy_python_install_tree(
    source_root: &Path,
    source: &Path,
    destination_root: &Path,
    destination: &Path,
) -> Result<(), String> {
    fs::create_dir_all(destination).map_err(|error| {
        format!(
            "cannot create bundled Python install directory {}: {error}",
            destination.display()
        )
    })?;
    let mut entries = fs::read_dir(source)
        .map_err(|error| format!("cannot inspect managed Python install: {error}"))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("cannot inspect managed Python install: {error}"))?;
    entries.sort();
    for source_path in entries {
        let name = source_path.file_name().ok_or_else(|| {
            format!(
                "managed Python path has no file name: {}",
                source_path.display()
            )
        })?;
        let destination_path = destination.join(name);
        let file_type = fs::symlink_metadata(&source_path)
            .map_err(|error| format!("cannot inspect {}: {error}", source_path.display()))?
            .file_type();
        if file_type.is_symlink() {
            let target = fs::read_link(&source_path)
                .map_err(|error| format!("cannot read link {}: {error}", source_path.display()))?;
            let copied_target = if target.is_absolute() {
                fs::canonicalize(&target)
                    .ok()
                    .and_then(|canonical| {
                        canonical
                            .strip_prefix(source_root)
                            .ok()
                            .map(Path::to_path_buf)
                    })
                    .map(|relative| destination_root.join(relative))
                    .unwrap_or(target)
            } else {
                target
            };
            create_symlink(&copied_target, &source_path, &destination_path)?;
        } else if file_type.is_dir() {
            copy_python_install_tree(
                source_root,
                &source_path,
                destination_root,
                &destination_path,
            )?;
        } else if file_type.is_file() {
            fs::copy(&source_path, &destination_path).map_err(|error| {
                format!(
                    "cannot copy managed Python file {}: {error}",
                    source_path.display()
                )
            })?;
        } else {
            return Err(format!(
                "unsupported entry in managed Python install: {}",
                source_path.display()
            ));
        }
    }
    Ok(())
}

#[cfg(unix)]
fn create_symlink(target: &Path, _source: &Path, destination: &Path) -> Result<(), String> {
    std::os::unix::fs::symlink(target, destination).map_err(|error| {
        format!(
            "cannot copy managed Python link {}: {error}",
            destination.display()
        )
    })
}

#[cfg(windows)]
fn create_symlink(target: &Path, source: &Path, destination: &Path) -> Result<(), String> {
    let target_path = if target.is_absolute() {
        target.to_path_buf()
    } else {
        source
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(target)
    };
    if fs::metadata(&target_path).is_ok_and(|metadata| metadata.is_dir()) {
        std::os::windows::fs::symlink_dir(target, destination)
    } else {
        std::os::windows::fs::symlink_file(target, destination)
    }
    .map_err(|error| {
        format!(
            "cannot copy managed Python link {}: {error}",
            destination.display()
        )
    })
}

#[cfg(not(any(unix, windows)))]
fn create_symlink(_target: &Path, _source: &Path, destination: &Path) -> Result<(), String> {
    Err(format!(
        "managed Python links are unsupported on this host: {}",
        destination.display()
    ))
}

fn find_installed_python(source: &Path, version: &str) -> Result<Option<PathBuf>, String> {
    if !source.is_dir() {
        return Ok(None);
    }
    let prefix = format!("cpython-{version}");
    let major_minor = version.split('.').take(2).collect::<Vec<_>>().join(".");
    let executable = if cfg!(windows) {
        PathBuf::from("python.exe")
    } else {
        PathBuf::from("bin").join(format!("python{major_minor}"))
    };
    let mut entries = fs::read_dir(source)
        .map_err(|error| {
            format!(
                "cannot inspect managed Python install {}: {error}",
                source.display()
            )
        })?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| {
            format!(
                "cannot inspect managed Python install {}: {error}",
                source.display()
            )
        })?;
    entries.sort();
    for entry in entries {
        let Some(name) = entry.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if name.starts_with(&prefix) && entry.join(&executable).is_file() {
            return Ok(Some(entry.join(executable)));
        }
    }
    Ok(None)
}

pub fn bundle_python(root: &Path, package_root: &Path) -> Result<Option<PythonBundle>, String> {
    let package_root = fs::canonicalize(package_root).map_err(|error| {
        format!(
            "cannot resolve Python package staging directory {}: {error}",
            package_root.display()
        )
    })?;
    let manifest_path = root.join("veritas.toml");
    let manifest_bytes = fs::read(&manifest_path)
        .map_err(|error| format!("cannot read {}: {error}", manifest_path.display()))?;
    let manifest_text = std::str::from_utf8(&manifest_bytes)
        .map_err(|error| format!("{} is not UTF-8: {error}", manifest_path.display()))?;
    let Some(project) = parse_python_project(manifest_text)? else {
        return Ok(None);
    };
    let manifest_digest = sha256_bytes(&manifest_bytes)?;
    let source_bridge = root.join(BRIDGE_DIRECTORY);
    let source_uv_lock = source_bridge.join("uv.lock");
    let source_lock = root.join("veritas.lock");
    verify_lock_metadata(
        &source_lock,
        &manifest_digest,
        &source_uv_lock,
        &project.version,
    )?;
    let source_pyproject = source_bridge.join("pyproject.toml");
    if !source_pyproject.is_file() {
        return Err(format!(
            "managed Python bridge project is missing {}; run `veritas sync <project>`",
            source_pyproject.display()
        ));
    }

    let package_bridge = package_root.join(BRIDGE_DIRECTORY);
    fs::create_dir_all(&package_bridge).map_err(|error| {
        format!(
            "cannot create packaged Python bridge {}: {error}",
            package_bridge.display()
        )
    })?;
    fs::copy(&source_pyproject, package_bridge.join("pyproject.toml"))
        .map_err(|error| format!("cannot copy managed Python project: {error}"))?;
    fs::copy(&source_uv_lock, package_bridge.join("uv.lock"))
        .map_err(|error| format!("cannot copy managed Python lock: {error}"))?;
    fs::copy(&source_lock, package_root.join("veritas.lock"))
        .map_err(|error| format!("cannot copy Veritas lock: {error}"))?;

    let source_uv = find_uv()?;
    verify_uv(&source_uv)?;
    let packaged_uv = package_root.join("toolchains/uv");
    fs::create_dir_all(packaged_uv.parent().unwrap())
        .map_err(|error| format!("cannot create bundled toolchain directory: {error}"))?;
    fs::copy(&source_uv, &packaged_uv)
        .map_err(|error| format!("cannot copy pinned uv: {error}"))?;
    let uv_cache = package_root.join(".uv-cache");
    fs::create_dir_all(&uv_cache)
        .map_err(|error| format!("cannot create package dependency cache: {error}"))?;
    let python_install = package_root.join("python");
    if let Some(source_install) = env::var_os("VERITAS_PYTHON_INSTALL_DIR") {
        let source_install = PathBuf::from(source_install);
        let interpreter =
            find_installed_python(&source_install, &project.version)?.ok_or_else(|| {
                format!(
                    "VERITAS_PYTHON_INSTALL_DIR does not contain CPython {}: {}",
                    project.version,
                    source_install.display()
                )
            })?;
        verify_interpreter(&interpreter, &project.version)?;
        copy_python_install_contents(&source_install, &python_install)?;
    } else {
        let install = Command::new(&packaged_uv)
            .args(["python", "install", "--no-bin", "--install-dir"])
            .arg(&python_install)
            .arg("--cache-dir")
            .arg(&uv_cache)
            .arg(&project.version)
            .output()
            .map_err(|error| {
                format!(
                    "cannot install bundled CPython {}: {error}",
                    project.version
                )
            })?;
        ensure_success(
            "bundled CPython install",
            install,
            "check network access and disk space, then retry `veritas package`",
        )?;
    }

    let sync = Command::new(&packaged_uv)
        .args(["sync", "--project"])
        .arg(&package_bridge)
        .args(["--locked", "--python", &project.version, "--managed-python"])
        .arg("--cache-dir")
        .arg(&uv_cache)
        .env("UV_PYTHON_INSTALL_DIR", &python_install)
        .output()
        .map_err(|error| format!("cannot create packaged Python environment: {error}"))?;
    ensure_success(
        "packaged Python dependency sync",
        sync,
        "check network access and the project's locked wheel availability, then retry `veritas package`",
    )?;
    // `uv sync` may replace its own parent directory with a link to the
    // configured Python install directory. Restore the package toolchain as a
    // real directory after uv exits so it never points back into staging.
    let toolchain_dir = package_root.join("toolchains");
    if fs::symlink_metadata(&toolchain_dir)
        .map(|metadata| metadata.file_type().is_symlink())
        .unwrap_or(false)
    {
        fs::remove_file(&toolchain_dir)
            .map_err(|error| format!("cannot restore packaged toolchain directory: {error}"))?;
    } else if toolchain_dir.exists() {
        fs::remove_dir_all(&toolchain_dir)
            .map_err(|error| format!("cannot restore packaged toolchain directory: {error}"))?;
    }
    fs::create_dir_all(&toolchain_dir)
        .map_err(|error| format!("cannot recreate packaged toolchain directory: {error}"))?;
    fs::copy(&source_uv, &packaged_uv)
        .map_err(|error| format!("cannot restore bundled uv executable: {error}"))?;
    let stray_uv = python_install.join("uv");
    if stray_uv.is_file() {
        fs::remove_file(&stray_uv)
            .map_err(|error| format!("cannot remove misplaced uv executable: {error}"))?;
    }
    normalize_python_install_links(&python_install)?;
    normalize_bridge_environment_links(&package_root, &package_bridge.join(".venv"))?;
    let packaged_interpreter = interpreter_path(&package_bridge);
    verify_interpreter(&packaged_interpreter, &project.version)?;
    let uv_sha256 = sha256_file(&packaged_uv)?;
    let lock_sha256 = sha256_file(&source_uv_lock)?;
    let uv_version = pinned_uv_version()?;
    let dependency_report_sha256 = write_python_dependency_report(
        &package_root,
        &package_bridge,
        &python_install,
        &project.version,
        &uv_version,
        &lock_sha256,
    )?;
    let (python_sha256, python_size_bytes) = sha256_tree(&python_install)?;
    let (python_cache_sha256, python_cache_size_bytes) = sha256_python_cache_payload(&uv_cache)?;
    Ok(Some(PythonBundle {
        version: project.version,
        uv_version,
        uv_sha256,
        lock_sha256,
        python_sha256,
        python_size_bytes,
        python_cache_sha256,
        python_cache_size_bytes,
        dependency_report_sha256,
    }))
}

fn write_python_dependency_report(
    package_root: &Path,
    package_bridge: &Path,
    python_install: &Path,
    python_version: &str,
    uv_version: &str,
    lock_sha256: &str,
) -> Result<String, String> {
    let venv = package_bridge.join(".venv");
    let environments = [
        (find_site_packages(python_install)?, "managed-interpreter"),
        (find_site_packages(&venv)?, "project-environment"),
    ];
    let mut distributions = Vec::new();
    for (site_packages, environment) in &environments {
        let mut entries = fs::read_dir(site_packages)
            .map_err(|error| {
                format!(
                    "cannot list Python packages in {}: {error}",
                    site_packages.display()
                )
            })?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("cannot list Python package metadata: {error}"))?;
        entries.retain(|path| {
            path.is_dir()
                && path
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().ends_with(".dist-info"))
        });
        distributions.extend(
            entries
                .into_iter()
                .map(|dist_info| (dist_info, site_packages.clone(), (*environment).to_owned())),
        );
    }
    distributions.sort_by(|left, right| left.0.cmp(&right.0));

    let mut records = Vec::with_capacity(distributions.len());
    let mut warnings = Vec::new();
    let license_root = package_root.join("licenses/python");
    for (dist_info, _site_packages, environment) in distributions {
        let metadata_path = dist_info.join("METADATA");
        let metadata = fs::read_to_string(&metadata_path).map_err(|error| {
            format!(
                "cannot read Python dependency metadata {}: {error}",
                metadata_path.display()
            )
        })?;
        let headers = parse_core_metadata_headers(&metadata);
        let name = headers
            .get("Name")
            .and_then(|values| values.first())
            .cloned()
            .ok_or_else(|| {
                format!(
                    "Python package {} has no Name metadata",
                    dist_info.display()
                )
            })?;
        let version = headers
            .get("Version")
            .and_then(|values| values.first())
            .cloned()
            .ok_or_else(|| format!("Python package {name} has no Version metadata"))?;
        let dist_info_canonical = fs::canonicalize(&dist_info).map_err(|error| {
            format!(
                "cannot resolve Python distribution {}: {error}",
                dist_info.display()
            )
        })?;
        let expression = headers
            .get("License-Expression")
            .and_then(|values| values.first())
            .cloned();
        let legacy_license = headers
            .get("License")
            .and_then(|values| values.first())
            .cloned();
        let package_id = format!(
            "{}-{}",
            safe_license_component(&name),
            safe_license_component(&version)
        );
        let package_license_root = license_root.join(&package_id);
        let mut files = collect_distribution_license_files(&dist_info, &headers)?;
        files.sort_by(|left, right| left.0.cmp(&right.0));
        files.dedup_by(|left, right| left.0 == right.0);
        let mut file_records = Vec::with_capacity(files.len());
        let mut missing = Vec::new();
        for (relative, source) in files {
            let destination = package_license_root.join(&relative);
            let source_canonical = fs::canonicalize(&source).map_err(|error| {
                format!("cannot resolve license file {}: {error}", source.display())
            })?;
            if !source_canonical.starts_with(&dist_info_canonical) || !source_canonical.is_file() {
                return Err(format!(
                    "Python license file escapes its installed distribution: {}",
                    source.display()
                ));
            }
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent)
                    .map_err(|error| format!("cannot create Python license directory: {error}"))?;
            }
            fs::copy(&source_canonical, &destination).map_err(|error| {
                format!("cannot copy Python license {}: {error}", source.display())
            })?;
            let (sha256, size_bytes) = hash_file_contents(&destination)?;
            file_records.push(serde_json::json!({
                "path": destination.strip_prefix(package_root).unwrap().to_string_lossy(),
                "sha256": sha256,
                "size_bytes": size_bytes,
            }));
        }
        for declared in headers.get("License-File").into_iter().flatten() {
            validate_license_relative_path(declared)?;
            let found = dist_info.join("licenses").join(declared).is_file()
                || dist_info.join(declared).is_file();
            if !found {
                missing.push(declared.clone());
            }
        }
        let has_license_identifier = expression
            .as_ref()
            .is_some_and(|value| !value.trim().is_empty())
            || legacy_license
                .as_ref()
                .is_some_and(|value| !value.trim().is_empty());
        let complete = has_license_identifier && missing.is_empty() && !file_records.is_empty();
        if !complete {
            warnings.push(format!(
                "{name} {version}: license metadata or files are incomplete"
            ));
        }
        records.push(serde_json::json!({
            "name": name,
            "version": version,
            "environment": environment,
            "license_expression": expression,
            "legacy_license": legacy_license,
            "license_status": if complete { "complete" } else { "incomplete" },
            "missing_license_files": missing,
            "license_files": file_records,
        }));
    }
    let runtime_components =
        write_runtime_component_licenses(package_root, python_install, python_version, uv_version)?;
    let report = serde_json::json!({
        "schema_version": 1,
        "python_version": python_version,
        "uv_lock_sha256": lock_sha256,
        "packages": records,
        "runtime_components": runtime_components,
        "warnings": warnings,
    });
    let report_bytes = serde_json::to_vec_pretty(&report)
        .map_err(|error| format!("cannot encode Python dependency report: {error}"))?;
    let report_path = package_root.join("PYTHON-DEPENDENCIES.json");
    fs::write(&report_path, &report_bytes)
        .map_err(|error| format!("cannot write {}: {error}", report_path.display()))?;
    hash_file_contents(&report_path).map(|(digest, _)| digest)
}

fn write_runtime_component_licenses(
    package_root: &Path,
    python_install: &Path,
    python_version: &str,
    uv_version: &str,
) -> Result<Vec<serde_json::Value>, String> {
    let mut version_parts = python_version.split('.');
    let major = version_parts
        .next()
        .ok_or_else(|| format!("invalid Python version {python_version}"))?;
    let minor = version_parts
        .next()
        .ok_or_else(|| format!("invalid Python version {python_version}"))?;
    let mut python_roots = fs::read_dir(python_install)
        .map_err(|error| format!("cannot inspect managed Python install: {error}"))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("cannot inspect managed Python install: {error}"))?;
    python_roots.sort();
    let python_license = python_roots
        .iter()
        .map(|root| root.join(format!("lib/python{major}.{minor}/LICENSE.txt")))
        .find(|path| path.is_file())
        .ok_or("managed CPython license file is missing")?;
    let mut components = Vec::new();
    let cpython_relative = Path::new("licenses/runtime/cpython/LICENSE.txt");
    copy_runtime_license(package_root, cpython_relative, &python_license)?;
    let (digest, size) = hash_file_contents(&package_root.join(cpython_relative))?;
    components.push(serde_json::json!({
        "name": "CPython",
        "version": python_version,
        "license_expression": "PSF-2.0",
        "license_files": [{
            "path": cpython_relative.to_string_lossy(),
            "sha256": digest,
            "size_bytes": size
        }]
    }));

    let uv_files: [(&str, &[u8]); 2] = [
        (
            "LICENSE-MIT.txt",
            include_bytes!("../../../distribution/licenses/uv-LICENSE-MIT.txt"),
        ),
        (
            "LICENSE-APACHE.txt",
            include_bytes!("../../../distribution/licenses/uv-LICENSE-APACHE.txt"),
        ),
    ];
    let mut uv_records = Vec::new();
    for (name, bytes) in uv_files {
        let relative = Path::new("licenses/runtime/uv").join(name);
        let destination = package_root.join(&relative);
        fs::create_dir_all(destination.parent().unwrap())
            .map_err(|error| format!("cannot create uv license directory: {error}"))?;
        fs::write(&destination, bytes).map_err(|error| {
            format!("cannot write uv license {}: {error}", destination.display())
        })?;
        let (digest, size) = hash_file_contents(&destination)?;
        uv_records.push(serde_json::json!({
            "path": relative.to_string_lossy(),
            "sha256": digest,
            "size_bytes": size
        }));
    }
    components.push(serde_json::json!({
        "name": "uv",
        "version": uv_version,
        "license_expression": "MIT OR Apache-2.0",
        "license_files": uv_records
    }));

    let tcltk_files: [(&str, &[u8], &str); 2] = [
        (
            "tcl-license.terms",
            include_bytes!("../../../distribution/licenses/tcl-license.terms"),
            "https://raw.githubusercontent.com/tcltk/tcl/core-9-0-branch/license.terms",
        ),
        (
            "tk-license.terms",
            include_bytes!("../../../distribution/licenses/tk-license.terms"),
            "https://raw.githubusercontent.com/tcltk/tk/core-9-0-branch/license.terms",
        ),
    ];
    let mut tcltk_records = Vec::new();
    for (name, bytes, source_url) in tcltk_files {
        let relative = Path::new("licenses/runtime/tcltk").join(name);
        let destination = package_root.join(&relative);
        fs::create_dir_all(destination.parent().unwrap())
            .map_err(|error| format!("cannot create Tcl/Tk license directory: {error}"))?;
        fs::write(&destination, bytes).map_err(|error| {
            format!(
                "cannot write Tcl/Tk license {}: {error}",
                destination.display()
            )
        })?;
        let (digest, size) = hash_file_contents(&destination)?;
        tcltk_records.push(serde_json::json!({
            "path": relative.to_string_lossy(),
            "sha256": digest,
            "size_bytes": size,
            "source_url": source_url
        }));
    }
    components.push(serde_json::json!({
        "name": "Tcl/Tk",
        "version": "9.0",
        "license_name": "Tcl/Tk license.terms",
        "license_files": tcltk_records
    }));
    Ok(components)
}

fn copy_runtime_license(package_root: &Path, relative: &Path, source: &Path) -> Result<(), String> {
    let destination = package_root.join(relative);
    fs::create_dir_all(destination.parent().unwrap())
        .map_err(|error| format!("cannot create runtime license directory: {error}"))?;
    fs::copy(source, &destination)
        .map_err(|error| format!("cannot copy runtime license {}: {error}", source.display()))?;
    Ok(())
}

fn find_site_packages(venv: &Path) -> Result<PathBuf, String> {
    fn visit(path: &Path, depth: usize, matches: &mut Vec<PathBuf>) -> Result<(), String> {
        if depth > 5 {
            return Ok(());
        }
        for entry in fs::read_dir(path).map_err(|error| {
            format!(
                "cannot inspect Python environment {}: {error}",
                path.display()
            )
        })? {
            let entry =
                entry.map_err(|error| format!("cannot inspect Python environment: {error}"))?;
            if !entry
                .file_type()
                .map_err(|error| format!("cannot inspect Python environment entry: {error}"))?
                .is_dir()
            {
                continue;
            }
            let child = entry.path();
            if child
                .file_name()
                .is_some_and(|name| name == "site-packages")
            {
                matches.push(child);
            } else {
                visit(&child, depth + 1, matches)?;
            }
        }
        Ok(())
    }
    let mut matches = Vec::new();
    visit(venv, 0, &mut matches)?;
    match matches.as_slice() {
        [site_packages] => Ok(site_packages.clone()),
        [] => Err(format!(
            "managed Python environment has no site-packages directory under {}",
            venv.display()
        )),
        _ => Err(format!(
            "managed Python environment has ambiguous site-packages directories under {}",
            venv.display()
        )),
    }
}

fn parse_core_metadata_headers(metadata: &str) -> std::collections::BTreeMap<String, Vec<String>> {
    let mut headers = std::collections::BTreeMap::<String, Vec<String>>::new();
    let mut previous: Option<String> = None;
    for line in metadata.lines() {
        if line.is_empty() {
            break;
        }
        if line.starts_with([' ', '\t']) {
            if let Some(key) = previous.as_ref() {
                if let Some(value) = headers.get_mut(key).and_then(|values| values.last_mut()) {
                    value.push(' ');
                    value.push_str(line.trim());
                }
            }
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.to_owned();
        if matches!(
            key.as_str(),
            "Name" | "Version" | "License" | "License-Expression" | "License-File"
        ) {
            headers
                .entry(key.clone())
                .or_default()
                .push(value.trim().to_owned());
            previous = Some(key);
        } else {
            previous = None;
        }
    }
    headers
}

fn collect_distribution_license_files(
    dist_info: &Path,
    headers: &std::collections::BTreeMap<String, Vec<String>>,
) -> Result<Vec<(PathBuf, PathBuf)>, String> {
    fn walk(root: &Path, path: &Path, files: &mut Vec<(PathBuf, PathBuf)>) -> Result<(), String> {
        for entry in fs::read_dir(path)
            .map_err(|error| format!("cannot scan license directory {}: {error}", path.display()))?
        {
            let entry = entry.map_err(|error| format!("cannot scan license files: {error}"))?;
            let child = entry.path();
            let kind = entry
                .file_type()
                .map_err(|error| format!("cannot inspect license file: {error}"))?;
            if kind.is_dir() {
                walk(root, &child, files)?;
            } else if kind.is_file() {
                files.push((child.strip_prefix(root).unwrap().to_path_buf(), child));
            } else {
                return Err(format!(
                    "unsupported special file in Python licenses: {}",
                    child.display()
                ));
            }
        }
        Ok(())
    }

    let mut files = Vec::new();
    let license_dir = dist_info.join("licenses");
    if license_dir.is_dir() {
        walk(&license_dir, &license_dir, &mut files)?;
    }
    for declared in headers.get("License-File").into_iter().flatten() {
        validate_license_relative_path(declared)?;
        let under_licenses = license_dir.join(declared);
        let source = if under_licenses.is_file() {
            under_licenses
        } else {
            let in_dist_info = dist_info.join(declared);
            if in_dist_info.is_file() {
                in_dist_info
            } else {
                continue;
            }
        };
        let relative = source
            .strip_prefix(&license_dir)
            .unwrap_or_else(|_| source.file_name().map(Path::new).unwrap());
        if !files.iter().any(|(existing, _)| existing == relative) {
            files.push((relative.to_path_buf(), source));
        }
    }
    Ok(files)
}

fn validate_license_relative_path(path: &str) -> Result<(), String> {
    let path = Path::new(path);
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err("invalid license-file path in Python package metadata".into());
    }
    Ok(())
}

fn safe_license_component(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-') {
                character.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect()
}

fn hash_file_contents(path: &Path) -> Result<(String, u64), String> {
    let mut file = fs::File::open(path)
        .map_err(|error| format!("cannot read {} for package report: {error}", path.display()))?;
    let mut digest = Sha256::new();
    let mut size = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| format!("cannot hash {}: {error}", path.display()))?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
        size = size
            .checked_add(read as u64)
            .ok_or_else(|| format!("file {} is too large to report", path.display()))?;
    }
    Ok((format!("{:x}", digest.finalize()), size))
}

fn normalize_python_install_links(python_install: &Path) -> Result<(), String> {
    let root = fs::canonicalize(python_install).map_err(|error| {
        format!(
            "cannot resolve bundled Python directory {}: {error}",
            python_install.display()
        )
    })?;
    fn normalize_relative_links(root: &Path, directory: &Path) -> Result<(), String> {
        for entry in fs::read_dir(directory)
            .map_err(|error| format!("cannot inspect bundled Python directory: {error}"))?
        {
            let entry =
                entry.map_err(|error| format!("cannot inspect bundled Python entry: {error}"))?;
            let path = entry.path();
            let file_type = entry
                .file_type()
                .map_err(|error| format!("cannot inspect {}: {error}", path.display()))?;
            if file_type.is_dir() {
                normalize_relative_links(root, &path)?;
                continue;
            }
            if !file_type.is_symlink() {
                continue;
            }
            let target = fs::read_link(&path).map_err(|error| {
                format!("cannot read Python symlink {}: {error}", path.display())
            })?;
            if target.is_absolute() {
                continue;
            }
            let corrected = corrected_relative_link_target(root, &path, &target)?;
            let Some(corrected) = corrected else {
                let is_terminfo = path.strip_prefix(root).ok().is_some_and(|relative| {
                    relative
                        .components()
                        .any(|part| part.as_os_str() == "terminfo")
                });
                if is_terminfo {
                    fs::remove_file(&path).map_err(|error| {
                        format!(
                            "cannot remove dangling optional terminfo alias {}: {error}",
                            path.display()
                        )
                    })?;
                    continue;
                }
                return Err(format!(
                    "bundled Python contains a dangling symlink outside optional terminfo data: {}",
                    path.display()
                ));
            };
            if corrected != target {
                fs::remove_file(&path).map_err(|error| {
                    format!("cannot replace Python symlink {}: {error}", path.display())
                })?;
                create_path_symlink(&corrected, &path)?;
            }
        }
        Ok(())
    }
    normalize_relative_links(&root, &root)?;
    for entry in fs::read_dir(python_install)
        .map_err(|error| format!("cannot inspect bundled Python directory: {error}"))?
    {
        let entry =
            entry.map_err(|error| format!("cannot inspect bundled Python entry: {error}"))?;
        let file_type = entry
            .file_type()
            .map_err(|error| format!("cannot inspect {}: {error}", entry.path().display()))?;
        if !file_type.is_symlink() {
            continue;
        }
        let link_path = entry.path();
        let target = fs::read_link(&link_path).map_err(|error| {
            format!(
                "cannot read Python symlink {}: {error}",
                link_path.display()
            )
        })?;
        if !target.is_absolute() {
            continue;
        }
        let resolved = fs::canonicalize(&target).map_err(|error| {
            format!(
                "bundled Python link {} has an invalid target: {error}",
                link_path.display()
            )
        })?;
        if !resolved.starts_with(&root) {
            return Err(format!(
                "bundled Python link {} escapes its package directory",
                link_path.display()
            ));
        }
        let relative_target = resolved
            .file_name()
            .map(PathBuf::from)
            .ok_or_else(|| format!("invalid bundled Python link target {}", resolved.display()))?;
        fs::remove_file(&link_path).map_err(|error| {
            format!(
                "cannot replace Python link {}: {error}",
                link_path.display()
            )
        })?;
        create_directory_symlink(&relative_target, &link_path)?;
    }
    Ok(())
}

fn corrected_relative_link_target(
    root: &Path,
    link_path: &Path,
    target: &Path,
) -> Result<Option<PathBuf>, String> {
    use std::path::Component;

    let mut current = link_path
        .parent()
        .ok_or_else(|| format!("Python symlink has no parent: {}", link_path.display()))?
        .to_path_buf();
    let mut corrected = PathBuf::new();
    for component in target.components() {
        match component {
            Component::CurDir => corrected.push("."),
            Component::ParentDir => {
                if current == root || !current.pop() || !current.starts_with(root) {
                    return Err(format!(
                        "bundled Python symlink escapes its package: {}",
                        link_path.display()
                    ));
                }
                corrected.push("..");
            }
            Component::Normal(requested) => {
                let entries = fs::read_dir(&current).map_err(|error| {
                    format!(
                        "cannot inspect Python symlink parent {}: {error}",
                        current.display()
                    )
                })?;
                let mut exact = None;
                let mut case_matches = Vec::new();
                for entry in entries {
                    let entry = entry.map_err(|error| {
                        format!("cannot inspect Python symlink target: {error}")
                    })?;
                    let name = entry.file_name();
                    if name == requested {
                        exact = Some(name);
                        break;
                    }
                    if name
                        .to_string_lossy()
                        .eq_ignore_ascii_case(&requested.to_string_lossy())
                    {
                        case_matches.push(name);
                    }
                }
                let Some(actual) =
                    exact.or_else(|| (case_matches.len() == 1).then(|| case_matches.remove(0)))
                else {
                    if case_matches.len() > 1 {
                        return Err(format!(
                            "bundled Python symlink target is ambiguous by case: {}",
                            link_path.display()
                        ));
                    }
                    return Ok(None);
                };
                current.push(&actual);
                if !current.starts_with(root) {
                    return Err(format!(
                        "bundled Python symlink escapes its package: {}",
                        link_path.display()
                    ));
                }
                corrected.push(actual);
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(format!(
                    "bundled Python symlink target must be relative: {}",
                    link_path.display()
                ));
            }
        }
    }
    if fs::canonicalize(&current).is_err() {
        return Ok(None);
    }
    Ok(Some(corrected))
}

fn normalize_bridge_environment_links(
    package_root: &Path,
    environment: &Path,
) -> Result<(), String> {
    let root = fs::canonicalize(package_root)
        .map_err(|error| format!("cannot resolve Python package root: {error}"))?;
    let environment = fs::canonicalize(environment)
        .map_err(|error| format!("cannot resolve bundled Python bridge environment: {error}"))?;
    if !environment.starts_with(&root) {
        return Err("bundled Python bridge environment escapes its package".into());
    }

    fn walk(root: &Path, directory: &Path) -> Result<(), String> {
        for entry in fs::read_dir(directory)
            .map_err(|error| format!("cannot inspect Python bridge environment: {error}"))?
        {
            let entry =
                entry.map_err(|error| format!("cannot inspect Python bridge entry: {error}"))?;
            let path = entry.path();
            let kind = entry
                .file_type()
                .map_err(|error| format!("cannot inspect {}: {error}", path.display()))?;
            if kind.is_dir() {
                walk(root, &path)?;
                continue;
            }
            if !kind.is_symlink() {
                continue;
            }
            let target = fs::read_link(&path).map_err(|error| {
                format!("cannot read Python bridge link {}: {error}", path.display())
            })?;
            if !target.is_absolute() {
                continue;
            }
            let resolved = fs::canonicalize(&target).map_err(|error| {
                format!(
                    "Python bridge link {} has an invalid target: {error}",
                    path.display()
                )
            })?;
            if !resolved.starts_with(root) {
                return Err(format!(
                    "Python bridge link {} escapes its package",
                    path.display()
                ));
            }
            let parent = fs::canonicalize(path.parent().unwrap())
                .map_err(|error| format!("cannot resolve Python bridge link parent: {error}"))?;
            let relative = relative_path(&parent, &resolved)?;
            fs::remove_file(&path).map_err(|error| {
                format!(
                    "cannot replace Python bridge link {}: {error}",
                    path.display()
                )
            })?;
            create_path_symlink(&relative, &path)?;
        }
        Ok(())
    }

    walk(&root, &environment)
}

fn relative_path(from: &Path, to: &Path) -> Result<PathBuf, String> {
    let from = from.components().collect::<Vec<_>>();
    let to = to.components().collect::<Vec<_>>();
    let common = from
        .iter()
        .zip(&to)
        .take_while(|(left, right)| left == right)
        .count();
    if common == 0 {
        return Err("cannot create a relative Python bridge link across filesystem roots".into());
    }
    let mut relative = PathBuf::new();
    for _ in common..from.len() {
        relative.push("..");
    }
    for component in &to[common..] {
        relative.push(component.as_os_str());
    }
    Ok(relative)
}

#[cfg(unix)]
fn create_path_symlink(target: &Path, link: &Path) -> Result<(), String> {
    std::os::unix::fs::symlink(target, link).map_err(|error| {
        format!(
            "cannot make Python bridge link {} relative: {error}",
            link.display()
        )
    })
}

#[cfg(windows)]
fn create_path_symlink(target: &Path, link: &Path) -> Result<(), String> {
    let metadata = fs::metadata(link.parent().unwrap().join(target))
        .map_err(|error| format!("cannot inspect Python bridge link target: {error}"))?;
    if metadata.is_dir() {
        std::os::windows::fs::symlink_dir(target, link)
    } else {
        std::os::windows::fs::symlink_file(target, link)
    }
    .map_err(|error| {
        format!(
            "cannot make Python bridge link {} relative: {error}",
            link.display()
        )
    })
}

#[cfg(not(any(unix, windows)))]
fn create_path_symlink(_target: &Path, link: &Path) -> Result<(), String> {
    Err(format!(
        "cannot make Python bridge link {} relative on this host",
        link.display()
    ))
}

#[cfg(unix)]
fn create_directory_symlink(target: &Path, link: &Path) -> Result<(), String> {
    std::os::unix::fs::symlink(target, link).map_err(|error| {
        format!(
            "cannot make bundled Python link {} relative: {error}",
            link.display()
        )
    })
}

#[cfg(windows)]
fn create_directory_symlink(target: &Path, link: &Path) -> Result<(), String> {
    std::os::windows::fs::symlink_dir(target, link).map_err(|error| {
        format!(
            "cannot make bundled Python link {} relative: {error}",
            link.display()
        )
    })
}

#[cfg(not(any(unix, windows)))]
fn create_directory_symlink(_target: &Path, link: &Path) -> Result<(), String> {
    Err(format!(
        "cannot create a portable Python symlink at {} on this host",
        link.display()
    ))
}

pub fn sha256_tree(root: &Path) -> Result<(String, u64), String> {
    fn visit(root: &Path, path: &Path, digest: &mut Sha256, size: &mut u64) -> Result<(), String> {
        let relative = path.strip_prefix(root).unwrap_or(path);
        digest.update(relative.to_string_lossy().as_bytes());
        digest.update([0]);
        let metadata = fs::symlink_metadata(path).map_err(|error| {
            format!(
                "cannot inspect bundled Python file {}: {error}",
                path.display()
            )
        })?;
        let kind = metadata.file_type();
        if kind.is_symlink() {
            digest.update(b"L");
            let target = fs::read_link(path).map_err(|error| {
                format!(
                    "cannot read bundled Python link {}: {error}",
                    path.display()
                )
            })?;
            digest.update(target.to_string_lossy().as_bytes());
        } else if kind.is_dir() {
            digest.update(b"D");
            let mut entries = fs::read_dir(path)
                .map_err(|error| {
                    format!(
                        "cannot scan bundled Python directory {}: {error}",
                        path.display()
                    )
                })?
                .map(|entry| entry.map(|entry| entry.path()))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| format!("cannot scan bundled Python directory: {error}"))?;
            entries.sort();
            for entry in entries {
                visit(root, &entry, digest, size)?;
            }
        } else if kind.is_file() {
            digest.update(b"F");
            let mut file = fs::File::open(path).map_err(|error| {
                format!(
                    "cannot read bundled Python file {}: {error}",
                    path.display()
                )
            })?;
            let mut buffer = [0u8; 64 * 1024];
            loop {
                let read = file.read(&mut buffer).map_err(|error| {
                    format!(
                        "cannot hash bundled Python file {}: {error}",
                        path.display()
                    )
                })?;
                if read == 0 {
                    break;
                }
                digest.update(&buffer[..read]);
                *size = size
                    .checked_add(read as u64)
                    .ok_or_else(|| "bundled Python runtime is too large to report".to_owned())?;
            }
        } else {
            return Err(format!(
                "unsupported special file in bundled Python runtime: {}",
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

pub fn sha256_python_cache_payload(root: &Path) -> Result<(String, u64), String> {
    fn is_mutable_uv_metadata(path: &Path) -> bool {
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            return false;
        };
        name == ".lock"
            || name.ends_with(".lock")
            || name.ends_with(".http")
            || name.ends_with(".msgpack")
    }

    fn visit(root: &Path, path: &Path, digest: &mut Sha256, size: &mut u64) -> Result<(), String> {
        let relative = path.strip_prefix(root).unwrap_or(path);
        digest.update(relative.to_string_lossy().as_bytes());
        digest.update([0]);
        let metadata = fs::symlink_metadata(path).map_err(|error| {
            format!(
                "cannot inspect offline Python cache entry {}: {error}",
                path.display()
            )
        })?;
        let kind = metadata.file_type();
        if kind.is_symlink() {
            digest.update(b"L");
            let target = fs::read_link(path).map_err(|error| {
                format!(
                    "cannot read offline Python cache link {}: {error}",
                    path.display()
                )
            })?;
            digest.update(target.to_string_lossy().as_bytes());
        } else if kind.is_dir() {
            digest.update(b"D");
            let mut entries = fs::read_dir(path)
                .map_err(|error| {
                    format!(
                        "cannot scan offline Python cache {}: {error}",
                        path.display()
                    )
                })?
                .map(|entry| entry.map(|entry| entry.path()))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| format!("cannot scan offline Python cache: {error}"))?;
            entries.retain(|entry| !is_mutable_uv_metadata(entry));
            entries.sort();
            for entry in entries {
                visit(root, &entry, digest, size)?;
            }
        } else if kind.is_file() {
            digest.update(b"F");
            let mut file = fs::File::open(path).map_err(|error| {
                format!(
                    "cannot read offline Python cache file {}: {error}",
                    path.display()
                )
            })?;
            let mut buffer = [0u8; 64 * 1024];
            loop {
                let read = file.read(&mut buffer).map_err(|error| {
                    format!(
                        "cannot hash offline Python cache file {}: {error}",
                        path.display()
                    )
                })?;
                if read == 0 {
                    break;
                }
                digest.update(&buffer[..read]);
                *size = size
                    .checked_add(read as u64)
                    .ok_or_else(|| "offline Python cache is too large to report".to_owned())?;
            }
        } else {
            return Err(format!(
                "unsupported special file in offline Python cache: {}",
                path.display()
            ));
        }
        Ok(())
    }

    let mut digest = Sha256::new();
    let mut size = 0u64;
    visit(root, root, &mut digest, &mut size)?;
    Ok((format!("{:x}", digest.finalize()), size))
}

pub fn sync(root: &Path, update: bool) -> Result<String, SyncError> {
    let manifest_path = root.join("veritas.toml");
    let manifest_bytes = fs::read(&manifest_path)
        .map_err(|error| format!("cannot read {}: {error}", manifest_path.display()))?;
    let manifest_text = std::str::from_utf8(&manifest_bytes)
        .map_err(|error| format!("{} is not UTF-8: {error}", manifest_path.display()))?;
    let project = parse_python_project(manifest_text)
        .map_err(|message| SyncError::new("E_SYNC_CONFIGURATION", message))?
        .ok_or_else(|| {
            SyncError::new(
                "E_SYNC_CONFIGURATION",
                "project has no [python] table in veritas.toml",
            )
        })?;
    let manifest_digest = sha256_bytes(&manifest_bytes)?;
    let bridge_root = root.join(BRIDGE_DIRECTORY);
    let uv_lock_path = bridge_root.join("uv.lock");
    let lock_path = root.join("veritas.lock");
    let uv = find_uv().map_err(|message| SyncError::new("E_UV_NOT_FOUND", message))?;
    verify_uv_for_sync(&uv)?;

    if uv_lock_path.exists() && !update {
        verify_lock_metadata(
            &lock_path,
            &manifest_digest,
            &uv_lock_path,
            &project.version,
        )
        .map_err(|message| SyncError::new("E_SYNC_LOCK_INVALID", message))?;
    }

    let mut staged_bridge = if update {
        let parent = bridge_root
            .parent()
            .ok_or("managed Python bridge directory has no parent")?;
        fs::create_dir_all(parent)
            .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
        Some(create_sync_stage(parent)?)
    } else {
        None
    };
    let working_bridge = staged_bridge
        .as_ref()
        .map(|stage| stage.path.as_path())
        .unwrap_or(&bridge_root);
    fs::create_dir_all(working_bridge)
        .map_err(|error| format!("cannot create {}: {error}", working_bridge.display()))?;
    let pyproject_path = working_bridge.join("pyproject.toml");
    fs::write(&pyproject_path, render_pyproject(&project))
        .map_err(|error| format!("cannot write {}: {error}", pyproject_path.display()))?;

    let working_uv_lock_path = working_bridge.join("uv.lock");
    let has_lock = working_uv_lock_path.is_file();
    let mut lock_command = Command::new(&uv);
    lock_command
        .args(["lock", "--project"])
        .arg(working_bridge)
        .args(["--python", &project.version, "--managed-python"]);
    if update {
        lock_command.arg("--upgrade");
    } else if has_lock {
        lock_command.arg("--check");
    }
    ensure_success(
        "uv lock",
        lock_command
            .output()
            .map_err(|error| format!("cannot start managed resolver {}: {error}", uv.display()))?,
        "lock verification failed; run `veritas sync --update <project>` to change dependencies",
    )
    .map_err(|message| SyncError::new("E_DEPENDENCY_RESOLUTION_FAILED", message))?;

    let sync = Command::new(&uv)
        .args(["sync", "--project"])
        .arg(working_bridge)
        .args(["--locked", "--python", &project.version, "--managed-python"])
        .output()
        .map_err(|error| format!("cannot start managed resolver {}: {error}", uv.display()))?;
    ensure_success(
        "uv sync",
        sync,
        "dependency installation failed; check the network or uv cache and retry `veritas sync`",
    )
    .map_err(|message| SyncError::new("E_DEPENDENCY_INSTALL_FAILED", message))?;

    let interpreter = interpreter_path(working_bridge);
    verify_interpreter(&interpreter, &project.version)
        .map_err(|message| SyncError::new("E_MANAGED_PYTHON_UNAVAILABLE", message))?;
    let uv_lock_digest = sha256_file(&working_uv_lock_path)?;
    let uv_version = pinned_uv_version()?;
    if let Some(stage) = staged_bridge.take() {
        commit_sync_stage(
            stage,
            &bridge_root,
            &lock_path,
            &manifest_digest,
            &uv_lock_digest,
            &project.version,
            &uv_version,
        )?;
    } else {
        write_lock_metadata(
            &lock_path,
            &manifest_digest,
            &uv_lock_digest,
            &project.version,
            &uv_version,
        )?;
    }
    Ok(format!(
        "synced Python {} and {} declared dependencies",
        project.version,
        project.dependencies.len()
    ))
}

struct SyncStage {
    path: PathBuf,
}

impl Drop for SyncStage {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn create_sync_stage(parent: &Path) -> Result<SyncStage, String> {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| format!("system clock is before UNIX epoch: {error}"))?
        .as_nanos();
    let path = parent.join(format!(
        ".python-bridge-update-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir(&path).map_err(|error| {
        format!(
            "cannot create dependency update stage {}: {error}",
            path.display()
        )
    })?;
    Ok(SyncStage { path })
}

fn commit_sync_stage(
    stage: SyncStage,
    bridge_root: &Path,
    lock_path: &Path,
    manifest_digest: &str,
    uv_lock_digest: &str,
    python_version: &str,
    uv_version: &str,
) -> Result<(), String> {
    let parent = bridge_root
        .parent()
        .ok_or("managed Python bridge directory has no parent")?;
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| format!("system clock is before UNIX epoch: {error}"))?
        .as_nanos();
    let backup = parent.join(format!(
        ".python-bridge-backup-{}-{nonce}",
        std::process::id()
    ));
    let had_existing = match fs::symlink_metadata(bridge_root) {
        Ok(_) => true,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => {
            return Err(format!(
                "cannot inspect existing managed Python environment {}: {error}",
                bridge_root.display()
            ));
        }
    };
    if had_existing {
        fs::rename(bridge_root, &backup).map_err(|error| {
            format!(
                "cannot preserve the existing managed Python environment {}: {error}",
                bridge_root.display()
            )
        })?;
    }
    if let Err(error) = fs::rename(&stage.path, bridge_root) {
        if had_existing {
            if let Err(restore_error) = fs::rename(&backup, bridge_root) {
                return Err(format!(
                    "cannot install updated environment: {error}; previous environment remains at {} because restore failed: {restore_error}",
                    backup.display()
                ));
            }
        }
        return Err(format!(
            "cannot install the updated Python environment at {}: {error}",
            bridge_root.display()
        ));
    }
    if let Err(error) = write_lock_metadata(
        lock_path,
        manifest_digest,
        uv_lock_digest,
        python_version,
        uv_version,
    ) {
        if had_existing {
            if let Err(remove_error) = remove_path(bridge_root) {
                return Err(format!(
                    "cannot record updated dependency locks: {error}; updated environment remains at {} and previous environment remains at {} because rollback cleanup failed: {remove_error}",
                    bridge_root.display(),
                    backup.display()
                ));
            }
            if let Err(restore_error) = fs::rename(&backup, bridge_root) {
                return Err(format!(
                    "cannot record updated dependency locks: {error}; previous environment remains at {} because restore failed: {restore_error}",
                    backup.display()
                ));
            }
        } else if let Err(remove_error) = remove_path(bridge_root) {
            return Err(format!(
                "cannot record updated dependency locks: {error}; incomplete environment remains at {} and could not be removed: {remove_error}",
                bridge_root.display()
            ));
        }
        return Err(format!(
            "cannot record updated Python dependency locks; previous environment restored: {error}"
        ));
    }
    if had_existing {
        let _ = remove_path(&backup);
    }
    Ok(())
}

fn remove_path(path: &Path) -> Result<(), String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("cannot inspect {}: {error}", path.display())),
    };
    if metadata.file_type().is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
    .map_err(|error| format!("cannot remove {}: {error}", path.display()))
}

pub fn interpreter_for_locked_run(root: &Path) -> Result<Option<PathBuf>, String> {
    let manifest_path = root.join("veritas.toml");
    let manifest_text = match fs::read_to_string(&manifest_path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("cannot read {}: {error}", manifest_path.display())),
    };
    let Some(project) = parse_python_project(&manifest_text)? else {
        return Ok(None);
    };

    let manifest_digest = sha256_bytes(manifest_text.as_bytes())?;
    let bridge_root = root.join(BRIDGE_DIRECTORY);
    let uv_lock_path = bridge_root.join("uv.lock");
    let lock_path = root.join("veritas.lock");
    verify_lock_metadata(
        &lock_path,
        &manifest_digest,
        &uv_lock_path,
        &project.version,
    )?;

    let uv = find_uv()?;
    verify_uv(&uv)?;
    let sync = Command::new(&uv)
        .args(["sync", "--project"])
        .arg(&bridge_root)
        .args([
            "--locked",
            "--offline",
            "--python",
            &project.version,
            "--managed-python",
        ])
        .output()
        .map_err(|error| format!("cannot start managed resolver {}: {error}", uv.display()))?;
    ensure_success(
        "offline uv sync",
        sync,
        "locked Python environment is incomplete offline; run `veritas sync <project>` first",
    )?;

    let interpreter = interpreter_path(&bridge_root);
    verify_interpreter(&interpreter, &project.version)?;
    Ok(Some(interpreter))
}

pub fn doctor_status() -> String {
    match find_uv().and_then(|uv| {
        verify_uv(&uv)?;
        Ok(format!("uv {}: {}", pinned_uv_version()?, uv.display()))
    }) {
        Ok(status) => format!("managed Python toolchain: {status}"),
        Err(error) => format!("managed Python toolchain unavailable: {error}"),
    }
}

pub fn doctor_report() -> serde_json::Value {
    let toolchain = find_uv()
        .map_err(|message| SyncError::new("E_UV_NOT_FOUND", message))
        .and_then(|uv| verify_uv_for_sync(&uv).map(|()| uv));
    let (toolchain_status, toolchain_record) = match toolchain {
        Ok(uv) => (
            "available",
            serde_json::json!({
                "status": "available",
                "uv_version": pinned_uv_version().unwrap_or_default(),
                "path": uv,
                "diagnostic": null,
            }),
        ),
        Err(error) => (
            "unavailable",
            serde_json::json!({
                "status": "unavailable",
                "diagnostic": {
                    "code": error.code,
                    "message": error.message,
                },
            }),
        ),
    };
    let python = env::var_os("VERITAS_PYTHON");
    let (sidecar_mode, interpreter) = match python {
        Some(path) => ("explicit", Some(path.to_string_lossy().into_owned())),
        None if toolchain_status == "available" => ("managed", None),
        None => ("not_configured", None),
    };
    serde_json::json!({
        "schema_version": 1,
        "kind": "veritas_doctor_report",
        "veritas": {
            "version": env!("CARGO_PKG_VERSION"),
            "artifact_format": veritas_model::Artifact::CURRENT_VERSION,
        },
        "host": {
            "os": env::consts::OS,
            "architecture": env::consts::ARCH,
        },
        "runtime": {
            "status": "available",
            "mode": "native_bytecode_vm",
        },
        "python": {
            "sidecar_mode": sidecar_mode,
            "interpreter": interpreter,
        },
        "managed_toolchain": toolchain_record,
        "capabilities": {
            "pure_veritas": true,
            "python_calls": sidecar_mode == "explicit" || toolchain_status == "available",
        },
    })
}

fn parse_python_project(manifest: &str) -> Result<Option<PythonProject>, String> {
    let value: toml::Value =
        toml::from_str(manifest).map_err(|error| format!("invalid veritas.toml: {error}"))?;
    let Some(python) = value.get("python") else {
        return Ok(None);
    };
    let table = python
        .as_table()
        .ok_or("veritas.toml [python] must be a table")?;
    let version = table
        .get("version")
        .and_then(toml::Value::as_str)
        .ok_or("veritas.toml [python] requires a pinned `version` such as `3.14.8`")?
        .to_owned();
    if !is_pinned_python_version(&version) {
        return Err(format!(
            "Python version `{version}` must be an exact major.minor.patch value"
        ));
    }
    let dependencies = match table.get("dependencies") {
        None => Vec::new(),
        Some(value) => value
            .as_array()
            .ok_or("veritas.toml [python].dependencies must be an array of package requirements")?
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .filter(|requirement| !requirement.trim().is_empty())
                    .map(str::to_owned)
                    .ok_or("Python dependency requirements must be non-empty strings")
            })
            .collect::<Result<Vec<_>, _>>()?,
    };
    Ok(Some(PythonProject {
        version,
        dependencies,
    }))
}

fn is_pinned_python_version(version: &str) -> bool {
    let mut parts = version.split('.');
    let Some(major) = parts.next() else {
        return false;
    };
    let Some(minor) = parts.next() else {
        return false;
    };
    let Some(patch) = parts.next() else {
        return false;
    };
    parts.next().is_none()
        && [major, minor, patch]
            .into_iter()
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
}

fn render_pyproject(project: &PythonProject) -> String {
    let mut metadata = toml::map::Map::new();
    metadata.insert(
        "name".into(),
        toml::Value::String("veritas-python-bridge".into()),
    );
    metadata.insert("version".into(), toml::Value::String("0.0.0".into()));
    metadata.insert(
        "requires-python".into(),
        toml::Value::String(format!("=={}", project.version)),
    );
    metadata.insert(
        "dependencies".into(),
        toml::Value::Array(
            project
                .dependencies
                .iter()
                .cloned()
                .map(toml::Value::String)
                .collect(),
        ),
    );
    let mut root = toml::map::Map::new();
    root.insert("project".into(), toml::Value::Table(metadata));
    toml::to_string_pretty(&toml::Value::Table(root)).expect("serializing a TOML value cannot fail")
}

fn find_uv() -> Result<PathBuf, String> {
    if let Some(path) = env::var_os("VERITAS_UV") {
        return Ok(PathBuf::from(path));
    }
    if let Ok(executable) = env::current_exe() {
        if let Some(directory) = executable.parent() {
            let bundled = directory.join("toolchains/uv");
            if bundled.is_file() {
                return Ok(bundled);
            }
        }
    }
    if Command::new("uv").arg("--version").output().is_ok() {
        return Ok(PathBuf::from("uv"));
    }
    Err("uv is not bundled or available on PATH; set VERITAS_UV to the pinned uv executable".into())
}

fn verify_uv(uv: &Path) -> Result<(), String> {
    let expected_version = pinned_uv_version()?;
    let output = Command::new(uv)
        .arg("--version")
        .output()
        .map_err(|error| format!("cannot run uv at {}: {error}", uv.display()))?;
    if !output.status.success() {
        return Err(format!("uv --version failed at {}", uv.display()));
    }
    let version_line = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let found = version_line.split_whitespace().nth(1).unwrap_or_default();
    if found != expected_version {
        return Err(format!(
            "expected pinned uv {expected_version}, found `{version_line}` at {}",
            uv.display()
        ));
    }
    Ok(())
}

fn verify_uv_for_sync(uv: &Path) -> Result<(), SyncError> {
    let expected_version = pinned_uv_version()?;
    let output = Command::new(uv)
        .arg("--version")
        .output()
        .map_err(|error| {
            SyncError::new(
                "E_UV_NOT_FOUND",
                format!("cannot run uv at {}: {error}", uv.display()),
            )
        })?;
    if !output.status.success() {
        return Err(SyncError::new(
            "E_UV_UNAVAILABLE",
            format!("uv --version failed at {}", uv.display()),
        ));
    }
    let version_line = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let found_version = version_line.split_whitespace().nth(1).unwrap_or_default();
    if found_version != expected_version {
        return Err(SyncError::new(
            "E_UV_VERSION_MISMATCH",
            format!(
                "expected pinned uv {expected_version}, found `{version_line}` at {}",
                uv.display()
            ),
        ));
    }
    Ok(())
}

fn interpreter_path(bridge_root: &Path) -> PathBuf {
    if cfg!(windows) {
        bridge_root.join(".venv/Scripts/python.exe")
    } else {
        bridge_root.join(".venv/bin/python3")
    }
}

fn verify_interpreter(interpreter: &Path, expected: &str) -> Result<(), String> {
    let output = Command::new(interpreter)
        .arg("--version")
        .output()
        .map_err(|error| {
            format!(
                "managed CPython is missing at {}: {error}; run `veritas sync`",
                interpreter.display()
            )
        })?;
    if !output.status.success() {
        return Err(format!(
            "managed CPython failed at {}",
            interpreter.display()
        ));
    }
    let actual = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let expected_line = format!("Python {expected}");
    if actual != expected_line {
        return Err(format!(
            "managed interpreter at {} reports `{actual}`, expected `{expected_line}`",
            interpreter.display()
        ));
    }
    Ok(())
}

fn verify_lock_metadata(
    lock_path: &Path,
    manifest_digest: &str,
    uv_lock_path: &Path,
    python_version: &str,
) -> Result<(), String> {
    if !uv_lock_path.is_file() || !lock_path.is_file() {
        return Err("managed Python lock is missing; run `veritas sync <project>`".into());
    }
    let lock_text = fs::read_to_string(lock_path)
        .map_err(|error| format!("cannot read {}: {error}", lock_path.display()))?;
    let lock: toml::Value = toml::from_str(&lock_text).map_err(|error| {
        format!(
            "{} is invalid: {error}; run `veritas sync --update`",
            lock_path.display()
        )
    })?;
    let recorded_manifest = lock.get("manifest_sha256").and_then(toml::Value::as_str);
    let recorded_uv_lock = lock.get("uv_lock_sha256").and_then(toml::Value::as_str);
    let recorded_uv_version = lock.get("uv_version").and_then(toml::Value::as_str);
    let recorded_python_version = lock.get("python_version").and_then(toml::Value::as_str);
    let recorded_target = lock.get("target").and_then(toml::Value::as_str);
    let format_version = lock.get("format_version").and_then(toml::Value::as_integer);
    let current_target = format!("{}-{}", env::consts::OS, env::consts::ARCH);
    let current_uv_lock = sha256_file(uv_lock_path)?;
    if format_version != Some(1)
        || recorded_uv_version != Some(pinned_uv_version()?.as_str())
        || recorded_python_version != Some(python_version)
        || recorded_target != Some(current_target.as_str())
        || recorded_manifest != Some(manifest_digest)
        || recorded_uv_lock != Some(current_uv_lock.as_str())
    {
        return Err("managed Python lock does not match this target, veritas.toml, or uv.lock; run `veritas sync --update <project>` to refresh it".into());
    }
    Ok(())
}

fn write_lock_metadata(
    lock_path: &Path,
    manifest_digest: &str,
    uv_lock_digest: &str,
    python_version: &str,
    uv_version: &str,
) -> Result<(), String> {
    let mut lock = toml::map::Map::new();
    lock.insert("format_version".into(), toml::Value::Integer(1));
    lock.insert("uv_version".into(), toml::Value::String(uv_version.into()));
    lock.insert(
        "python_version".into(),
        toml::Value::String(python_version.into()),
    );
    lock.insert(
        "target".into(),
        toml::Value::String(format!("{}-{}", env::consts::OS, env::consts::ARCH)),
    );
    lock.insert(
        "manifest_sha256".into(),
        toml::Value::String(manifest_digest.into()),
    );
    lock.insert(
        "uv_lock_sha256".into(),
        toml::Value::String(uv_lock_digest.into()),
    );
    let serialized = toml::to_string_pretty(&toml::Value::Table(lock))
        .map_err(|error| format!("cannot encode {}: {error}", lock_path.display()))?;
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| format!("system clock is before UNIX epoch: {error}"))?
        .as_nanos();
    let temporary = lock_path.with_extension(format!("lock.tmp-{}-{nonce}", std::process::id()));
    fs::write(&temporary, serialized)
        .map_err(|error| format!("cannot write {}: {error}", temporary.display()))?;
    fs::rename(&temporary, lock_path)
        .map_err(|error| format!("cannot replace {}: {error}", lock_path.display()))
}

fn pinned_uv_version() -> Result<String, String> {
    let manifest: toml::Value = toml::from_str(include_str!(
        "../../../distribution/toolchains/manifest.toml"
    ))
    .map_err(|error| format!("invalid bundled toolchain manifest: {error}"))?;
    manifest
        .get("uv_version")
        .and_then(toml::Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| "bundled toolchain manifest has no uv_version".into())
}

fn sha256_bytes(bytes: &[u8]) -> Result<String, String> {
    let directory = env::temp_dir();
    let path = directory.join(format!("veritas-manifest-{}.tmp", std::process::id()));
    fs::write(&path, bytes)
        .map_err(|error| format!("cannot stage manifest for hashing: {error}"))?;
    let digest = sha256_file(&path);
    let _ = fs::remove_file(path);
    digest
}

fn sha256_file(path: &Path) -> Result<String, String> {
    for (program, arguments) in [("shasum", vec!["-a", "256"]), ("sha256sum", vec![])] {
        let output = match Command::new(program).args(arguments).arg(path).output() {
            Ok(output) => output,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(format!("cannot run {program}: {error}")),
        };
        if !output.status.success() {
            return Err(format!("{program} failed to hash {}", path.display()));
        }
        let digest = String::from_utf8_lossy(&output.stdout)
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .to_owned();
        if digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Ok(digest);
        }
        return Err(format!("{program} returned an invalid SHA-256 digest"));
    }
    Err("SHA-256 utility unavailable (need shasum or sha256sum)".into())
}

fn ensure_success(label: &str, output: Output, recovery: &str) -> Result<(), String> {
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        Err(format!("{label} failed: {stderr}\n{recovery}"))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        copy_python_install_contents, is_pinned_python_version, normalize_bridge_environment_links,
        normalize_python_install_links, parse_python_project, render_pyproject,
        verify_lock_metadata, write_lock_metadata,
    };
    use std::fs;

    #[test]
    fn requires_an_exact_python_patch_version() {
        assert!(is_pinned_python_version("3.14.8"));
        assert!(!is_pinned_python_version("3.14"));
        assert!(!is_pinned_python_version(">=3.14"));
        assert!(!is_pinned_python_version("3.14.8.1"));
    }

    #[cfg(unix)]
    #[test]
    fn copies_an_installed_python_runtime_without_breaking_relative_links() {
        use std::os::unix::fs::symlink;

        let root = std::env::temp_dir().join(format!(
            "veritas python install copy-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let source = root.join("installed");
        let destination = root.join("package/python");
        let runtime = source.join("cpython-3.14.8-test");
        fs::create_dir_all(runtime.join("bin")).unwrap();
        fs::write(runtime.join("bin/python3.14"), b"runtime").unwrap();
        fs::write(source.join(".lock"), b"install metadata").unwrap();
        symlink("cpython-3.14.8-test", source.join("cpython-3.14")).unwrap();

        copy_python_install_contents(&source, &destination).unwrap();

        assert_eq!(
            fs::read(destination.join("cpython-3.14.8-test/bin/python3.14")).unwrap(),
            b"runtime"
        );
        assert_eq!(
            fs::read_link(destination.join("cpython-3.14")).unwrap(),
            std::path::PathBuf::from("cpython-3.14.8-test")
        );
        assert_eq!(
            fs::read(destination.join("cpython-3.14/bin/python3.14")).unwrap(),
            b"runtime"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn rebases_absolute_links_into_the_copied_python_runtime() {
        use std::os::unix::fs::symlink;

        let root = std::env::temp_dir().join(format!(
            "veritas python absolute install link-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let source = root.join("installed");
        let destination = root.join("package/python");
        let runtime = source.join("cpython-3.14.8-test");
        fs::create_dir_all(runtime.join("bin")).unwrap();
        fs::write(runtime.join("bin/python3.14"), b"runtime").unwrap();
        symlink(&runtime, source.join("cpython-3.14")).unwrap();

        copy_python_install_contents(&source, &destination).unwrap();

        assert_eq!(
            fs::canonicalize(destination.join("cpython-3.14")).unwrap(),
            fs::canonicalize(destination.join("cpython-3.14.8-test")).unwrap()
        );
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn finds_only_a_runtime_install_matching_the_pinned_patch_version() {
        use std::os::unix::fs::PermissionsExt;

        let root = std::env::temp_dir().join(format!(
            "veritas python install lookup-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let runtime = root.join("cpython-3.14.8-linux-test/bin/python3.14");
        fs::create_dir_all(runtime.parent().unwrap()).unwrap();
        fs::write(&runtime, "#!/bin/sh\nprintf 'Python 3.14.8\\n'\n").unwrap();
        let mut permissions = fs::metadata(&runtime).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&runtime, permissions).unwrap();

        assert_eq!(
            super::find_installed_python(&root, "3.14.8").unwrap(),
            Some(runtime)
        );
        assert_eq!(super::find_installed_python(&root, "3.14.9").unwrap(), None);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn renders_a_uv_project_from_veritas_dependencies() {
        let project = parse_python_project(
            "[python]\nversion = \"3.14.8\"\ndependencies = [\"requests==2.32.5\"]\n",
        )
        .unwrap()
        .unwrap();
        let rendered = render_pyproject(&project);
        let parsed: toml::Value = toml::from_str(&rendered).unwrap();
        assert_eq!(
            parsed["project"]["requires-python"].as_str(),
            Some("==3.14.8")
        );
        assert_eq!(
            parsed["project"]["dependencies"][0].as_str(),
            Some("requests==2.32.5")
        );
    }

    #[test]
    fn locked_environment_rejects_a_different_host_target() {
        let root =
            std::env::temp_dir().join(format!("veritas-target-lock-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let lock_path = root.join("veritas.lock");
        let uv_lock_path = root.join("uv.lock");
        fs::write(&uv_lock_path, "version = 1\n").unwrap();
        write_lock_metadata(
            &lock_path,
            "manifest-digest",
            &super::sha256_file(&uv_lock_path).unwrap(),
            "3.14.8",
            &super::pinned_uv_version().unwrap(),
        )
        .unwrap();

        let mut lock: toml::Value =
            toml::from_str(&fs::read_to_string(&lock_path).unwrap()).unwrap();
        let other_target = format!("{}-different", std::env::consts::ARCH);
        lock.as_table_mut()
            .unwrap()
            .insert("target".into(), toml::Value::String(other_target));
        fs::write(&lock_path, toml::to_string(&lock).unwrap()).unwrap();

        let result = verify_lock_metadata(&lock_path, "manifest-digest", &uv_lock_path, "3.14.8");
        assert!(
            result.is_err(),
            "a lock created for another host must not be accepted"
        );
        assert!(result.unwrap_err().contains("target"));
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn bundled_python_bridge_environment_rewrites_absolute_stage_links() {
        use std::os::unix::fs::symlink;

        let root = std::env::temp_dir().join(format!(
            "veritas bridge relocation Ω-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let package = root.join("package");
        let environment = package.join(".veritas/python-bridge/.venv");
        let binary_directory = environment.join("bin");
        let bundled_python = package.join("python/cpython/bin/python3.14");
        fs::create_dir_all(&binary_directory).unwrap();
        fs::create_dir_all(bundled_python.parent().unwrap()).unwrap();
        fs::write(&bundled_python, "python runtime").unwrap();
        let interpreter_link = binary_directory.join("python");
        symlink(&bundled_python, &interpreter_link).unwrap();

        normalize_bridge_environment_links(&package, &environment).unwrap();

        let target = fs::read_link(&interpreter_link).unwrap();
        assert!(!target.is_absolute(), "link should be relocatable");
        assert_eq!(
            fs::canonicalize(interpreter_link).unwrap(),
            fs::canonicalize(bundled_python).unwrap()
        );
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn repairs_case_mismatched_optional_terminfo_aliases_and_removes_dangling_ones() {
        use std::os::unix::fs::symlink;

        let root = std::env::temp_dir().join(format!(
            "veritas Python terminfo links-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let python = root.join("python");
        let terminfo = python.join("cpython/share/terminfo/u");
        fs::create_dir_all(&terminfo).unwrap();
        let terminfo_root = terminfo.parent().unwrap();
        fs::create_dir_all(terminfo_root.join("A")).unwrap();
        fs::write(terminfo_root.join("A/att7300"), b"terminfo").unwrap();
        let mixed_case = terminfo.join("unixpc");
        symlink("../a/att7300", &mixed_case).unwrap();
        let dangling = terminfo.join("missing");
        symlink("../A/not-present", &dangling).unwrap();

        normalize_python_install_links(&python).unwrap();

        assert_eq!(
            fs::read_link(&mixed_case).unwrap(),
            std::path::PathBuf::from("../A/att7300")
        );
        assert_eq!(fs::read(mixed_case).unwrap(), b"terminfo");
        assert!(fs::symlink_metadata(&dangling).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn rejects_dangling_bundled_python_links_outside_optional_terminfo_data() {
        use std::os::unix::fs::symlink;

        let root = std::env::temp_dir().join(format!(
            "veritas dangling Python link-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let python = root.join("python");
        let bin = python.join("cpython/bin");
        fs::create_dir_all(&bin).unwrap();
        let broken = bin.join("python3");
        symlink("missing-python", &broken).unwrap();

        let error = normalize_python_install_links(&python).unwrap_err();

        assert!(error.contains("dangling symlink outside optional terminfo data"));
        let _ = fs::remove_dir_all(root);
    }
}
