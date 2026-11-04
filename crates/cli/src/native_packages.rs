use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Component, Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

const LOCK_FORMAT_VERSION: i64 = 2;
const CACHE_DIRECTORY: &str = ".guion/packages";

fn project_manifest_path(root: impl AsRef<Path>) -> PathBuf {
    let root = root.as_ref();
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

fn project_lock_path(root: impl AsRef<Path>) -> PathBuf {
    let root = root.as_ref();
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

#[derive(Clone, Debug)]
struct LibraryManifest {
    name: String,
    version: String,
    entry: String,
    exports: Vec<String>,
    dependencies: BTreeMap<String, PackageDependency>,
    declared_sha256: Option<String>,
}

#[derive(Clone, Debug)]
struct PackageDependency {
    version: String,
    path: Option<String>,
    url: Option<String>,
    sha256: Option<String>,
}

#[derive(Clone, Debug)]
struct ResolvedPackage {
    manifest: LibraryManifest,
    root: PathBuf,
    digest: String,
    source_kind: String,
    source: String,
}

#[derive(Clone, Debug)]
struct LockState {
    native: BTreeMap<String, toml::Value>,
    python: Option<toml::Value>,
}

pub fn package_library(source: &Path, output: &Path) -> Result<String, String> {
    let source = fs::canonicalize(source).map_err(|error| {
        format!(
            "cannot resolve library source {}: {error}",
            source.display()
        )
    })?;
    let manifest = read_library_manifest(&source, None)?;
    let digest = content_sha256(&source)?;
    verify_declared_digest(&manifest, &digest)?;
    let parent = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .map_err(|error| format!("cannot create package parent {}: {error}", parent.display()))?;
    if fs::symlink_metadata(output).is_ok() {
        return Err(format!(
            "package output already exists: {}",
            output.display()
        ));
    }
    let stage = temporary_path(parent, "native-package")?;
    fs::create_dir(&stage)
        .map_err(|error| format!("cannot create package stage {}: {error}", stage.display()))?;
    let result = (|| {
        copy_library_tree(&source, &stage)?;
        set_manifest_digest(&stage.join("guion.toml"), &digest)?;
        fs::rename(&stage, output)
            .map_err(|error| format!("cannot publish package {}: {error}", output.display()))?;
        Ok(digest.clone())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&stage);
    }
    result
}

pub fn add_path(project: &Path, expected_name: &str, source: &Path) -> Result<String, String> {
    let source = fs::canonicalize(source).map_err(|error| {
        format!(
            "cannot resolve package source {}: {error}",
            source.display()
        )
    })?;
    install_package_graph(
        project,
        expected_name,
        &source,
        "path",
        &source.display().to_string(),
        None,
        false,
    )
}

pub fn add_url(
    project: &Path,
    expected_name: &str,
    url: &str,
    version: &str,
    archive_sha256: &str,
) -> Result<String, String> {
    add_url_internal(project, expected_name, url, version, archive_sha256, false)
}

fn add_url_internal(
    project: &Path,
    expected_name: &str,
    url: &str,
    version: &str,
    archive_sha256: &str,
    updating: bool,
) -> Result<String, String> {
    validate_package_name_for_add(expected_name, updating)?;
    validate_exact_version(version)?;
    validate_sha256(archive_sha256)?;
    let parsed =
        reqwest::Url::parse(url).map_err(|error| format!("invalid package URL: {error}"))?;
    if parsed.scheme() != "https"
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return Err("native package URLs must use HTTPS and cannot contain credentials".into());
    }
    let project = fs::canonicalize(project)
        .map_err(|error| format!("cannot resolve project {}: {error}", project.display()))?;
    let transaction = project.join(format!(
        ".guion/acquire-{}-{}",
        std::process::id(),
        nonce()?
    ));
    fs::create_dir_all(&transaction)
        .map_err(|error| format!("cannot create package acquisition stage: {error}"))?;
    let source = transaction.join("package");
    let result = (|| {
        let bytes = download_package_archive(url)?;
        let actual = format!("{:x}", Sha256::digest(&bytes));
        if actual != archive_sha256.to_ascii_lowercase() {
            return Err(format!(
                "package archive SHA-256 mismatch: expected {}, found {actual}",
                archive_sha256.to_ascii_lowercase()
            ));
        }
        extract_tar_safely(&bytes, &source)?;
        let package = read_library_manifest(&source, Some(expected_name))?;
        if package.version != version {
            return Err(format!(
                "archive package version {} does not match requested exact version {version}",
                package.version
            ));
        }
        let digest = content_sha256(&source)?;
        verify_declared_digest(&package, &digest)?;
        install_package_graph(
            &project,
            expected_name,
            &source,
            "url",
            url,
            Some(archive_sha256),
            updating,
        )
    })();
    let _ = fs::remove_dir_all(&transaction);
    result
}

fn download_package_archive(url: &str) -> Result<Vec<u8>, String> {
    const MAX_ARCHIVE_BYTES: usize = 128 * 1024 * 1024;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|error| format!("cannot configure package downloader: {error}"))?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("cannot start package downloader: {error}"))?;
    runtime.block_on(async move {
        let response = client
            .get(url)
            .send()
            .await
            .map_err(|error| format!("package download failed: {error}"))?;
        if !response.status().is_success() {
            return Err(format!(
                "package server returned HTTP {}",
                response.status()
            ));
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_ARCHIVE_BYTES as u64)
        {
            return Err("package archive exceeds the 128 MiB limit".into());
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|error| format!("cannot read package archive: {error}"))?;
        if bytes.len() > MAX_ARCHIVE_BYTES {
            return Err("package archive exceeds the 128 MiB limit".into());
        }
        Ok(bytes.to_vec())
    })
}

