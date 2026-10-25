use std::{env, fs, path::PathBuf, process::Command};

const LIBICAL_SHA256: &str = "cc09a3ac41d60e6144e644bd3fcf97d47106d659c4a0b8965102581401e67c9c";

fn main() {
    println!("cargo:rerun-if-changed=../../native/vendor/libical-4.0.5.tar.gz");
    println!("cargo:rerun-if-changed=../../native/libical_bridge.c");
    let manifest = PathBuf::from(
        env::var_os("CARGO_MANIFEST_DIR").expect("Cargo provides manifest directory"),
    );
    let archive = manifest.join("../../native/vendor/libical-4.0.5.tar.gz");
    let contents =
        fs::read(&archive).expect("pinned libical archive must be available for offline builds");
    let actual = format!("{:x}", sha2::Sha256::digest(&contents));
    assert_eq!(
        actual, LIBICAL_SHA256,
        "pinned libical archive hash mismatch"
    );
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo provides OUT_DIR"));
    let source = out.join("libical-src");
    fs::create_dir_all(&source).expect("create libical build source directory");
    run(
        Command::new("tar")
            .arg("-xzf")
            .arg(&archive)
            .arg("-C")
            .arg(&source),
        "extract libical source",
    );
    let source = source.join("libical-4.0.5");
    let build = out.join("libical-build");
    let install = out.join("libical-install");
    let mut configure = Command::new("cmake");
    configure
        .arg("-S")
        .arg(&source)
        .arg("-B")
        .arg(&build)
        .arg(format!("-DCMAKE_INSTALL_PREFIX={}", install.display()))
        .arg("-DCMAKE_INSTALL_LIBDIR=lib")
        .arg("-DCMAKE_BUILD_TYPE=Release")
        .arg("-DBUILD_SHARED_LIBS=OFF")
        .arg("-DLIBICAL_STATIC=ON")
        .arg("-DLIBICAL_CXX_BINDINGS=OFF")
        .arg("-DLIBICAL_JAVA_BINDINGS=OFF")
        .arg("-DLIBICAL_GOBJECT_INTROSPECTION=OFF")
        .arg("-DLIBICAL_GLIB=OFF")
        .arg("-DLIBICAL_BUILD_VZIC=OFF")
        .arg("-DLIBICAL_BUILD_TESTING=OFF")
        .arg("-DLIBICAL_BUILD_EXAMPLES=OFF")
        .arg("-DLIBICAL_BUILD_DOCS=OFF")
        .arg("-DLIBICAL_ENABLE_BUILTIN_TZDATA=ON")
        .arg("-DCMAKE_DISABLE_FIND_PACKAGE_ICU=TRUE");
    run(&mut configure, "configure pinned libical");
    run(
        Command::new("cmake")
            .arg("--build")
            .arg(&build)
            .arg("--target")
            .arg("install")
            .arg("--config")
            .arg("Release")
            .arg("--parallel")
            .arg("2"),
        "build pinned libical",
    );

    let include = install.join("include/libical");
    let library = install.join("lib");
    println!("cargo:rustc-link-search=native={}", library.display());
    println!("cargo:rustc-link-lib=static=ical");
    if env::var("CARGO_CFG_TARGET_FAMILY").as_deref() == Ok("unix") {
        println!("cargo:rustc-link-lib=pthread");
        println!("cargo:rustc-link-lib=m");
    }
    cc::Build::new()
        .file(manifest.join("../../native/libical_bridge.c"))
        .include(&include)
        .warnings(true)
        .compile("guion_ical_bridge");
}

fn run(command: &mut Command, action: &str) {
    let output = command
        .output()
        .unwrap_or_else(|error| panic!("could not {action}: {error}"));
    if !output.status.success() {
        panic!(
            "failed to {action}:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

use sha2::Digest;
