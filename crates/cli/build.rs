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