fn extract_tar_safely(bytes: &[u8], destination: &Path) -> Result<(), String> {
    const BLOCK: usize = 512;
    const MAX_EXPANDED_BYTES: u64 = 256 * 1024 * 1024;
    fs::create_dir_all(destination)
        .map_err(|error| format!("cannot create extraction directory: {error}"))?;
    let mut offset = 0usize;
    let mut seen = BTreeSet::new();
    let mut expanded = 0u64;
    let mut zero_blocks = 0;
    while offset + BLOCK <= bytes.len() {
        let header = &bytes[offset..offset + BLOCK];
        offset += BLOCK;
        if header.iter().all(|byte| *byte == 0) {
            zero_blocks += 1;
            if zero_blocks == 2 {
                break;
            }
            continue;
        }
        zero_blocks = 0;
        let expected_checksum = parse_tar_octal(&header[148..156])?;
        let actual_checksum: u64 = header
            .iter()
            .enumerate()
            .map(|(index, byte)| {
                if (148..156).contains(&index) {
                    32
                } else {
                    u64::from(*byte)
                }
            })
            .sum();
        if expected_checksum != actual_checksum {
            return Err("package archive has an invalid tar header checksum".into());
        }
        let name = tar_field(&header[0..100])?;
        let prefix = tar_field(&header[345..500])?;
        let path = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        validate_relative_path(&path)?;
        if !seen.insert(path.clone()) {
            return Err(format!("package archive repeats path `{path}`"));
        }
        let size = parse_tar_octal(&header[124..136])?;
        let kind = header[156];
        let path = destination.join(&path);
        if kind == b'5' {
            if size != 0 {
                return Err("package archive directory entries cannot contain data".into());
            }
            fs::create_dir_all(&path).map_err(|error| {
                format!(
                    "cannot create package directory {}: {error}",
                    path.display()
                )
            })?;
            continue;
        }
        if kind != 0 && kind != b'0' {
            return Err("package archive contains a link or unsupported tar entry".into());
        }
        if size > MAX_EXPANDED_BYTES || expanded.saturating_add(size) > MAX_EXPANDED_BYTES {
            return Err("expanded package archive exceeds the 256 MiB limit".into());
        }
        let size = usize::try_from(size).map_err(|_| "package file is too large")?;
        if offset.checked_add(size).is_none_or(|end| end > bytes.len()) {
            return Err("package archive is truncated".into());
        }
        if path.exists() {
            return Err(format!(
                "package archive file collides with a directory: {}",
                path.display()
            ));
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("cannot create package subdirectory: {error}"))?;
        }
        fs::write(&path, &bytes[offset..offset + size])
            .map_err(|error| format!("cannot extract package file {}: {error}", path.display()))?;
        expanded += size as u64;
        offset += size.div_ceil(512) * 512;
    }
    if zero_blocks < 2 {
        return Err("package archive has no complete tar terminator".into());
    }
    Ok(())
}

fn parse_tar_octal(field: &[u8]) -> Result<u64, String> {
    let digits = field
        .iter()
        .copied()
        .skip_while(|byte| *byte == b' ' || *byte == 0)
        .take_while(|byte| *byte != 0 && *byte != b' ')
        .collect::<Vec<_>>();
    if digits.is_empty() {
        return Ok(0);
    }
    digits.into_iter().try_fold(0u64, |value, digit| {
        if !(b'0'..=b'7').contains(&digit) {
            return Err("package archive has invalid tar numeric metadata".into());
        }
        value
            .checked_mul(8)
            .and_then(|value| value.checked_add(u64::from(digit - b'0')))
            .ok_or_else(|| "package archive tar metadata overflows".into())
    })
}

fn tar_field(field: &[u8]) -> Result<String, String> {
    let end = field
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(field.len());
    std::str::from_utf8(&field[..end])
        .map(str::to_owned)
        .map_err(|_| "package archive path is not UTF-8".into())
}

fn set_dependency_source(
    manifest_path: &Path,
    name: &str,
    version: &str,
    kind: &str,
    source: &str,
    sha256: Option<&str>,
) -> Result<(), String> {
    let text = fs::read_to_string(manifest_path)
        .map_err(|error| format!("cannot read {}: {error}", manifest_path.display()))?;
    let mut manifest: toml::Value = toml::from_str(&text)
        .map_err(|error| format!("{}: invalid TOML: {error}", manifest_path.display()))?;
    let dependencies = manifest
        .as_table_mut()
        .unwrap()
        .entry("dependencies")
        .or_insert_with(|| toml::Value::Table(toml::map::Map::new()))
        .as_table_mut()
        .ok_or("[dependencies] must be a table")?;
    let mut requirement = toml::map::Map::new();
    requirement.insert("version".into(), toml::Value::String(version.into()));
    requirement.insert(kind.into(), toml::Value::String(source.into()));
    if let Some(sha256) = sha256 {
        requirement.insert("sha256".into(), toml::Value::String(sha256.into()));
    }
    dependencies.insert(name.into(), toml::Value::Table(requirement));
    let serialized = toml::to_string_pretty(&manifest)
        .map_err(|error| format!("cannot serialize project manifest: {error}"))?;
    atomic_write(manifest_path, serialized.as_bytes())
}

fn set_lock_source(lock_path: &Path, name: &str, kind: &str, source: &str) -> Result<(), String> {
    let project = lock_path.parent().ok_or("lock has no parent directory")?;
    let mut lock = read_lock(project)?;
    let package = lock
        .native
        .get_mut(name)
        .ok_or_else(|| format!("package `{name}` is missing from lock"))?;
    let package = package
        .as_table_mut()
        .ok_or_else(|| format!("package `{name}` lock record is not a table"))?;
    package.insert("source_kind".into(), toml::Value::String(kind.into()));
    package.insert("source".into(), toml::Value::String(source.into()));
    let serialized = serialize_lock(&lock)?;
    atomic_write(lock_path, serialized.as_bytes())
}

fn install_package_graph(
    project: &Path,
    expected_name: &str,
    source_root: &Path,
    source_kind: &str,
    source_ref: &str,
    source_sha256: Option<&str>,
    updating: bool,
) -> Result<String, String> {
    validate_package_name_for_add(expected_name, updating)?;
    let project = fs::canonicalize(project)
        .map_err(|error| format!("cannot resolve project {}: {error}", project.display()))?;
    let source_root = fs::canonicalize(source_root).map_err(|error| {
        format!(
            "cannot resolve package source {}: {error}",
            source_root.display()
        )
    })?;
    let stage = project.join(format!(".guion/graph-{}-{}", std::process::id(), nonce()?));
    fs::create_dir_all(&stage)
        .map_err(|error| format!("cannot create dependency resolution stage: {error}"))?;
    let mut visiting = BTreeSet::new();
    let mut resolved = BTreeMap::new();
    let mut ordered = Vec::new();
    let archive_boundary = (source_kind == "url").then_some(source_root.as_path());
    let graph_result = resolve_package_graph(
        &source_root,
        expected_name,
        None,
        source_kind,
        source_ref,
        source_sha256,
        archive_boundary,
        &stage,
        &mut visiting,
        &mut resolved,
        &mut ordered,
    );
    if let Err(error) = graph_result {
        let _ = fs::remove_dir_all(&stage);
        return Err(error);
    }

    let manifest_path = project_manifest_path(&project);
    let lock_path = project_lock_path(&project);
    let old_manifest = fs::read(&manifest_path)
        .map_err(|error| format!("cannot preserve project manifest: {error}"))?;
    let old_lock = fs::read(&lock_path).ok();
    let root_package = resolved
        .get(expected_name)
        .ok_or_else(|| format!("resolved package graph lost root `{expected_name}`"))?;
    let root_version = root_package.manifest.version.clone();
    let mut new_caches = Vec::new();
    let mut backed_up_caches = Vec::new();
    let mut seen_caches = BTreeSet::new();
    let install_result: Result<(), String> = (|| {
        for name in &ordered {
            let package = &resolved[name];
            let cache = project
                .join(CACHE_DIRECTORY)
                .join(&package.manifest.name)
                .join(&package.manifest.version);
            if !seen_caches.insert(cache.clone()) {
                continue;
            }
            if cache.exists() {
                if updating {
                    let backup = stage
                        .join("previous-cache")
                        .join(&package.manifest.name)
                        .join(&package.manifest.version);
                    fs::create_dir_all(backup.parent().unwrap()).map_err(|error| {
                        format!("cannot create package cache rollback stage: {error}")
                    })?;
                    fs::rename(&cache, &backup).map_err(|error| {
                        format!(
                            "cannot stage old package cache {}: {error}",
                            cache.display()
                        )
                    })?;
                    backed_up_caches.push((backup, cache.clone()));
                } else {
                    verify_cached_library(
                        &cache,
                        &package.manifest.name,
                        &package.manifest.version,
                        &package.digest,
                    )?;
                }
            } else {
                new_caches.push(cache);
            }
        }
        for name in &ordered {
            let package = &resolved[name];
            add_path_internal(&project, &package.manifest.name, &package.root, updating)?;
            if package.source_kind == "url" {
                set_lock_source(&lock_path, &package.manifest.name, "url", &package.source)?;
            }
        }
        atomic_write(&manifest_path, &old_manifest)?;
        set_dependency_source(
            &manifest_path,
            expected_name,
            &root_version,
            source_kind,
            source_ref,
            source_sha256,
        )?;
        Ok(())
    })();
    if let Err(error) = install_result {
        let _ = atomic_write(&manifest_path, &old_manifest);
        match old_lock {
            Some(contents) => {
                let _ = atomic_write(&lock_path, &contents);
            }
            None => {
                let _ = fs::remove_file(&lock_path);
            }
        }
        for cache in &new_caches {
            let _ = fs::remove_dir_all(cache);
        }
        for (_, cache) in &backed_up_caches {
            let _ = fs::remove_dir_all(cache);
        }
        for (backup, cache) in backed_up_caches.iter().rev() {
            if let Some(parent) = cache.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let _ = fs::rename(backup, cache);
        }
        let _ = fs::remove_dir_all(&stage);
        return Err(format!("native package transaction rolled back: {error}"));
    }
    let _ = fs::remove_dir_all(&stage);
    Ok(format!(
        "added {} {} ({})",
        expected_name, root_version, root_package.digest
    ))
}

