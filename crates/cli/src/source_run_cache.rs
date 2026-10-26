use crate::compiler_driver;
use guion_model::{Artifact, Diagnostic};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

const CACHE_DIRECTORY: &str = ".guion/cache";
const CACHE_FILE: &str = "source-run.cache";
const CACHE_TEMP_PREFIX: &str = ".source-run.cache.tmp-";
const CACHE_MAGIC: &[u8; 8] = b"GUCACH01";
const CACHE_SCHEMA_VERSION: u32 = 1;
pub const MAX_CACHE_BYTES: u64 = 128 * 1024 * 1024;
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct CacheMetadata {
    schema_version: u32,
    key_sha256: String,
    source_graph_sha256: String,
    artifact_sha256: String,
    artifact_bytes: u64,
}

enum CachePath {
    Available(PathBuf),
    Disabled,
}

pub struct SourceRunCacheContext<'a> {
    pub entry: &'a Path,
    pub project_root: &'a Path,
    pub stdlib_root: Option<&'a Path>,
    pub package_roots: &'a HashMap<String, PathBuf>,
    pub compiler_identity: &'a str,
    pub runtime_identity: &'a str,
    pub no_cache: bool,
}

/// Load one validated compiled source artifact, or compile and atomically cache it.
/// The cache is a performance aid; any uncertainty falls back to the normal compiler.
pub fn load_or_compile<F>(
    context: SourceRunCacheContext<'_>,
    compile_fallback: F,
) -> Result<(Artifact, &'static str), Diagnostic>
where
    F: FnOnce() -> Result<Artifact, Diagnostic>,
{
    let SourceRunCacheContext {
        entry,
        project_root,
        stdlib_root,
        package_roots,
        compiler_identity,
        runtime_identity,
        no_cache,
    } = context;
    if no_cache {
        return compile_fallback().map(|artifact| (artifact, "bypassed"));
    }
    let root = match project_root.canonicalize() {
        Ok(root) => root,
        Err(_) => return compile_fallback().map(|artifact| (artifact, "disabled")),
    };
    let entry = match entry.canonicalize() {
        Ok(entry) if entry.starts_with(&root) && entry.is_file() => entry,
        _ => return compile_fallback().map(|artifact| (artifact, "disabled")),
    };
    let cache_path = match cache_path(&root) {
        CachePath::Available(path) => path,
        CachePath::Disabled => return compile_fallback().map(|artifact| (artifact, "disabled")),
    };
    let input_before = match input_identity(
        &entry,
        &root,
        stdlib_root,
        package_roots,
        compiler_identity,
        runtime_identity,
    ) {
        Ok(identity) => identity,
        Err(()) => return compile_fallback().map(|artifact| (artifact, "disabled")),
    };

    if let Some((metadata, bytes)) = read_cache(&cache_path) {
        if metadata.schema_version == CACHE_SCHEMA_VERSION
            && metadata.artifact_bytes == bytes.len() as u64
            && metadata.artifact_sha256 == sha256(&bytes)
        {
            if let Ok(artifact) = Artifact::from_bytes(&bytes) {
                if artifact.source_name == logical_entry(&entry, &root).unwrap_or_default() {
                    if let Ok(source_graph_sha256) = compiler_driver::source_graph_digest(
                        &artifact,
                        &root,
                        stdlib_root,
                        package_roots,
                    ) {
                        let expected_key = cache_key(&input_before, &source_graph_sha256);
                        if metadata.source_graph_sha256 == source_graph_sha256
                            && metadata.key_sha256 == expected_key
                        {
                            return Ok((artifact, "hit"));
                        }
                    }
                }
            }
        }
    }

    let source_snapshot_before = source_snapshot(&root, stdlib_root, package_roots).ok();
    let artifact = compile_fallback()?;
    let source_graph_sha256 =
        match compiler_driver::source_graph_digest(&artifact, &root, stdlib_root, package_roots) {
            Ok(digest) => digest,
            Err(_) => return Ok((artifact, "miss")),
        };
    let input_after = match input_identity(
        &entry,
        &root,
        stdlib_root,
        package_roots,
        compiler_identity,
        runtime_identity,
    ) {
        Ok(identity) => identity,
        Err(()) => return Ok((artifact, "miss")),
    };
    let source_snapshot_after = source_snapshot(&root, stdlib_root, package_roots).ok();
    if input_before != input_after || source_snapshot_before != source_snapshot_after {
        return Ok((artifact, "miss"));
    }
    let Ok(bytes) = artifact.to_bytes() else {
        return Ok((artifact, "miss"));
    };
    let metadata = CacheMetadata {
        schema_version: CACHE_SCHEMA_VERSION,
        key_sha256: cache_key(&input_after, &source_graph_sha256),
        source_graph_sha256,
        artifact_sha256: sha256(&bytes),
        artifact_bytes: bytes.len() as u64,
    };
    let _ = write_cache(&root, &cache_path, &metadata, &bytes);
    Ok((artifact, "miss"))
}

pub fn clean_project_cache(project_root: &Path) -> Result<u64, String> {
    let root = project_root
        .canonicalize()
        .map_err(|error| format!("cannot resolve project {}: {error}", project_root.display()))?;
    let cache_dir = root.join(CACHE_DIRECTORY);
    match safe_directory(&root, &cache_dir, false) {
        Ok(false) => return Ok(0),
        Ok(true) => {}
        Err(message) => return Err(message),
    }
    let cache_file = cache_dir.join(CACHE_FILE);
    let mut removed_bytes = remove_owned_file(&root, &cache_file)?;
    for entry in
        fs::read_dir(&cache_dir).map_err(|error| format!("cannot inspect Guion cache: {error}"))?
    {
        let entry = entry.map_err(|error| format!("cannot inspect Guion cache entry: {error}"))?;
        if entry
            .file_name()
            .to_string_lossy()
            .starts_with(CACHE_TEMP_PREFIX)
        {
            removed_bytes += remove_owned_file(&root, &entry.path())?;
        }
    }
    Ok(removed_bytes)
}

fn cache_path(root: &Path) -> CachePath {
    let directory = root.join(CACHE_DIRECTORY);
    match safe_directory(root, &directory, false) {
        Ok(_) => {
            let file = directory.join(CACHE_FILE);
            if file.exists() {
                let Ok(metadata) = fs::symlink_metadata(&file) else {
                    return CachePath::Disabled;
                };
                if metadata.file_type().is_symlink() || !metadata.is_file() {
                    return CachePath::Disabled;
                }
                if !file.canonicalize().is_ok_and(|path| path.starts_with(root)) {
                    return CachePath::Disabled;
                }
            }
            CachePath::Available(file)
        }
        Err(_) => CachePath::Disabled,
    }
}

fn safe_directory(root: &Path, directory: &Path, create: bool) -> Result<bool, String> {
    let relative = directory
        .strip_prefix(root)
        .map_err(|_| "cache path is outside the project root".to_owned())?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(format!(
                    "refusing unsafe cache directory {}",
                    current.display()
                ));
            }
            Ok(_) => {
                let canonical = current
                    .canonicalize()
                    .map_err(|error| format!("cannot resolve cache directory: {error}"))?;
                if !canonical.starts_with(root) {
                    return Err("cache directory resolves outside the project root".into());
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && create => {
                match fs::create_dir(&current) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                    Err(error) => {
                        return Err(format!("cannot create Guion cache directory: {error}"))
                    }
                }
                let metadata = fs::symlink_metadata(&current)
                    .map_err(|error| format!("cannot inspect Guion cache directory: {error}"))?;
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    return Err("refusing unsafe Guion cache directory".into());
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(format!("cannot inspect Guion cache directory: {error}")),
        }
    }
    Ok(true)
}

