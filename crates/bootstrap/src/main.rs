use sha2::{Digest, Sha256};
use std::{env, fs, path::Path};
use veritas_bootstrap::assemble_ir_file;
use veritas_compiler::compile_project;
use veritas_model::{Artifact, BuildMetadata, ResourceLimits};

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut arguments = env::args().skip(1);
    match arguments.next().as_deref() {
        Some("seed") => {
            let root = arguments.next().unwrap_or_else(|| ".".into());
            if arguments.next().is_some() {
                return Err(usage());
            }
            build_seed(Path::new(&root))
        }
        Some("assemble-ir") => {
            let input = arguments.next().ok_or_else(usage)?;
            let output = arguments.next().ok_or_else(usage)?;
            if arguments.next().is_some() {
                return Err(usage());
            }
            assemble_ir_file(&input, &output)?;
            println!("assembled {input} -> {output}");
            Ok(())
        }
        _ => Err(usage()),
    }
}

fn build_seed(repository: &Path) -> Result<(), String> {
    let repository = repository
        .canonicalize()
        .map_err(|error| format!("cannot resolve repository root: {error}"))?;
    let self_host = repository.join("language/self_host");
    let entry = self_host.join("compiler.vr");
    let mut artifact = compile_project(&entry, &self_host).map_err(|diagnostic| {
        let source = diagnostic.source_name.as_deref().unwrap_or("compiler.vr");
        format!(
            "{source}:{}:{}: error[{}]: {}",
            diagnostic.span.line, diagnostic.span.column, diagnostic.code, diagnostic.message
        )
    })?;
    let source_sha256 = artifact
        .compiled_source_sha256
        .clone()
        .ok_or("stage-zero compiler did not compute the compiler source graph digest")?;
    let schema = fs::read(repository.join("language/artifact-format.md"))
        .map_err(|error| format!("cannot read artifact format contract: {error}"))?;
    artifact.build_metadata = Some(BuildMetadata {
        metadata_schema_version: 1,
        compiler_version: env!("CARGO_PKG_VERSION").to_owned(),
        target: "veritas-host-abi-1".into(),
        source_sha256: Some(source_sha256.clone()),
        dependency_lock_sha256: None,
        resource_limits: ResourceLimits {
            instruction_limit: 500_000_000,
            allocation_limit_bytes: 8_589_934_592,
            call_depth_limit: 160,
            output_limit_bytes: 1_000_000,
            max_integer_digits: 4_096,
            max_text_bytes: 16 * 1024 * 1024,
            max_collection_items: 1_000_000,
            max_parallelism: 64,
            max_file_bytes: 64 * 1024 * 1024,
        },
    });
    let bytes = artifact
        .to_bytes()
        .map_err(|error| format!("error[{}]: {}", error.code, error.message))?;
    let artifact_sha256 = format!("{:x}", Sha256::digest(&bytes));
    let artifact_schema_sha256 = format!("{:x}", Sha256::digest(&schema));
    let artifact_path = self_host.join("compiler.vrb");
    let manifest_path = self_host.join("compiler-seed.json");
    write_if_changed(&artifact_path, &bytes)?;
    let manifest = serde_json::json!({
        "schema_version": 1,
        "artifact_format": Artifact::CURRENT_VERSION,
        "host_abi_version": 1,
        "compiler_entry": "compiler.vr",
        "compiler_source_files": artifact.sources,
        "compiler_source_sha256": source_sha256,
        "artifact_sha256": artifact_sha256,
        "artifact_schema_sha256": artifact_schema_sha256,
        "regeneration_command": "cargo run --locked --offline -p veritas-bootstrap --bin veritas-stage0 -- seed .",
    });
    let manifest_bytes = serde_json::to_vec_pretty(&manifest)
        .map_err(|error| format!("cannot encode seed manifest: {error}"))?;
    write_if_changed(&manifest_path, &manifest_bytes)?;
    println!(
        "wrote {} ({} bytes); source sha256 {}",
        artifact_path.display(),
        bytes.len(),
        source_sha256
    );
    Ok(())
}

fn write_if_changed(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if fs::read(path).ok().as_deref() == Some(bytes) {
        return Ok(());
    }
    fs::write(path, bytes).map_err(|error| format!("cannot write {}: {error}", path.display()))
}

fn usage() -> String {
    "usage: veritas-stage0 seed [repository-root] | assemble-ir <input.vri.json> <output.vrb>"
        .into()
}