fn resolve_package_dependency_path(
    package_root: &Path,
    requested_path: &str,
    archive_boundary: Option<&Path>,
) -> Result<PathBuf, String> {
    let relative = Path::new(requested_path);
    if requested_path.is_empty() || relative.is_absolute() {
        return Err(format!(
            "dependency path must be relative to the package: `{requested_path}`"
        ));
    }
    let resolved = fs::canonicalize(package_root.join(relative)).map_err(|error| {
        format!(
            "cannot resolve `{requested_path}` from {}: {error}",
            package_root.display()
        )
    })?;
    if let Some(boundary) = archive_boundary {
        let boundary = fs::canonicalize(boundary).map_err(|error| {
            format!(
                "cannot resolve extracted package boundary {}: {error}",
                boundary.display()
            )
        })?;
        if !resolved.starts_with(&boundary) {
            return Err(format!(
                "URL package dependency `{requested_path}` escapes its extracted archive"
            ));
        }
    }
    Ok(resolved)
}

#[allow(clippy::too_many_arguments)]
fn resolve_package_graph(
    root: &Path,
    expected_name: &str,
    expected_version: Option<&str>,
    source_kind: &str,
    source_ref: &str,
    source_sha256: Option<&str>,
    archive_boundary: Option<&Path>,
    stage: &Path,
    visiting: &mut BTreeSet<String>,
    resolved: &mut BTreeMap<String, ResolvedPackage>,
    ordered: &mut Vec<String>,
) -> Result<(), String> {
    let root = fs::canonicalize(root)
        .map_err(|error| format!("cannot resolve native dependency `{expected_name}`: {error}"))?;
    let manifest = read_library_manifest(&root, Some(expected_name))?;
    if expected_version.is_some_and(|expected| expected != manifest.version) {
        return Err(format!(
            "dependency `{expected_name}` resolved to {}, expected exact version {}",
            manifest.version,
            expected_version.unwrap()
        ));
    }
    let digest = content_sha256(&root)?;
    verify_declared_digest(&manifest, &digest)?;
    if visiting.contains(expected_name) {
        return Err(format!(
            "native package dependency cycle contains `{expected_name}`"
        ));
    }
    if let Some(existing) = resolved.get(expected_name) {
        if existing.manifest.version != manifest.version || existing.digest != digest {
            return Err(format!("native package graph requires conflicting versions or content for `{expected_name}`"));
        }
        return Ok(());
    }
    visiting.insert(expected_name.to_owned());
    for (dependency_name, dependency) in &manifest.dependencies {
        let (dependency_root, dependency_kind, dependency_ref, dependency_archive_boundary) =
            if let Some(path) = &dependency.path {
                let resolved_path = resolve_package_dependency_path(&root, path, archive_boundary)
                .map_err(|error| {
                    format!("cannot resolve path for native dependency `{dependency_name}`: {error}")
                })?;
                (
                    resolved_path.clone(),
                    "path".to_owned(),
                    resolved_path.display().to_string(),
                    archive_boundary.map(Path::to_path_buf),
                )
            } else {
                let url = dependency
                    .url
                    .as_deref()
                    .ok_or_else(|| format!("dependency `{dependency_name}` has no source"))?;
                let parsed = reqwest::Url::parse(url).map_err(|error| {
                    format!("invalid URL for native dependency `{dependency_name}`: {error}")
                })?;
                if parsed.scheme() != "https"
                    || parsed.host_str().is_none()
                    || !parsed.username().is_empty()
                    || parsed.password().is_some()
                {
                    return Err(format!("URL dependency `{dependency_name}` must use HTTPS and cannot contain credentials"));
                }
                let expected_hash = dependency
                    .sha256
                    .as_deref()
                    .ok_or_else(|| format!("URL dependency `{dependency_name}` has no SHA-256"))?;
                let archive = download_package_archive(url)?;
                let actual = format!("{:x}", Sha256::digest(&archive));
                if actual != expected_hash.to_ascii_lowercase() {
                    return Err(format!(
                        "archive SHA-256 mismatch for dependency `{dependency_name}`"
                    ));
                }
                let extracted = stage
                    .join("downloads")
                    .join(dependency_name)
                    .join(&dependency.version);
                if extracted.exists() {
                    fs::remove_dir_all(&extracted).map_err(|error| {
                        format!("cannot reset dependency extraction stage: {error}")
                    })?;
                }
                extract_tar_safely(&archive, &extracted)?;
                (
                    extracted.clone(),
                    "url".to_owned(),
                    url.to_owned(),
                    Some(extracted),
                )
            };
        resolve_package_graph(
            &dependency_root,
            dependency_name,
            Some(&dependency.version),
            &dependency_kind,
            &dependency_ref,
            dependency.sha256.as_deref(),
            dependency_archive_boundary.as_deref(),
            stage,
            visiting,
            resolved,
            ordered,
        )?;
    }
    visiting.remove(expected_name);
    resolved.insert(
        expected_name.to_owned(),
        ResolvedPackage {
            manifest,
            root,
            digest,
            source_kind: source_kind.to_owned(),
            source: source_ref.to_owned(),
        },
    );
    ordered.push(expected_name.to_owned());
    let _ = source_sha256;
    Ok(())
}

