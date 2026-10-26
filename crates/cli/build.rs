use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    env, fs,
    path::{Path, PathBuf},
};

fn main() {
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let repository = manifest_dir.join("../..");
    let compiler_build_id = compiler_build_id(&repository);
    println!("cargo:rustc-env=GUION_COMPILER_BUILD_ID={compiler_build_id}");
    let self_host = repository.join("language/self_host");
    let seed_path = self_host.join("compiler.gub");
    let seed_manifest_path = self_host.join("compiler-seed.json");
    let schema_path = repository.join("language/artifact-format.md");
    for path in [&seed_path, &seed_manifest_path, &schema_path] {
        println!("cargo:rerun-if-changed={}", path.display());
    }
    let seed = fs::read(&seed_path).unwrap_or_else(|error| {
        panic!(
            "cannot read embedded compiler seed {}: {error}",
            seed_path.display()
        )
    });
    let manifest_bytes = fs::read(&seed_manifest_path).unwrap_or_else(|error| {
        panic!(
            "cannot read compiler seed identity {}: {error}",
            seed_manifest_path.display()
        )
    });
    let manifest: Value = serde_json::from_slice(&manifest_bytes)
        .unwrap_or_else(|error| panic!("compiler seed manifest is invalid JSON: {error}"));
    let schema = fs::read(&schema_path)
        .unwrap_or_else(|error| panic!("cannot read artifact format contract: {error}"));
    let source_files = manifest["compiler_source_files"]
        .as_array()
        .unwrap_or_else(|| panic!("compiler seed manifest lacks `compiler_source_files`"));
    let mut sources = Vec::with_capacity(source_files.len());
    for source in source_files {
        let name = source
            .as_str()
            .unwrap_or_else(|| panic!("compiler source file names must be text"));
        let path = self_host.join(name);
        println!("cargo:rerun-if-changed={}", path.display());
        let bytes = fs::read(&path).unwrap_or_else(|error| {
            panic!("cannot read compiler source {}: {error}", path.display())
        });
        sources.push((name.to_owned(), bytes));
    }
    sources.sort_by(|left, right| left.0.cmp(&right.0));
    let mut source_hash = Sha256::new();
    source_hash.update((sources.len() as u64).to_be_bytes());
    for (name, bytes) in sources {
        source_hash.update((name.len() as u64).to_be_bytes());
        source_hash.update(name.as_bytes());
        source_hash.update((bytes.len() as u64).to_be_bytes());
        source_hash.update(bytes);
    }
    let source_hash = format!("{:x}", source_hash.finalize());
    let seed_hash = hex_sha256(&seed);
    let schema_hash = hex_sha256(&schema);
    let embedded_format = seed
        .get(4..8)
        .map(|bytes| u32::from_le_bytes(bytes.try_into().expect("four bytes")));
    let expected_format = manifest["artifact_format"]
        .as_u64()
        .map(|value| value as u32);
    let expected_abi = manifest["host_abi_version"].as_u64();
    let compiler_entry = manifest["compiler_entry"].as_str();
    if manifest["schema_version"].as_u64() != Some(1)
        || expected_format != embedded_format
        || expected_abi != Some(1)
        || compiler_entry != Some("compiler.gu")
        || manifest["artifact_sha256"].as_str() != Some(seed_hash.as_str())
        || manifest["compiler_source_sha256"].as_str() != Some(source_hash.as_str())
        || manifest["artifact_schema_sha256"].as_str() != Some(schema_hash.as_str())
        || !source_files
            .iter()
            .any(|source| source.as_str() == Some("compiler.gu"))
        || seed.get(..4) != Some(b"VRTA")
    {
        panic!(
            "embedded compiler seed identity mismatch; regenerate it with `cargo run --locked --offline -p guion-bootstrap --bin guion-stage0 -- seed .`"
        );
    }
}

fn compiler_build_id(repository: &Path) -> String {
    let compiler_root = repository.join("crates/compiler");
    let model_root = repository.join("crates/model");
    let mut files = vec![
        compiler_root.join("Cargo.toml"),
        model_root.join("Cargo.toml"),
        repository.join("Cargo.lock"),
    ];
    collect_rust_sources(&compiler_root.join("src"), &mut files);
    collect_rust_sources(&model_root.join("src"), &mut files);
    files.sort();

    let mut digest = Sha256::new();
    for path in files {
        println!("cargo:rerun-if-changed={}", path.display());
        let relative = path
            .strip_prefix(repository)
            .expect("compiler input is inside the workspace")
            .to_string_lossy()
            .replace('\\', "/");
        let contents = fs::read(&path).unwrap_or_else(|error| {
            panic!(
                "cannot read compiler build input {}: {error}",
                path.display()
            )
        });
        digest.update((relative.len() as u64).to_be_bytes());
        digest.update(relative.as_bytes());
        digest.update((contents.len() as u64).to_be_bytes());
        digest.update(contents);
    }
    format!("{:x}", digest.finalize())
}

fn collect_rust_sources(directory: &Path, files: &mut Vec<PathBuf>) {
    println!("cargo:rerun-if-changed={}", directory.display());
    let mut entries = fs::read_dir(directory)
        .unwrap_or_else(|error| {
            panic!(
                "cannot inspect compiler source {}: {error}",
                directory.display()
            )
        })
        .map(|entry| entry.expect("compiler source directory entry"))
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            collect_rust_sources(&path, files);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }
}

fn hex_sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
