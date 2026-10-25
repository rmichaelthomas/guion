use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    env, fs,
    path::{Path, PathBuf},
};
use veritas_model::{Artifact, Diagnostic, RuntimeError, SharedJson, Span, Value};
use veritas_runtime::{HostPolicy, Runtime};

const COMPILER_SEED: &[u8] = include_bytes!("../../../language/self_host/compiler.vrb");
const COMPILER_SEED_MANIFEST: &str = include_str!("../../../language/self_host/compiler-seed.json");
const ARTIFACT_SCHEMA: &str = include_str!("../../../language/artifact-format.md");
const COMPILER_ENTRY: &str = "compiler.vr";
const HOST_ABI_VERSION: u64 = 1;

pub fn compile_project_with_roots(
    entry: &Path,
    project_root: &Path,
    stdlib_root: Option<&Path>,
    package_roots: &HashMap<String, PathBuf>,
) -> Result<Artifact, Diagnostic> {
    let root = project_root
        .canonicalize()
        .map_err(|error| failure("E_PROJECT_ROOT", error.to_string(), None))?;
    let entry = entry
        .canonicalize()
        .map_err(|error| failure("E_PROJECT_ENTRY", error.to_string(), None))?;
    if !entry.starts_with(&root) || !entry.is_file() {
        return Err(failure(
            "E_PROJECT_ENTRY",
            "project entry must be a file inside its root",
            None,
        ));
    }
    let logical_entry = relative_name(&entry, &root)
        .map_err(|message| failure("E_SOURCE_IDENTITY", message, None))?;
    let source = fs::read_to_string(&entry).map_err(|error| {
        failure(
            "E_SOURCE_READ",
            format!("cannot read {}: {error}", entry.display()),
            Some(logical_entry.clone()),
        )
    })?;
    let stdlib_root = stdlib_root.map(canonical_directory).transpose()?;
    let mut canonical_packages = HashMap::new();
    for (name, path) in package_roots {
        if name.is_empty() || name.contains('/') || name.contains('\\') {
            return Err(failure(
                "E_IMPORT_ROOT",
                format!("invalid package root name `{name}`"),
                None,
            ));
        }
        canonical_packages.insert(name.clone(), canonical_directory(path)?);
    }

    let seed = checked_seed()?;
    let mut policy = HostPolicy {
        instruction_limit: 500_000_000,
        allocation_limit_bytes: 8_589_934_592,
        call_depth_limit: 160,
        max_collection_items: 1_000_000,
        max_file_bytes: 64 * 1024 * 1024,
        ..HostPolicy::default()
    }
    .with_file_access(&root, true, false);
    if let Some(stdlib) = &stdlib_root {
        policy = policy.with_file_root_alias("std", stdlib);
    }
    for (package, package_root) in &canonical_packages {
        policy = policy.with_file_root_alias(format!("pkg:{package}"), package_root);
    }
    let package_json = serde_json::Value::Object(
        canonical_packages
            .iter()
            .map(|(name, _path)| {
                (
                    name.clone(),
                    serde_json::Value::String(format!("pkg:{name}")),
                )
            })
            .collect(),
    );
    let stdlib_path = stdlib_root
        .as_ref()
        .map(|_| "std".to_owned())
        .unwrap_or_default();
    let mut runtime = Runtime::new()
        .load(seed, policy)
        .map_err(|error| runtime_failure(error, Some(logical_entry.clone())))?;
    let result = runtime.invoke(
        "compiler_compile_project",
        vec![
            Value::Text(logical_entry.clone()),
            Value::Text(source),
            Value::Text(stdlib_path),
            Value::Json(SharedJson::from_serde(package_json)),
        ],
    );
    let result = result.map_err(|error| runtime_failure(error, Some(logical_entry.clone())))?;
    let bytes = match result {
        Value::ResultOk(value) => match *value {
            Value::Bytes(bytes) => bytes,
            value => {
                return Err(failure(
                    "E_COMPILER_PROTOCOL",
                    format!("compiler returned {value:?}; expected Bytes"),
                    Some(logical_entry),
                ));
            }
        },
        Value::ResultErr(error) => {
            let Value::Json(error) = *error else {
                return Err(failure(
                    "E_COMPILER_PROTOCOL",
                    "compiler returned a non-JSON diagnostic",
                    Some(logical_entry),
                ));
            };
            return Err(compile_diagnostic(&error.to_serde(), &logical_entry));
        }
        value => {
            return Err(failure(
                "E_COMPILER_PROTOCOL",
                format!("compiler returned {value:?}; expected Result[Bytes, Json]"),
                Some(logical_entry),
            ));
        }
    };
    let mut artifact = Artifact::from_bytes(bytes.as_ref()).map_err(|error| {
        failure(
            error.code,
            format!(
                "self-hosted compiler emitted an invalid artifact: {}",
                error.message
            ),
            Some(logical_entry.clone()),
        )
    })?;
    normalize_artifact_sources(
        &mut artifact,
        &root,
        stdlib_root.as_deref(),
        &canonical_packages,
    )?;
    artifact.compiled_source_sha256 = Some(source_graph_digest(
        &artifact,
        &root,
        stdlib_root.as_deref(),
        &canonical_packages,
    )?);
    Ok(artifact)
}