fn add_path_internal(
    project: &Path,
    expected_name: &str,
    source: &Path,
    updating: bool,
) -> Result<String, String> {
    validate_package_name_for_add(expected_name, updating)?;
    let project = fs::canonicalize(project)
        .map_err(|error| format!("cannot resolve project {}: {error}", project.display()))?;
    let source = fs::canonicalize(source).map_err(|error| {
        format!(
            "cannot resolve package source {}: {error}",
            source.display()
        )
    })?;
    let manifest = read_library_manifest(&source, Some(expected_name))?;
    let digest = content_sha256(&source)?;
    verify_declared_digest(&manifest, &digest)?;
    let cache = project
        .join(CACHE_DIRECTORY)
        .join(&manifest.name)
        .join(&manifest.version);
    if cache.exists() {
        verify_cached_library(&cache, &manifest.name, &manifest.version, &digest)?;
    }

    let mut lock = read_lock(&project)?;
    if let Some(existing) = lock.native.get(&manifest.name) {
        let existing_version = existing.get("version").and_then(toml::Value::as_str);
        let existing_digest = existing.get("sha256").and_then(toml::Value::as_str);
        if !updating
            && (existing_version != Some(manifest.version.as_str())
                || existing_digest != Some(digest.as_str()))
        {
            return Err(format!(
                "package `{}` conflicts with the locked exact version or content identity",
                manifest.name
            ));
        }
    }
    let package_lock = locked_library_value(&manifest, &digest, &source);
    lock.native.insert(manifest.name.clone(), package_lock);

    let manifest_path = project_manifest_path(&project);
    let manifest_text = fs::read_to_string(&manifest_path)
        .map_err(|error| format!("cannot read {}: {error}", manifest_path.display()))?;
    let project_manifest: toml::Value = toml::from_str(&manifest_text)
        .map_err(|error| format!("{}: invalid TOML: {error}", manifest_path.display()))?;
    if project_manifest
        .get("project")
        .and_then(toml::Value::as_table)
        .is_none()
    {
        return Err(format!(
            "{} has no [project] table",
            manifest_path.display()
        ));
    }
    let mut project_manifest = project_manifest;
    let dependencies = project_manifest
        .as_table_mut()
        .unwrap()
        .entry("dependencies")
        .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
    let dependencies = dependencies
        .as_table_mut()
        .ok_or("[dependencies] must be a table")?;
    let mut requirement = toml::map::Map::new();
    requirement.insert(
        "version".into(),
        toml::Value::String(manifest.version.clone()),
    );
    requirement.insert(
        "path".into(),
        toml::Value::String(source.display().to_string()),
    );
    dependencies.insert(manifest.name.clone(), toml::Value::Table(requirement));

    let staged_cache_root = project.join(format!(
        ".guion/native-stage-{}-{}",
        std::process::id(),
        nonce()?
    ));
    fs::create_dir_all(&staged_cache_root).map_err(|error| {
        format!(
            "cannot create package cache stage {}: {error}",
            staged_cache_root.display()
        )
    })?;
    let staged_package = staged_cache_root
        .join(&manifest.name)
        .join(&manifest.version);
    let result = (|| {
        if !cache.exists() {
            fs::create_dir_all(staged_package.parent().unwrap())
                .map_err(|error| format!("cannot create staged package directory: {error}"))?;
            copy_library_tree(&source, &staged_package)?;
        }
        let new_manifest = toml::to_string_pretty(&project_manifest)
            .map_err(|error| format!("cannot serialize project manifest: {error}"))?;
        let new_lock = serialize_lock(&lock)?;
        let cache_parent = cache.parent().unwrap();
        fs::create_dir_all(cache_parent).map_err(|error| {
            format!(
                "cannot create package cache {}: {error}",
                cache_parent.display()
            )
        })?;
        let installed = if cache.exists() {
            false
        } else {
            fs::rename(&staged_package, &cache).map_err(|error| {
                format!(
                    "cannot install package in managed cache {}: {error}",
                    cache.display()
                )
            })?;
            true
        };
        let old_manifest = fs::read(&manifest_path)
            .map_err(|error| format!("cannot preserve project manifest: {error}"))?;
        let old_lock = fs::read(project_lock_path(&project)).ok();
        let commit = atomic_write(&manifest_path, new_manifest.as_bytes())
            .and_then(|_| atomic_write(&project_lock_path(&project), new_lock.as_bytes()));
        if let Err(error) = commit {
            let _ = atomic_write(&manifest_path, &old_manifest);
            match old_lock {
                Some(contents) => {
                    let _ = atomic_write(&project_lock_path(&project), &contents);
                }
                None => {
                    let _ = fs::remove_file(project_lock_path(&project));
                }
            }
            if installed {
                let _ = fs::remove_dir_all(&cache);
            }
            return Err(error);
        }
        Ok(())
    })();
    let _ = fs::remove_dir_all(&staged_cache_root);
    result?;
    Ok(format!(
        "added {} {} ({digest})",
        manifest.name, manifest.version
    ))
}

pub fn update_paths(project: &Path) -> Result<String, String> {
    let project = fs::canonicalize(project)
        .map_err(|error| format!("cannot resolve project {}: {error}", project.display()))?;
    let manifest_path = project_manifest_path(&project);
    let manifest_text = fs::read_to_string(&manifest_path)
        .map_err(|error| format!("cannot read {}: {error}", manifest_path.display()))?;
    let manifest: toml::Value = toml::from_str(&manifest_text)
        .map_err(|error| format!("{}: invalid TOML: {error}", manifest_path.display()))?;
    let mut sources = Vec::new();
    if let Some(dependencies) = manifest.get("dependencies").and_then(toml::Value::as_table) {
        for (name, dependency) in dependencies {
            let version = requirement_version(dependency)?;
            let path = dependency.get("path").and_then(toml::Value::as_str);
            let url = dependency.get("url").and_then(toml::Value::as_str);
            match (path, url) {
                (Some(path), None) => {
                    sources.push((name.clone(), version, Some(PathBuf::from(path)), None))
                }
                (None, Some(url)) => {
                    let sha256 = dependency
                        .get("sha256")
                        .and_then(toml::Value::as_str)
                        .ok_or_else(|| format!("URL dependency `{name}` requires `sha256`"))?;
                    validate_sha256(sha256)?;
                    sources.push((
                        name.clone(),
                        version,
                        None,
                        Some((url.to_owned(), sha256.to_owned())),
                    ));
                }
                _ => {
                    return Err(format!(
                        "dependency `{name}` must declare exactly one of `path` or `url`"
                    ))
                }
            }
        }
    }
    if sources.is_empty() {
        return Err("no native dependencies are configured for update".into());
    }
    let backup_root = project.join(format!(
        ".guion/update-backup-{}-{}",
        std::process::id(),
        nonce()?
    ));
    fs::create_dir_all(&backup_root)
        .map_err(|error| format!("cannot create update transaction: {error}"))?;
    let old_manifest = fs::read(&manifest_path)
        .map_err(|error| format!("cannot preserve project manifest: {error}"))?;
    let lock_path = project_lock_path(&project);
    let old_lock = fs::read(&lock_path).ok();
    let package_cache = project.join(CACHE_DIRECTORY);
    let cache_snapshot = backup_root.join("packages");
    let had_package_cache = package_cache.is_dir();
    if had_package_cache {
        if let Err(error) = copy_library_tree(&package_cache, &cache_snapshot) {
            let _ = fs::remove_dir_all(&backup_root);
            return Err(format!(
                "cannot stage package cache for update rollback: {error}"
            ));
        }
    }
    let update_result: Result<Vec<String>, String> = (|| {
        let mut updated = Vec::new();
        for (name, version, path, url) in &sources {
            let result = if let Some(source) = path {
                let source = fs::canonicalize(source).map_err(|error| {
                    format!("cannot resolve update source {}: {error}", source.display())
                })?;
                install_package_graph(
                    &project,
                    name,
                    &source,
                    "path",
                    &source.display().to_string(),
                    None,
                    true,
                )?
            } else if let Some((url, sha256)) = url {
                add_url_internal(&project, name, url, version, sha256, true)?
            } else {
                return Err(format!("dependency `{name}` has no update source"));
            };
            updated.push(result);
        }
        Ok(updated)
    })();
    let updated = match update_result {
        Ok(updated) => updated,
        Err(error) => {
            let _ = atomic_write(&manifest_path, &old_manifest);
            match old_lock {
                Some(contents) => {
                    let _ = atomic_write(&lock_path, &contents);
                }
                None => {
                    let _ = fs::remove_file(&lock_path);
                }
            }
            let _ = fs::remove_dir_all(&package_cache);
            if had_package_cache {
                if let Some(parent) = package_cache.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                let _ = fs::rename(&cache_snapshot, &package_cache);
            }
            let _ = fs::remove_dir_all(&backup_root);
            return Err(format!("native package update rolled back: {error}"));
        }
    };
    let _ = fs::remove_dir_all(&backup_root);
    Ok(format!(
        "updated {} native package(s): {}",
        updated.len(),
        updated.join(", ")
    ))
}

