use guion_model::{Artifact, Diagnostic, Span};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    env, fs,
    path::{Path, PathBuf},
    time::Instant,
};

pub fn compile_project_with_roots(
    entry: &Path,
    project_root: &Path,
    stdlib_root: Option<&Path>,
    package_roots: &HashMap<String, PathBuf>,
) -> Result<Artifact, Diagnostic> {
    guion_compiler::compile_project_with_roots(entry, project_root, stdlib_root, package_roots)
}

#[derive(Clone, Debug, Default)]
pub(crate) struct CompilerPhaseTimings {
    pub(crate) rust_compiler_compile_ns: u128,
}

pub(crate) fn compile_project_with_roots_profiled(
    entry: &Path,
    project_root: &Path,
    stdlib_root: Option<&Path>,
    package_roots: &HashMap<String, PathBuf>,
) -> Result<(Artifact, CompilerPhaseTimings), Diagnostic> {
    let started = Instant::now();
    let artifact = compile_project_with_roots(entry, project_root, stdlib_root, package_roots)?;
    let timings = CompilerPhaseTimings {
        rust_compiler_compile_ns: started.elapsed().as_nanos(),
    };
    Ok((artifact, timings))
}

pub(crate) fn source_graph_digest(
    artifact: &Artifact,
    project_root: &Path,
    stdlib_root: Option<&Path>,
    package_roots: &HashMap<String, PathBuf>,
) -> Result<String, Diagnostic> {
    let mut sources = Vec::with_capacity(artifact.sources.len());
    for name in &artifact.sources {
        let (path, allowed_root) = if let Some(relative) = name.strip_prefix("std:") {
            let mut module_path = relative.to_owned();
            if !module_path.ends_with(".gu") && !module_path.ends_with(".vr") {
                module_path.push_str(".gu");
            }
            (
                stdlib_root
                    .ok_or_else(|| {
                        failure("E_IMPORT_ROOT", "standard-library root is missing", None)
                    })?
                    .join(module_path),
                stdlib_root.expect("checked standard-library root"),
            )
        } else if let Some(package_path) = name.strip_prefix("pkg:") {
            let (package, relative) = package_path.split_once('/').ok_or_else(|| {
                failure(
                    "E_SOURCE_IDENTITY",
                    format!("invalid package source identity `{name}`"),
                    None,
                )
            })?;
            let root = package_roots.get(package).ok_or_else(|| {
                failure(
                    "E_IMPORT_ROOT",
                    format!("package root `{package}` is missing"),
                    None,
                )
            })?;
            (root.join(relative), root.as_path())
        } else {
            (project_root.join(name), project_root)
        };
        let canonical_path = path.canonicalize().map_err(|error| {
            failure(
                "E_SOURCE_READ",
                format!(
                    "cannot resolve compiled source {} for its digest: {error}",
                    path.display()
                ),
                Some(name.clone()),
            )
        })?;
        let canonical_root = allowed_root.canonicalize().map_err(|error| {
            failure(
                "E_IMPORT_ROOT",
                format!(
                    "cannot resolve source root {}: {error}",
                    allowed_root.display()
                ),
                Some(name.clone()),
            )
        })?;
        if !canonical_path.starts_with(&canonical_root) || !canonical_path.is_file() {
            return Err(failure(
                "E_SOURCE_IDENTITY",
                format!("compiled source `{name}` resolves outside its declared root"),
                Some(name.clone()),
            ));
        }
        let contents = fs::read(&canonical_path).map_err(|error| {
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

pub(crate) fn cache_identity() -> String {
    cache_identity_for_build_id(env!("GUION_COMPILER_BUILD_ID"))
}

fn cache_identity_for_build_id(build_id: &str) -> String {
    let mut digest = Sha256::new();
    for value in [
        build_id.as_bytes(),
        env!("CARGO_PKG_VERSION").as_bytes(),
        (Artifact::CURRENT_VERSION as u64).to_be_bytes().as_slice(),
        env::consts::OS.as_bytes(),
        env::consts::ARCH.as_bytes(),
    ] {
        digest.update((value.len() as u64).to_be_bytes());
        digest.update(value);
    }
    format!("{:x}", digest.finalize())
}

pub fn standard_library_root() -> Option<PathBuf> {
    if let Some(path) = env::var_os("GUION_STDLIB_ROOT") {
        return Some(PathBuf::from(path));
    }
    if let Ok(executable) = env::current_exe() {
        if let Some(directory) = executable.parent() {
            for candidate in [
                directory.join("stdlib"),
                directory.join("../share/guion/stdlib"),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiler_cache_identity_includes_compiler_build_fingerprint() {
        assert_ne!(
            cache_identity_for_build_id("compiler-source-a"),
            cache_identity_for_build_id("compiler-source-b")
        );
    }
}