pub fn verify_embedded_seed() -> Result<(), Diagnostic> {
    checked_seed().map(|_| ())
}

fn checked_seed() -> Result<Artifact, Diagnostic> {
    validate_seed(COMPILER_SEED, COMPILER_SEED_MANIFEST, ARTIFACT_SCHEMA)
}

fn validate_seed(
    seed_bytes: &[u8],
    manifest_text: &str,
    artifact_schema: &str,
) -> Result<Artifact, Diagnostic> {
    let manifest: serde_json::Value = serde_json::from_str(manifest_text).map_err(|error| {
        failure(
            "E_COMPILER_SEED",
            format!("embedded compiler seed manifest is invalid: {error}"),
            None,
        )
    })?;
    let seed_hash = hex_sha256(seed_bytes);
    if manifest["schema_version"].as_u64() != Some(1)
        || manifest["artifact_format"].as_u64() != Some(Artifact::CURRENT_VERSION as u64)
        || manifest["host_abi_version"].as_u64() != Some(HOST_ABI_VERSION)
        || manifest["compiler_entry"].as_str() != Some(COMPILER_ENTRY)
        || manifest["artifact_sha256"].as_str() != Some(seed_hash.as_str())
        || manifest["artifact_schema_sha256"].as_str()
            != Some(hex_sha256(artifact_schema.as_bytes()).as_str())
    {
        return Err(failure(
            "E_COMPILER_SEED",
            "embedded compiler seed identity does not match its manifest or artifact contract; regenerate it with veritas-stage0",
            None,
        ));
    }
    let artifact = Artifact::from_bytes(seed_bytes).map_err(|error| {
        failure(
            "E_COMPILER_SEED",
            format!(
                "embedded compiler seed is invalid: {}: {}",
                error.code, error.message
            ),
            None,
        )
    })?;
    let metadata = artifact.build_metadata.as_ref();
    if artifact.format_version != Artifact::CURRENT_VERSION
        || artifact.entry != "main"
        || !artifact
            .sources
            .iter()
            .any(|source| source == COMPILER_ENTRY)
        || metadata.is_none_or(|metadata| {
            metadata.target != format!("veritas-host-abi-{HOST_ABI_VERSION}")
                || metadata.source_sha256.as_deref() != manifest["compiler_source_sha256"].as_str()
        })
    {
        return Err(failure(
            "E_COMPILER_SEED",
            "embedded compiler seed payload does not match the declared source, format, or host ABI identity",
            None,
        ));
    }
    Ok(artifact)
}

fn canonical_directory(path: &Path) -> Result<PathBuf, Diagnostic> {
    let path = path
        .canonicalize()
        .map_err(|error| failure("E_IMPORT_ROOT", error.to_string(), None))?;
    if !path.is_dir() {
        return Err(failure(
            "E_IMPORT_ROOT",
            "import root must be a directory",
            None,
        ));
    }
    Ok(path)
}

fn normalize_artifact_sources(
    artifact: &mut Artifact,
    project_root: &Path,
    stdlib_root: Option<&Path>,
    package_roots: &HashMap<String, PathBuf>,
) -> Result<(), Diagnostic> {
    artifact.source_name = normalize_source_name(
        &artifact.source_name,
        project_root,
        stdlib_root,
        package_roots,
    )?;
    for source in &mut artifact.sources {
        *source = normalize_source_name(source, project_root, stdlib_root, package_roots)?;
    }
    Ok(())
}

fn normalize_source_name(
    source: &str,
    project_root: &Path,
    stdlib_root: Option<&Path>,
    package_roots: &HashMap<String, PathBuf>,
) -> Result<String, Diagnostic> {
    if source.starts_with("std:") || source.starts_with("pkg:") {
        return Ok(source.to_owned());
    }
    let path = Path::new(source);
    if !path.is_absolute() {
        return Ok(source.replace('\\', "/"));
    }
    if let Ok(relative) = path.strip_prefix(project_root) {
        return portable_relative(relative)
            .map_err(|message| failure("E_SOURCE_IDENTITY", message, None));
    }
    if let Some(stdlib) = stdlib_root {
        if let Ok(relative) = path.strip_prefix(stdlib) {
            return portable_relative(relative)
                .map(|name| format!("std:{name}"))
                .map_err(|message| failure("E_SOURCE_IDENTITY", message, None));
        }
    }
    for (package, root) in package_roots {
        if let Ok(relative) = path.strip_prefix(root) {
            return portable_relative(relative)
                .map(|name| format!("pkg:{package}/{name}"))
                .map_err(|message| failure("E_SOURCE_IDENTITY", message, None));
        }
    }
    Err(failure(
        "E_SOURCE_IDENTITY",
        format!("compiler source `{source}` is outside every granted root"),
        None,
    ))
}