pub fn load_locked_roots(project: &Path) -> Result<BTreeMap<String, PathBuf>, String> {
    let project = fs::canonicalize(project)
        .map_err(|error| format!("cannot resolve project {}: {error}", project.display()))?;
    let lock = read_lock(&project)?;
    let manifest_path = project_manifest_path(&project);
    let manifest: toml::Value = toml::from_str(
        &fs::read_to_string(&manifest_path)
            .map_err(|error| format!("cannot read {}: {error}", manifest_path.display()))?,
    )
    .map_err(|error| format!("{}: invalid TOML: {error}", manifest_path.display()))?;
    let direct = manifest.get("dependencies").and_then(toml::Value::as_table);
    if direct.is_some_and(|dependencies| !dependencies.is_empty()) && lock.native.is_empty() {
        return Err("native dependencies are declared but guion.lock has no native package graph; run `guion sync --locked --offline` after acquisition".into());
    }
    if let Some(direct) = direct {
        for (name, requirement) in direct {
            let version = requirement_version(requirement)?;
            let Some(locked) = lock.native.get(name) else {
                return Err(format!(
                    "native dependency `{name}` is absent from guion.lock"
                ));
            };
            if locked.get("version").and_then(toml::Value::as_str) != Some(version.as_str()) {
                return Err(format!(
                    "native dependency `{name}` version does not match guion.lock"
                ));
            }
        }
    }
    let native = lock.native;
    let mut roots = BTreeMap::new();
    for (name, package) in &native {
        let version = package
            .get("version")
            .and_then(toml::Value::as_str)
            .ok_or_else(|| format!("locked package `{name}` has no version"))?;
        let digest = package
            .get("sha256")
            .and_then(toml::Value::as_str)
            .ok_or_else(|| format!("locked package `{name}` has no SHA-256 identity"))?;
        validate_package_name(name)?;
        validate_exact_version(version)?;
        let cache = project.join(CACHE_DIRECTORY).join(name).join(version);
        verify_cached_library(&cache, name, version, digest)?;
        let manifest = read_library_manifest(&cache, Some(name))?;
        let locked_dependencies = package
            .get("dependencies")
            .and_then(toml::Value::as_table)
            .ok_or_else(|| format!("locked package `{name}` has no dependency table"))?;
        if locked_dependencies.len() != manifest.dependencies.len() {
            return Err(format!(
                "locked package `{name}` dependency list does not match its manifest"
            ));
        }
        for (dependency_name, dependency) in &manifest.dependencies {
            let Some(locked_dependency) = locked_dependencies.get(dependency_name) else {
                return Err(format!(
                    "locked package `{name}` omits dependency `{dependency_name}`"
                ));
            };
            if locked_dependency
                .get("version")
                .and_then(toml::Value::as_str)
                != Some(dependency.version.as_str())
            {
                return Err(format!("locked package `{name}` dependency `{dependency_name}` version does not match its manifest"));
            }
            if native
                .get(dependency_name)
                .and_then(|record| record.get("version"))
                .and_then(toml::Value::as_str)
                != Some(dependency.version.as_str())
            {
                return Err(format!("locked package `{name}` requires missing or conflicting dependency `{dependency_name}` {}", dependency.version));
            }
        }
        roots.insert(name.clone(), cache);
    }
    verify_lock_has_no_cycles(&native)?;
    Ok(roots)
}

fn verify_lock_has_no_cycles(native: &BTreeMap<String, toml::Value>) -> Result<(), String> {
    fn visit(
        name: &str,
        native: &BTreeMap<String, toml::Value>,
        visiting: &mut BTreeSet<String>,
        visited: &mut BTreeSet<String>,
    ) -> Result<(), String> {
        if visited.contains(name) {
            return Ok(());
        }
        if !visiting.insert(name.to_owned()) {
            return Err(format!(
                "native package lock contains a dependency cycle through `{name}`"
            ));
        }
        if let Some(dependencies) = native
            .get(name)
            .and_then(|package| package.get("dependencies"))
            .and_then(toml::Value::as_table)
        {
            for dependency in dependencies.keys() {
                visit(dependency, native, visiting, visited)?;
            }
        }
        visiting.remove(name);
        visited.insert(name.to_owned());
        Ok(())
    }
    let mut visiting = BTreeSet::new();
    let mut visited = BTreeSet::new();
    for name in native.keys() {
        visit(name, native, &mut visiting, &mut visited)?;
    }
    Ok(())
}

pub fn sync_locked_offline(project: &Path) -> Result<String, String> {
    let project = fs::canonicalize(project)
        .map_err(|error| format!("cannot resolve project {}: {error}", project.display()))?;
    let roots = load_locked_roots(&project)?;
    let lock_path = project_lock_path(&project);
    let previous_format = if lock_path.is_file() {
        let text = fs::read_to_string(&lock_path)
            .map_err(|error| format!("cannot read {}: {error}", lock_path.display()))?;
        let value: toml::Value = toml::from_str(&text)
            .map_err(|error| format!("{}: invalid TOML: {error}", lock_path.display()))?;
        value
            .get("format_version")
            .and_then(toml::Value::as_integer)
    } else {
        None
    };
    let lock = read_lock(&project)?;
    let serialized = serialize_lock(&lock)?;
    atomic_write(&lock_path, serialized.as_bytes())?;
    let migration = if previous_format == Some(1) {
        "; migrated Python-only lock to format 2"
    } else if previous_format.is_none() {
        "; wrote format 2 lock"
    } else {
        ""
    };
    Ok(format!(
        "verified {} locked native package(s) offline{migration}",
        roots.len()
    ))
}