fn input_identity(
    entry: &Path,
    root: &Path,
    stdlib_root: Option<&Path>,
    package_roots: &HashMap<String, PathBuf>,
    compiler_identity: &str,
    runtime_identity: &str,
) -> Result<String, ()> {
    let mut fields = serde_json::Map::new();
    fields.insert("schema".into(), serde_json::json!(CACHE_SCHEMA_VERSION));
    fields.insert(
        "entry".into(),
        serde_json::json!(logical_entry(entry, root)?),
    );
    fields.insert(
        "entry_sha256".into(),
        serde_json::json!(sha256(&fs::read(entry).map_err(|_| ())?)),
    );
    fields.insert(
        "manifest".into(),
        serde_json::json!(optional_file_identity(&manifest_path(root))?),
    );
    fields.insert(
        "lock".into(),
        serde_json::json!(optional_file_identity(&lock_path(root))?),
    );
    fields.insert("compiler".into(), serde_json::json!(compiler_identity));
    fields.insert("runtime".into(), serde_json::json!(runtime_identity));
    fields.insert(
        "target".into(),
        serde_json::json!(format!(
            "{}-{}",
            std::env::consts::OS,
            std::env::consts::ARCH
        )),
    );
    fields.insert(
        "stdlib".into(),
        serde_json::json!(root_identity(stdlib_root)?),
    );
    let mut packages = serde_json::Map::new();
    let mut ordered_packages = package_roots.iter().collect::<Vec<_>>();
    ordered_packages.sort_by(|left, right| left.0.cmp(right.0));
    for (name, package_root) in ordered_packages {
        packages.insert(
            name.clone(),
            serde_json::json!(root_identity(Some(package_root))?),
        );
    }
    fields.insert("packages".into(), serde_json::Value::Object(packages));
    Ok(sha256(
        serde_json::to_string(&fields).map_err(|_| ())?.as_bytes(),
    ))
}