fn relative_name(path: &Path, root: &Path) -> Result<String, String> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| "source path is outside its declared root".to_owned())?;
    portable_relative(relative)
}

fn portable_relative(path: &Path) -> Result<String, String> {
    if path.is_absolute()
        || path
            .components()
            .any(|part| !matches!(part, std::path::Component::Normal(_)))
    {
        return Err("source path is not a safe relative identity".into());
    }
    Ok(path.to_string_lossy().replace('\\', "/"))
}

fn source_graph_digest(
    artifact: &Artifact,
    project_root: &Path,
    stdlib_root: Option<&Path>,
    package_roots: &HashMap<String, PathBuf>,
) -> Result<String, Diagnostic> {
    let mut sources = Vec::with_capacity(artifact.sources.len());
    for name in &artifact.sources {
        let path = if let Some(relative) = name.strip_prefix("std:") {
            let mut module_path = relative.to_owned();
            if !module_path.ends_with(".vr") {
                module_path.push_str(".vr");
            }
            stdlib_root
                .ok_or_else(|| failure("E_IMPORT_ROOT", "standard-library root is missing", None))?
                .join(module_path)
        } else if let Some(package_path) = name.strip_prefix("pkg:") {
            let (package, relative) = package_path.split_once('/').ok_or_else(|| {
                failure(
                    "E_SOURCE_IDENTITY",
                    format!("invalid package source identity `{name}`"),
                    None,
                )
            })?;
            package_roots
                .get(package)
                .ok_or_else(|| {
                    failure(
                        "E_IMPORT_ROOT",
                        format!("package root `{package}` is missing"),
                        None,
                    )
                })?
                .join(relative)
        } else {
            project_root.join(name)
        };
        let contents = fs::read(&path).map_err(|error| {
            failure(
                "E_SOURCE_READ",
                format!(
                    "cannot read compiled source {} for its digest: {error}",
                    path.display()
                ),
                Some(name.clone()),
            )
        })?;
        sources.push((name.clone(), contents));
    }
    sources.sort_by(|left, right| left.0.cmp(&right.0));
    let mut digest = Sha256::new();
    digest.update((sources.len() as u64).to_be_bytes());
    for (name, source) in sources {
        digest.update((name.len() as u64).to_be_bytes());
        digest.update(name.as_bytes());
        digest.update((source.len() as u64).to_be_bytes());
        digest.update(source);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn compile_diagnostic(value: &serde_json::Value, fallback_source: &str) -> Diagnostic {
    let diagnostic = &value["diagnostic"];
    let code = diagnostic["code"].as_str().unwrap_or("E_COMPILE");
    let message = diagnostic["message"]
        .as_str()
        .unwrap_or("compilation failed");
    let span = &diagnostic["span"];
    let span = Span {
        source_id: 0,
        start: span["start"].as_u64().unwrap_or(0) as usize,
        end: span["end"].as_u64().unwrap_or(0) as usize,
        line: span["line"].as_u64().unwrap_or(1) as usize,
        column: span["column"].as_u64().unwrap_or(1) as usize,
    };
    Diagnostic::new(code, message, span).with_source_name(
        diagnostic["source_name"]
            .as_str()
            .filter(|name| !name.is_empty())
            .unwrap_or(fallback_source),
    )
}

fn runtime_failure(error: RuntimeError, source: Option<String>) -> Diagnostic {
    let span = error.trace.first().copied().unwrap_or_default();
    let message = error.message;
    let diagnostic = Diagnostic::new(error.code, message, span);
    match source {
        Some(source) => diagnostic.with_source_name(source),
        None => diagnostic,
    }
}

fn failure(
    code: impl Into<String>,
    message: impl Into<String>,
    source: Option<String>,
) -> Diagnostic {
    let diagnostic = Diagnostic::new(code, message, Span::default());
    match source {
        Some(source) => diagnostic.with_source_name(source),
        None => diagnostic,
    }
}

fn hex_sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn standard_library_root() -> Option<PathBuf> {
    if let Some(path) = env::var_os("VERITAS_STDLIB_ROOT") {
        return Some(PathBuf::from(path));
    }
    if let Ok(executable) = env::current_exe() {
        if let Some(directory) = executable.parent() {
            for candidate in [
                directory.join("stdlib"),
                directory.join("../share/veritas/stdlib"),
            ] {
                if candidate.is_dir() {
                    return candidate.canonicalize().ok();
                }
            }
        }
    }
    let development = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../language/stdlib");
    development.is_dir().then_some(development)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mismatched_embedded_seed_identity_fails_closed() {
        let mut tampered = COMPILER_SEED.to_vec();
        tampered[0] ^= 0xff;
        let error = validate_seed(&tampered, COMPILER_SEED_MANIFEST, ARTIFACT_SCHEMA)
            .expect_err("a seed that differs from its manifest must not load");
        assert_eq!(error.code, "E_COMPILER_SEED");
    }
}