fn read_library_manifest(
    root: &Path,
    expected_name: Option<&str>,
) -> Result<LibraryManifest, String> {
    if !root.is_dir() {
        return Err(format!(
            "native package source is not a directory: {}",
            root.display()
        ));
    }
    let path = project_manifest_path(root);
    let text = fs::read_to_string(&path).map_err(|error| {
        format!(
            "cannot read native package manifest {}: {error}",
            path.display()
        )
    })?;
    let value: toml::Value = toml::from_str(&text)
        .map_err(|error| format!("{}: invalid TOML: {error}", path.display()))?;
    let package = value
        .get("package")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| format!("{} has no [package] table", path.display()))?;
    let name = package
        .get("name")
        .and_then(toml::Value::as_str)
        .ok_or_else(|| format!("{}: [package].name must be text", path.display()))?
        .to_owned();
    validate_package_name(&name)?;
    if expected_name.is_some_and(|expected| expected != name) {
        return Err(format!(
            "package name `{name}` does not match requested name `{}`",
            expected_name.unwrap()
        ));
    }
    let version = package
        .get("version")
        .and_then(toml::Value::as_str)
        .ok_or_else(|| format!("{}: [package].version must be text", path.display()))?
        .to_owned();
    validate_exact_version(&version)?;
    let entry = package
        .get("entry")
        .and_then(toml::Value::as_str)
        .ok_or_else(|| format!("{}: [package].entry must be text", path.display()))?
        .to_owned();
    validate_relative_path(&entry)?;
    if !root.join(&entry).is_file() {
        return Err(format!(
            "{}: package entry does not exist: {entry}",
            path.display()
        ));
    }
    let exports = package
        .get("exports")
        .and_then(toml::Value::as_array)
        .ok_or_else(|| {
            format!(
                "{}: [package].exports must be an array of relative module paths",
                path.display()
            )
        })?
        .iter()
        .map(|value| {
            let module_path = value.as_str().ok_or_else(|| {
                format!(
                    "{}: package exports must contain only text paths",
                    path.display()
                )
            })?;
            validate_relative_path(module_path)?;
            if !root.join(module_path).is_file() {
                return Err(format!(
                    "{}: exported module does not exist: {module_path}",
                    path.display()
                ));
            }
            Ok(module_path.to_owned())
        })
        .collect::<Result<Vec<_>, String>>()?;
    if exports.is_empty() {
        return Err(format!(
            "{}: package exports must not be empty",
            path.display()
        ));
    }
    let declared_sha256 = package
        .get("sha256")
        .and_then(toml::Value::as_str)
        .map(str::to_owned);
    let mut dependencies = BTreeMap::new();
    if let Some(table) = value.get("dependencies") {
        let table = table
            .as_table()
            .ok_or_else(|| format!("{}: [dependencies] must be a table", path.display()))?;
        for (dependency_name, requirement) in table {
            validate_package_name(dependency_name)?;
            let version = requirement_version(requirement)?;
            let path_source = requirement
                .get("path")
                .and_then(toml::Value::as_str)
                .map(str::to_owned);
            let url_source = requirement
                .get("url")
                .and_then(toml::Value::as_str)
                .map(str::to_owned);
            if path_source.is_some() == url_source.is_some() {
                return Err(format!("{}: dependency `{dependency_name}` must declare exactly one of `path` or `url`", path.display()));
            }
            if let Some(path_source) = &path_source {
                if path_source.is_empty() || Path::new(path_source).is_absolute() {
                    return Err(format!(
                        "{}: dependency `{dependency_name}` path must be relative to the package",
                        path.display()
                    ));
                }
            }
            let sha256 = requirement
                .get("sha256")
                .and_then(toml::Value::as_str)
                .map(str::to_owned);
            if url_source.is_some() {
                let hash = sha256.as_deref().ok_or_else(|| {
                    format!(
                        "{}: URL dependency `{dependency_name}` requires `sha256`",
                        path.display()
                    )
                })?;
                validate_sha256(hash)?;
            }
            dependencies.insert(
                dependency_name.clone(),
                PackageDependency {
                    version,
                    path: path_source,
                    url: url_source,
                    sha256,
                },
            );
        }
    }
    Ok(LibraryManifest {
        name,
        version,
        entry,
        exports,
        dependencies,
        declared_sha256,
    })
}

fn locked_library_value(manifest: &LibraryManifest, digest: &str, source: &Path) -> toml::Value {
    let mut value = toml::map::Map::new();
    value.insert(
        "version".into(),
        toml::Value::String(manifest.version.clone()),
    );
    value.insert("entry".into(), toml::Value::String(manifest.entry.clone()));
    value.insert(
        "exports".into(),
        toml::Value::Array(
            manifest
                .exports
                .iter()
                .cloned()
                .map(toml::Value::String)
                .collect(),
        ),
    );
    value.insert("sha256".into(), toml::Value::String(digest.into()));
    let dependencies = manifest
        .dependencies
        .iter()
        .map(|(name, requirement)| {
            let mut item = toml::map::Map::new();
            item.insert(
                "version".into(),
                toml::Value::String(requirement.version.clone()),
            );
            (name.clone(), toml::Value::Table(item))
        })
        .collect();
    value.insert("dependencies".into(), toml::Value::Table(dependencies));
    value.insert("source_kind".into(), toml::Value::String("path".into()));
    value.insert(
        "source".into(),
        toml::Value::String(source.display().to_string()),
    );
    toml::Value::Table(value)
}