fn optional_file_identity(path: &Path) -> Result<serde_json::Value, ()> {
    match fs::read(path) {
        Ok(bytes) => Ok(serde_json::json!({"present": true, "sha256": sha256(&bytes)})),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(serde_json::json!({"present": false}))
        }
        Err(_) => Err(()),
    }
}

fn manifest_path(root: &Path) -> PathBuf {
    let current = root.join("guion.toml");
    if current.is_file() {
        current
    } else {
        root.join("veritas.toml")
    }
}

fn lock_path(root: &Path) -> PathBuf {
    let current = root.join("guion.lock");
    if current.is_file() {
        current
    } else {
        root.join("veritas.lock")
    }
}

fn root_identity(root: Option<&Path>) -> Result<serde_json::Value, ()> {
    let Some(root) = root else {
        return Ok(serde_json::json!({"present": false}));
    };
    let canonical = root.canonicalize().map_err(|_| ())?;
    if !canonical.is_dir() {
        return Err(());
    }
    let digest = directory_digest(&canonical)?;
    Ok(serde_json::json!({"present": true, "path": canonical, "sha256": digest}))
}

fn directory_digest(root: &Path) -> Result<String, ()> {
    let mut files = Vec::new();
    collect_source_files(root, root, &mut files)?;
    files.sort_by(|left, right| left.0.cmp(&right.0));
    let mut digest = Sha256::new();
    digest.update((files.len() as u64).to_be_bytes());
    for (name, path) in files {
        let bytes = fs::read(path).map_err(|_| ())?;
        digest.update((name.len() as u64).to_be_bytes());
        digest.update(name.as_bytes());
        digest.update((bytes.len() as u64).to_be_bytes());
        digest.update(bytes);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn collect_source_files(
    root: &Path,
    current: &Path,
    output: &mut Vec<(String, PathBuf)>,
) -> Result<(), ()> {
    for entry in fs::read_dir(current).map_err(|_| ())? {
        let entry = entry.map_err(|_| ())?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(|_| ())?;
        if metadata.file_type().is_symlink() {
            return Err(());
        }
        if metadata.is_dir() {
            collect_source_files(root, &path, output)?;
        } else if metadata.is_file()
            && matches!(
                path.extension().and_then(|extension| extension.to_str()),
                Some("gu" | "vr")
            )
        {
            let relative = path
                .strip_prefix(root)
                .map_err(|_| ())?
                .to_string_lossy()
                .replace('\\', "/");
            output.push((relative, path));
        }
    }
    Ok(())
}

fn source_snapshot(
    root: &Path,
    stdlib_root: Option<&Path>,
    package_roots: &HashMap<String, PathBuf>,
) -> Result<String, ()> {
    let mut roots = vec![("project".to_owned(), root.to_path_buf())];
    if let Some(stdlib) = stdlib_root {
        roots.push(("stdlib".to_owned(), stdlib.canonicalize().map_err(|_| ())?));
    }
    for (name, path) in package_roots {
        roots.push((
            format!("package:{name}"),
            path.canonicalize().map_err(|_| ())?,
        ));
    }
    roots.sort_by(|left, right| left.0.cmp(&right.0));
    let mut digest = Sha256::new();
    for (name, path) in roots {
        digest.update(name.as_bytes());
        digest.update(directory_digest(&path)?.as_bytes());
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn logical_entry(entry: &Path, root: &Path) -> Result<String, ()> {
    Ok(entry
        .strip_prefix(root)
        .map_err(|_| ())?
        .to_string_lossy()
        .replace('\\', "/"))
}

fn cache_key(input_identity: &str, source_graph_sha256: &str) -> String {
    sha256(format!("{CACHE_SCHEMA_VERSION}:{input_identity}:{source_graph_sha256}").as_bytes())
}

fn read_cache(path: &Path) -> Option<(CacheMetadata, Vec<u8>)> {
    let metadata = fs::metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_CACHE_BYTES {
        return None;
    }
    let bytes = fs::read(path).ok()?;
    if bytes.get(..CACHE_MAGIC.len())? != CACHE_MAGIC {
        return None;
    }
    let metadata_end = CACHE_MAGIC.len() + 4;
    let metadata_len = u32::from_le_bytes(
        bytes
            .get(CACHE_MAGIC.len()..metadata_end)?
            .try_into()
            .ok()?,
    ) as usize;
    let artifact_start = metadata_end.checked_add(metadata_len)?;
    let value: serde_json::Value =
        serde_json::from_slice(bytes.get(metadata_end..artifact_start)?).ok()?;
    let metadata = CacheMetadata {
        schema_version: u32::try_from(value.get("schema_version")?.as_u64()?).ok()?,
        key_sha256: value.get("key_sha256")?.as_str()?.to_owned(),
        source_graph_sha256: value.get("source_graph_sha256")?.as_str()?.to_owned(),
        artifact_sha256: value.get("artifact_sha256")?.as_str()?.to_owned(),
        artifact_bytes: value.get("artifact_bytes")?.as_u64()?,
    };
    Some((metadata, bytes.get(artifact_start..)?.to_vec()))
}

fn write_cache(
    root: &Path,
    path: &Path,
    metadata: &CacheMetadata,
    artifact: &[u8],
) -> Result<(), ()> {
    if artifact.len() as u64 > MAX_CACHE_BYTES {
        return Ok(());
    }
    let directory = path.parent().ok_or(())?;
    if !safe_directory(root, directory, true).map_err(|_| ())? {
        return Err(());
    }
    let metadata_bytes = serde_json::to_vec(&serde_json::json!({
        "schema_version": metadata.schema_version,
        "key_sha256": metadata.key_sha256,
        "source_graph_sha256": metadata.source_graph_sha256,
        "artifact_sha256": metadata.artifact_sha256,
        "artifact_bytes": metadata.artifact_bytes,
    }))
    .map_err(|_| ())?;
    let metadata_len = u32::try_from(metadata_bytes.len()).map_err(|_| ())?;
    let total_size =
        CACHE_MAGIC.len() as u64 + 4 + metadata_bytes.len() as u64 + artifact.len() as u64;
    if total_size > MAX_CACHE_BYTES {
        return Ok(());
    }
    let temporary = directory.join(format!(
        "{CACHE_TEMP_PREFIX}{}-{}",
        std::process::id(),
        NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
    ));
    let write_result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|_| ())?;
        file.write_all(CACHE_MAGIC).map_err(|_| ())?;
        file.write_all(&metadata_len.to_le_bytes())
            .map_err(|_| ())?;
        file.write_all(&metadata_bytes).map_err(|_| ())?;
        file.write_all(artifact).map_err(|_| ())?;
        file.sync_all().map_err(|_| ())?;
        drop(file);
        if !safe_directory(root, directory, false).map_err(|_| ())? {
            return Err(());
        }
        fs::rename(&temporary, path).map_err(|_| ())
    })();
    if write_result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    write_result
}

fn remove_owned_file(root: &Path, path: &Path) -> Result<u64, String> {
    if !path.exists() {
        return Ok(0);
    }
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("cannot inspect Guion cache file: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(format!(
            "refusing to remove unsafe cache path {}",
            path.display()
        ));
    }
    if !path
        .canonicalize()
        .is_ok_and(|canonical| canonical.starts_with(root))
    {
        return Err("refusing to remove a cache file outside the project root".into());
    }
    let size = metadata.len();
    fs::remove_file(path).map_err(|error| format!("cannot remove Guion cache file: {error}"))?;
    Ok(size)
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
