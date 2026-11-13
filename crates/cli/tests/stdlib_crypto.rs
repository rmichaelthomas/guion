use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct Scratch(PathBuf);
impl Scratch {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("guion-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("root")).unwrap();
        Scratch(dir)
    }
    fn root(&self) -> PathBuf {
        self.0.join("root")
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run_with_root(scratch: &Scratch, source: &str) -> Output {
    std::fs::write(scratch.0.join("main.gu"), source).unwrap();
    let root = scratch.root();
    Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", "--file-root", root.to_str().unwrap(), "main.gu"])
        .current_dir(&scratch.0)
        .output()
        .expect("guion starts")
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn write_patterned(path: &Path, bytes: usize) -> String {
    let mut file = std::fs::File::create(path).unwrap();
    let mut hasher = Sha256::new();
    let mut block = vec![0u8; 1 << 20];
    let mut written = 0usize;
    let mut counter = 0u64;
    while written < bytes {
        for chunk in block.chunks_mut(8) {
            counter = counter
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let word = counter.to_le_bytes();
            chunk.copy_from_slice(&word[..chunk.len()]);
        }
        let take = block.len().min(bytes - written);
        file.write_all(&block[..take]).unwrap();
        hasher.update(&block[..take]);
        written += take;
    }
    hex(&hasher.finalize())
}

#[test]
fn sha256_matches_the_nist_vectors() {
    let scratch = Scratch::new("sha256-vectors");
    let output = run_with_root(
        &scratch,
        r#"import "std:crypto"
import "std:encoding"
fn main() -> Int:
    assert(hex_encode(crypto_sha256(bytes_from_text(""))) == "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855")
    assert(hex_encode(crypto_sha256(bytes_from_text("abc"))) == "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
    assert(hex_encode(crypto_sha256(bytes_from_text("abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"))) == "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1")
    assert(hex_encode(crypto_sha256_text("")) == "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855")
    assert(hex_encode(crypto_sha256_text("abc")) == "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
    assert(len(bytes_to_ints(crypto_sha256(bytes_from_text("abc")))) == 32)
    return 0
"#,
    );
    assert_success(&output);
}

#[test]
fn sha256_file_matches_shasum_and_reports_missing_files() {
    let scratch = Scratch::new("sha256-file");
    let path = scratch.root().join("data.bin");
    let expected = write_patterned(&path, 3 * 1024 * 1024 + 17);
    let shasum = Command::new("shasum")
        .args(["-a", "256"])
        .arg(&path)
        .output();
    match shasum {
        Ok(result) if result.status.success() => {
            let text = String::from_utf8_lossy(&result.stdout).into_owned();
            assert_eq!(text.split_whitespace().next().unwrap(), expected);
        }
        _ => println!("notice: shasum is not available; comparing with the sha2 crate only"),
    }
    let output = run_with_root(
        &scratch,
        &format!(
            r#"import "std:crypto"
import "std:encoding"
fn main() -> Int:
    assert(hex_encode(unwrap(crypto_sha256_file("data.bin"))) == "{expected}")
    let missing = crypto_sha256_file("absent.bin")
    assert(not is_ok(missing))
    return 0
"#
        ),
    );
    assert_success(&output);
}

#[test]
fn sha256_file_streams_a_fifty_megabyte_file() {
    let scratch = Scratch::new("sha256-big");
    let path = scratch.root().join("big.bin");
    let expected = write_patterned(&path, 50 * 1024 * 1024);
    let output = run_with_root(
        &scratch,
        &format!(
            r#"import "std:crypto"
import "std:encoding"
fn main() -> Int:
    assert(hex_encode(unwrap(crypto_sha256_file("big.bin"))) == "{expected}")
    return 0
"#
        ),
    );
    assert_success(&output);
    // Memory bound: peak resident size of the same run stays well under the file size.
    #[cfg(unix)]
    if Path::new("/usr/bin/time").exists() {
        let flag = if cfg!(target_os = "macos") {
            "-l"
        } else {
            "-v"
        };
        let root = scratch.root();
        let timed = Command::new("/usr/bin/time")
            .arg(flag)
            .arg(env!("CARGO_BIN_EXE_guion"))
            .args(["run", "--file-root", root.to_str().unwrap(), "main.gu"])
            .current_dir(&scratch.0)
            .output()
            .unwrap();
        let report = String::from_utf8_lossy(&timed.stderr);
        let peak = report.lines().find_map(|line| {
            if cfg!(target_os = "macos") {
                line.trim()
                    .strip_suffix("maximum resident set size")
                    .and_then(|v| v.trim().parse::<u64>().ok())
            } else {
                line.trim()
                    .strip_prefix("Maximum resident set size (kbytes):")
                    .and_then(|v| v.trim().parse::<u64>().ok().map(|k| k * 1024))
            }
        });
        if let Some(peak) = peak {
            assert!(
                peak < 40 * 1024 * 1024,
                "peak resident size {peak} bytes is not bounded for a 50 MiB file"
            );
        }
    }
}