fn read_lock(project: &Path) -> Result<LockState, String> {
    let path = project_lock_path(project);
    if !path.is_file() {
        return Ok(LockState {
            native: BTreeMap::new(),
            python: None,
        });
    }
    let text = fs::read_to_string(&path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let value: toml::Value = toml::from_str(&text)
        .map_err(|error| format!("{}: invalid TOML: {error}", path.display()))?;
    match value
        .get("format_version")
        .and_then(toml::Value::as_integer)
    {
        Some(1) => {
            let mut python = value.as_table().cloned().unwrap_or_default();
            python.remove("format_version");
            Ok(LockState {
                native: BTreeMap::new(),
                python: if python.is_empty() {
                    None
                } else {
                    Some(toml::Value::Table(python))
                },
            })
        }
        Some(LOCK_FORMAT_VERSION) => {
            let native = value
                .get("native")
                .and_then(toml::Value::as_table)
                .map(|table| {
                    table
                        .iter()
                        .map(|(name, value)| (name.clone(), value.clone()))
                        .collect()
                })
                .unwrap_or_default();
            let python = value.get("python").cloned();
            Ok(LockState { native, python })
        }
        Some(other) => Err(format!(
            "{}: unsupported lock format version {other}",
            path.display()
        )),
        None => Err(format!(
            "{}: lock file has no integer format_version",
            path.display()
        )),
    }
}

fn serialize_lock(lock: &LockState) -> Result<String, String> {
    let mut root = toml::map::Map::new();
    root.insert(
        "format_version".into(),
        toml::Value::Integer(LOCK_FORMAT_VERSION),
    );
    let mut native = toml::map::Map::new();
    for (name, package) in &lock.native {
        native.insert(name.clone(), package.clone());
    }
    root.insert("native".into(), toml::Value::Table(native));
    if let Some(python) = &lock.python {
        root.insert("python".into(), python.clone());
    }
    toml::to_string_pretty(&toml::Value::Table(root))
        .map_err(|error| format!("cannot serialize guion.lock: {error}"))
}

fn verify_cached_library(
    root: &Path,
    name: &str,
    version: &str,
    expected: &str,
) -> Result<(), String> {
    let manifest = read_library_manifest(root, Some(name))?;
    if manifest.version != version {
        return Err(format!(
            "cached package `{name}` has version {}, expected {version}",
            manifest.version
        ));
    }
    let actual = content_sha256(root)?;
    if actual != expected {
        return Err(format!(
            "cached package `{name}` content hash mismatch: expected {expected}, found {actual}"
        ));
    }
    verify_declared_digest(&manifest, &actual)
}

fn verify_declared_digest(manifest: &LibraryManifest, actual: &str) -> Result<(), String> {
    if let Some(expected) = &manifest.declared_sha256 {
        if expected != actual {
            return Err(format!(
                "package `{}` declares SHA-256 {expected}, but its content hashes to {actual}",
                manifest.name
            ));
        }
    }
    Ok(())
}

fn content_sha256(root: &Path) -> Result<String, String> {
    let manifest_path = project_manifest_path(root);
    let text = fs::read_to_string(&manifest_path).map_err(|error| {
        format!(
            "cannot read native package manifest {}: {error}",
            manifest_path.display()
        )
    })?;
    let mut manifest: toml::Value = toml::from_str(&text)
        .map_err(|error| format!("{}: invalid TOML: {error}", manifest_path.display()))?;
    manifest
        .get_mut("package")
        .and_then(toml::Value::as_table_mut)
        .ok_or_else(|| format!("{} has no [package] table", manifest_path.display()))?
        .remove("sha256");
    let canonical_manifest = toml::to_string(&manifest)
        .map_err(|error| format!("cannot canonicalize native package manifest: {error}"))?;
    let mut files = Vec::new();
    collect_files(root, root, &mut files)?;
    files.sort();
    let mut digest = Sha256::new();
    digest.update(b"guion-native-package-v1\0");
    digest.update((canonical_manifest.len() as u64).to_le_bytes());
    digest.update(canonical_manifest.as_bytes());
    for relative in files {
        if relative == Path::new("guion.toml") {
            continue;
        }
        let bytes = fs::read(root.join(&relative)).map_err(|error| {
            format!(
                "cannot read native package file {}: {error}",
                root.join(&relative).display()
            )
        })?;
        let name = relative.to_string_lossy().replace('\\', "/");
        digest.update((name.len() as u64).to_le_bytes());
        digest.update(name.as_bytes());
        digest.update((bytes.len() as u64).to_le_bytes());
        digest.update(bytes);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn collect_files(root: &Path, directory: &Path, output: &mut Vec<PathBuf>) -> Result<(), String> {
    let mut entries = fs::read_dir(directory)
        .map_err(|error| {
            format!(
                "cannot inspect package directory {}: {error}",
                directory.display()
            )
        })?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| {
            format!(
                "cannot inspect package directory {}: {error}",
                directory.display()
            )
        })?;
    entries.sort();
    for path in entries {
        let relative = path.strip_prefix(root).unwrap();
        if excluded_path(relative) {
            continue;
        }
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| format!("cannot inspect package entry {}: {error}", path.display()))?;
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "native package cannot contain symbolic links: {}",
                relative.display()
            ));
        }
        if metadata.is_dir() {
            collect_files(root, &path, output)?;
        } else if metadata.is_file() {
            output.push(relative.to_path_buf());
        }
    }
    Ok(())
}

fn copy_library_tree(source: &Path, destination: &Path) -> Result<(), String> {
    fs::create_dir_all(destination).map_err(|error| {
        format!(
            "cannot create package directory {}: {error}",
            destination.display()
        )
    })?;
    let mut entries = fs::read_dir(source)
        .map_err(|error| {
            format!(
                "cannot inspect package source {}: {error}",
                source.display()
            )
        })?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| {
            format!(
                "cannot inspect package source {}: {error}",
                source.display()
            )
        })?;
    entries.sort();
    for path in entries {
        let relative = path.strip_prefix(source).unwrap();
        if excluded_path(relative) {
            continue;
        }
        let target = destination.join(relative);
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| format!("cannot inspect package entry {}: {error}", path.display()))?;
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "native package cannot contain symbolic links: {}",
                relative.display()
            ));
        }
        if metadata.is_dir() {
            copy_library_tree(&path, &target)?;
        } else if metadata.is_file() {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)
                    .map_err(|error| format!("cannot create package subdirectory: {error}"))?;
            }
            fs::copy(&path, &target)
                .map_err(|error| format!("cannot copy package file {}: {error}", path.display()))?;
        }
    }
    Ok(())
}

fn excluded_path(path: &Path) -> bool {
    path.components()
        .next()
        .is_some_and(|component| match component {
            Component::Normal(name) => matches!(
                name.to_str(),
                Some(
                    ".git"
                        | ".guion"
                        | "target"
                        | "dist"
                        | "node_modules"
                        | ".venv"
                        | "__pycache__"
                )
            ),
            _ => false,
        })
        || path.extension().is_some_and(|extension| extension == "pyc")
}

fn set_manifest_digest(path: &Path, digest: &str) -> Result<(), String> {
    let text = fs::read_to_string(path)
        .map_err(|error| format!("cannot read package manifest {}: {error}", path.display()))?;
    let mut value: toml::Value = toml::from_str(&text)
        .map_err(|error| format!("{}: invalid TOML: {error}", path.display()))?;
    let package = value
        .get_mut("package")
        .and_then(toml::Value::as_table_mut)
        .ok_or_else(|| format!("{} has no [package] table", path.display()))?;
    package.insert("sha256".into(), toml::Value::String(digest.into()));
    let serialized = toml::to_string_pretty(&value)
        .map_err(|error| format!("cannot write package manifest: {error}"))?;
    fs::write(path, serialized).map_err(|error| format!("cannot write package manifest: {error}"))
}

fn requirement_version(requirement: &toml::Value) -> Result<String, String> {
    let version = requirement
        .as_str()
        .or_else(|| requirement.get("version").and_then(toml::Value::as_str))
        .ok_or("native dependency must specify an exact version")?
        .to_owned();
    validate_exact_version(&version)?;
    Ok(version)
}

/// Names already present in a lock or manifest keep loading under the older rule (with a warning
/// when they break the current one).
fn validate_package_name(name: &str) -> Result<(), String> {
    crate::package_name::check_existing_package_name(name)
}

/// A new `add` follows the full rule set; updating an existing dependency does not re-litigate its
/// name.
fn validate_package_name_for_add(name: &str, updating: bool) -> Result<(), String> {
    if updating {
        validate_package_name(name)
    } else {
        crate::package_name::validate_new_package_name(name)
    }
}

fn validate_sha256(value: &str) -> Result<(), String> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("SHA-256 must contain exactly 64 hexadecimal characters".into());
    }
    Ok(())
}

fn validate_exact_version(version: &str) -> Result<(), String> {
    let parts = version.split('.').collect::<Vec<_>>();
    if parts.len() != 3
        || parts
            .iter()
            .any(|part| part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return Err(format!(
            "native package version must be an exact major.minor.patch value, got `{version}`"
        ));
    }
    Ok(())
}

fn validate_relative_path(path: &str) -> Result<(), String> {
    let relative = Path::new(path);
    if path.is_empty()
        || relative.is_absolute()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
        || path.contains('\\')
    {
        return Err(format!(
            "package path must be a safe relative path: `{path}`"
        ));
    }
    Ok(())
}

fn atomic_write(path: &Path, contents: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("{} has no parent directory", path.display()))?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
    let temporary = parent.join(format!(".guion-write-{}-{}", std::process::id(), nonce()?));
    fs::write(&temporary, contents)
        .map_err(|error| format!("cannot write {}: {error}", temporary.display()))?;
    fs::rename(&temporary, path).map_err(|error| {
        let _ = fs::remove_file(&temporary);
        format!("cannot replace {}: {error}", path.display())
    })
}

fn temporary_path(parent: &Path, prefix: &str) -> Result<PathBuf, String> {
    Ok(parent.join(format!(".{prefix}-{}-{}", std::process::id(), nonce()?)))
}

fn nonce() -> Result<u128, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .map_err(|error| format!("system clock is before UNIX epoch: {error}"))
}

#[cfg(test)]
mod archive_tests {
    use super::*;

    fn tar_entry(name: &str, kind: u8, data: &[u8]) -> Vec<u8> {
        let mut header = [0u8; 512];
        header[..name.len()].copy_from_slice(name.as_bytes());
        header[100..108].copy_from_slice(b"0000644\0");
        header[108..116].copy_from_slice(b"0000000\0");
        header[116..124].copy_from_slice(b"0000000\0");
        let size = format!("{:011o}\0", data.len());
        header[124..136].copy_from_slice(size.as_bytes());
        header[136..148].copy_from_slice(b"00000000000\0");
        header[148..156].fill(b' ');
        header[156] = kind;
        header[257..263].copy_from_slice(b"ustar\0");
        let checksum = header.iter().map(|byte| u64::from(*byte)).sum::<u64>();
        let encoded = format!("{checksum:06o}\0 ");
        header[148..156].copy_from_slice(encoded.as_bytes());
        let mut archive = header.to_vec();
        archive.extend_from_slice(data);
        archive.resize(512 + data.len().div_ceil(512) * 512, 0);
        archive.extend_from_slice(&[0; 1024]);
        archive
    }

    #[test]
    fn package_tar_extraction_accepts_files_and_rejects_escape_or_links() {
        let root = std::env::temp_dir().join(format!(
            "guion-tar-safety-{}-{}",
            std::process::id(),
            nonce().unwrap()
        ));
        let _ = fs::remove_dir_all(&root);
        let safe = root.join("safe");
        extract_tar_safely(
            &tar_entry(
                "src/lib.gu",
                b'0',
                b"pub fn answer() -> Int:\n    return 42\n",
            ),
            &safe,
        )
        .unwrap();
        assert_eq!(
            fs::read(safe.join("src/lib.gu")).unwrap(),
            b"pub fn answer() -> Int:\n    return 42\n"
        );

        let escaped = root.join("escaped");
        assert!(
            extract_tar_safely(&tar_entry("../outside", b'0', b"bad"), &escaped)
                .unwrap_err()
                .contains("safe relative")
        );
        assert!(!root.join("outside").exists());

        let linked = root.join("linked");
        assert!(
            extract_tar_safely(&tar_entry("src/link", b'2', b""), &linked)
                .unwrap_err()
                .contains("link or unsupported")
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn url_package_dependency_paths_stay_in_archive_and_local_paths_may_be_external() {
        let root = std::env::temp_dir().join(format!(
            "guion-package-path-boundary-{}-{}",
            std::process::id(),
            nonce().unwrap()
        ));
        let archive = root.join("archive");
        let package = archive.join("packages/top");
        let sibling = archive.join("packages/base");
        let outside = root.join("outside");
        fs::create_dir_all(&package).unwrap();
        fs::create_dir_all(&sibling).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::create_dir_all(package.join("src")).unwrap();
        fs::write(
            package.join("guion.toml"),
            "[package]\nname = \"top-lib\"\nversion = \"1.0.0\"\nentry = \"src/lib.gu\"\nexports = [\"src/lib.gu\"]\n\n[dependencies.outside-lib]\nversion = \"1.0.0\"\npath = \"../../../outside\"\n",
        )
        .unwrap();
        fs::write(
            package.join("src/lib.gu"),
            "pub fn top() -> Int:\n    return 1\n",
        )
        .unwrap();
        fs::create_dir_all(outside.join("src")).unwrap();
        fs::write(
            outside.join("guion.toml"),
            "[package]\nname = \"outside-lib\"\nversion = \"1.0.0\"\nentry = \"src/lib.gu\"\nexports = [\"src/lib.gu\"]\n",
        )
        .unwrap();
        fs::write(
            outside.join("src/lib.gu"),
            "pub fn outside() -> Int:\n    return 2\n",
        )
        .unwrap();

        let sibling_path =
            resolve_package_dependency_path(&package, "../base", Some(&archive)).unwrap();
        assert_eq!(sibling_path, fs::canonicalize(&sibling).unwrap());

        let escaped = resolve_package_dependency_path(&package, "../../../outside", Some(&archive))
            .expect_err("a URL package cannot resolve outside its extracted archive");
        assert!(escaped.contains("extracted archive"));

        let mut visiting = BTreeSet::new();
        let mut resolved = BTreeMap::new();
        let mut ordered = Vec::new();
        let graph_error = resolve_package_graph(
            &package,
            "top-lib",
            None,
            "url",
            "https://packages.example/top-lib.tar",
            None,
            Some(&archive),
            &root,
            &mut visiting,
            &mut resolved,
            &mut ordered,
        )
        .expect_err("URL package resolution must enforce the archive boundary");
        assert!(graph_error.contains("escapes its extracted archive"));

        visiting.clear();
        resolved.clear();
        ordered.clear();
        resolve_package_graph(
            &package,
            "top-lib",
            None,
            "path",
            &package.display().to_string(),
            None,
            None,
            &root,
            &mut visiting,
            &mut resolved,
            &mut ordered,
        )
        .expect("local package graphs may intentionally depend on sibling paths");

        let local_path = resolve_package_dependency_path(&package, "../../../outside", None)
            .expect("local path packages retain intentional external dependencies");
        assert_eq!(local_path, fs::canonicalize(&outside).unwrap());
        let _ = fs::remove_dir_all(&root);
    }
}
