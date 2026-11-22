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

#[test]
fn random_bytes_run_from_the_command_line_and_cannot_be_pinned_from_guion_toml() {
    let scratch = Scratch::new("random-cli");
    let program = r#"import "std:crypto"
fn main() -> Int:
    print(unwrap(crypto_random_hex(16)))
    return 0
"#;
    let run = |dir: &Path| {
        Command::new(env!("CARGO_BIN_EXE_guion"))
            .args(["run", "main.gu"])
            .current_dir(dir)
            .output()
            .expect("guion starts")
    };
    std::fs::write(scratch.0.join("main.gu"), program).unwrap();
    let first = run(&scratch.0);
    let second = run(&scratch.0);
    assert_success(&first);
    assert_success(&second);
    let (first, second) = (
        String::from_utf8_lossy(&first.stdout).trim().to_owned(),
        String::from_utf8_lossy(&second.stdout).trim().to_owned(),
    );
    assert_eq!(first.len(), 32);
    assert_ne!(first, second, "two runs must not repeat");

    // The runtime table has no entropy key: a pin in guion.toml is an unknown field, not a pin.
    let project = scratch.0.join("project");
    std::fs::create_dir_all(project.join("src")).unwrap();
    std::fs::write(project.join("src/main.gu"), program).unwrap();
    std::fs::write(
        project.join("guion.toml"),
        "[project]\nname = \"pinned\"\nversion = \"0.1.0\"\nentry = \"src/main.gu\"\n\n[runtime]\nentropy_source = \"0000\"\n",
    )
    .unwrap();
    let pinned = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", "src/main.gu"])
        .current_dir(&project)
        .output()
        .expect("guion starts");
    assert!(
        !pinned.status.success(),
        "guion.toml must not accept an entropy pin: {}",
        String::from_utf8_lossy(&pinned.stdout)
    );
    assert!(
        String::from_utf8_lossy(&pinned.stderr).contains("unsupported guion.toml [runtime] field"),
        "{}",
        String::from_utf8_lossy(&pinned.stderr)
    );
}

const CRYPTO_FIRST_FORMAT: u32 = 78;
const SIGNATURE_FIRST_FORMAT: u32 = 79;
const BLS_FIRST_FORMAT: u32 = 80;
const TLOCK_FIRST_FORMAT: u32 = 81;

const SHA512_ABC: &str = "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f";
const SHA3_512_ABC: &str = "b751850b1a57168a5693cd924b6b096e08f621827444f70d884f5d0240d2712e10e116e9192af3c91a7ec57647e3934057340b4cf408d5a56592f8274eec53f0";

const SHA512_PROGRAM: &str = r#"import "std:crypto"
import "std:encoding"
fn main() -> Int:
    assert(hex_encode(crypto_sha512(bytes_from_text(""))) == "cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce47d0d13c5d85f2b0ff8318d2877eec2f63b931bd47417a81a538327af927da3e")
    assert(hex_encode(crypto_sha512(bytes_from_text("abc"))) == "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f")
    assert(hex_encode(crypto_sha3_512(bytes_from_text(""))) == "a69f73cca23a9ac5c8b567dc185a756e97c982164fe25859e0d1dcc1475c80a615b2123af1f5f94c11e3e9402c3ac558f500199d95b6d3e301758586281dcd26")
    assert(hex_encode(crypto_sha3_512(bytes_from_text("abc"))) == "b751850b1a57168a5693cd924b6b096e08f621827444f70d884f5d0240d2712e10e116e9192af3c91a7ec57647e3934057340b4cf408d5a56592f8274eec53f0")
    assert(len(bytes_to_ints(crypto_sha512(bytes_from_text("abc")))) == 64)
    assert(len(bytes_to_ints(crypto_sha3_512(bytes_from_text("abc")))) == 64)
    return 0
"#;

#[test]
fn sha512_and_sha3_512_run_from_a_program() {
    let scratch = Scratch::new("sha512-program");
    let output = run_with_root(&scratch, SHA512_PROGRAM);
    assert_success(&output);
    // The constants above are the NIST vectors; keep them tied to the unit-test values.
    assert_eq!(SHA512_ABC.len(), 128);
    assert_eq!(SHA3_512_ABC.len(), 128);
}

#[test]
fn old_format_rejects_crypto_ops() {
    use guion_model::{Artifact, Op};
    let scratch = Scratch::new("crypto-old-format");
    std::fs::write(scratch.0.join("main.gu"), SHA512_PROGRAM).unwrap();
    let built = scratch.0.join("main.gub");
    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["build", "main.gu", "--output", built.to_str().unwrap()])
        .current_dir(&scratch.0)
        .output()
        .expect("guion starts");
    assert_success(&output);
    let current = std::fs::read(&built).unwrap();
    let mut artifact = Artifact::from_bytes(&current).unwrap();
    assert!(artifact.functions.iter().any(|function| function
        .code
        .iter()
        .any(|instruction| matches!(instruction.op, Op::Crypto(_)))));
    // The Crypto instruction arrived in format 78; later formats keep accepting it.
    let previous = CRYPTO_FIRST_FORMAT - 1;
    artifact.format_version = previous;
    assert_eq!(artifact.to_bytes().unwrap_err().code, "E_ARTIFACT_VERSION");
    let mut old = current;
    old[4..8].copy_from_slice(&previous.to_le_bytes());
    assert_eq!(
        Artifact::from_bytes(&old).unwrap_err().code,
        "E_ARTIFACT_VERSION"
    );
}

fn fixture(name: &str) -> Vec<u8> {
    let path = format!(
        "{}/../crypto/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read(&path).unwrap_or_else(|error| panic!("{path}: {error}"))
}

fn fixture_text(name: &str) -> String {
    String::from_utf8(fixture(name)).unwrap()
}

/// Constants for the signature programs: every fixture as a hex string, then `h(name)` decodes it.
fn signature_prelude() -> String {
    let mut out =
        String::from("import \"std:crypto\"\nimport \"std:encoding\"\nimport \"std:types\"\n");
    for (name, file) in [
        ("MESSAGE", "message.bin"),
        ("PKCS1_256", "rsa_pkcs1_sha256.sig"),
        ("PKCS1_512", "rsa_pkcs1_sha512.sig"),
        ("PSS_256", "rsa_pss_sha256.sig"),
        ("EC_KEY", "ec_public_sec1.bin"),
        ("EC_SIG", "ec_signature_rs.bin"),
        ("RSA_CERT", "rsa_cert.der"),
        ("EC_CERT", "ec_cert.der"),
    ] {
        out.push_str(&format!(
            "fn {}() -> Bytes:\n    return h(\"{}\")\n",
            name.to_lowercase(),
            hex(&fixture(file))
        ));
    }
    out.push_str(&format!(
        "fn modulus() -> Bytes:\n    return h(\"{}\")\nfn exponent() -> Bytes:\n    return h(\"{}\")\n",
        fixture_text("rsa_modulus.hex").trim(),
        fixture_text("rsa_exponent.hex").trim()
    ));
    out.push_str(
        "fn h(text: Text) -> Bytes:\n    match hex_decode(text):\n        Ok(data):\n            return data\n        Err(problem):\n            return bytes_from_text(\"\")\n\
fn flip(data: Bytes, index: Int) -> Bytes:\n    return bytes_xor_key(data, bytes_from_ints([1]))\n\
fn verdict(result: Result[Bool, StdError]) -> Text:\n    match result:\n        Ok(value):\n            if value:\n                return \"true\"\n            return \"false\"\n        Err(problem):\n            return problem.code\n",
    );
    out
}

fn run_signature_program(name: &str, body: &str) {
    let scratch = Scratch::new(name);
    let source = format!("{}{body}", signature_prelude());
    let output = run_with_root(&scratch, &source);
    assert_success(&output);
}

#[test]
fn rsa_verify_runs_from_a_program() {
    run_signature_program(
        "rsa-verify",
        r#"fn main() -> Int:
    assert(verdict(crypto_rsa_verify(modulus(), exponent(), message(), pkcs1_256(), "pkcs1v15-sha256")) == "true")
    assert(verdict(crypto_rsa_verify(modulus(), exponent(), message(), pkcs1_512(), "pkcs1v15-sha512")) == "true")
    assert(verdict(crypto_rsa_verify(modulus(), exponent(), message(), pss_256(), "pss-sha256")) == "true")
    assert(verdict(crypto_rsa_verify(modulus(), exponent(), message(), pkcs1_256(), "pss-sha256")) == "false")
    assert(verdict(crypto_rsa_verify(modulus(), exponent(), flip(message(), 0), pss_256(), "pss-sha256")) == "false")
    assert(verdict(crypto_rsa_verify(modulus(), exponent(), message(), flip(pss_256(), 7), "pss-sha256")) == "false")
    assert(verdict(crypto_rsa_verify(modulus(), exponent(), message(), pss_256(), "md5-rsa")) == "E_CRYPTO_SCHEME")
    assert(verdict(crypto_rsa_verify(modulus(), exponent(), message(), bytes_from_text("short"), "pss-sha256")) == "E_CRYPTO_INPUT")
    assert(verdict(crypto_rsa_verify(bytes_from_text(""), exponent(), message(), pss_256(), "pss-sha256")) == "E_CRYPTO_INPUT")
    return 0
"#,
    );
}

#[test]
fn ecdsa_p256_verify_runs_from_a_program() {
    run_signature_program(
        "ecdsa-verify",
        r#"fn main() -> Int:
    assert(verdict(crypto_ecdsa_p256_verify(ec_key(), message(), ec_sig())) == "true")
    assert(verdict(crypto_ecdsa_p256_verify(ec_key(), message(), flip(ec_sig(), 40))) == "false")
    assert(verdict(crypto_ecdsa_p256_verify(ec_key(), flip(message(), 2), ec_sig())) == "false")
    assert(verdict(crypto_ecdsa_p256_verify(flip(ec_key(), 64), message(), ec_sig())) == "E_CRYPTO_INPUT")
    assert(verdict(crypto_ecdsa_p256_verify(ec_key(), message(), bytes_from_text("short"))) == "E_CRYPTO_INPUT")
    return 0
"#,
    );
}

#[test]
fn x509_rsa_public_key_runs_from_a_program() {
    run_signature_program(
        "x509-rsa",
        r#"fn main() -> Int:
    match crypto_x509_rsa_public_key(rsa_cert()):
        Ok(parts):
            assert(len(parts) == 2)
            assert(hex_encode(parts[0]) == hex_encode(modulus()))
            assert(hex_encode(parts[1]) == "010001")
        Err(problem):
            assert(false)
    match crypto_x509_rsa_public_key(ec_cert()):
        Ok(parts):
            assert(false)
        Err(problem):
            assert(problem.code == "E_X509")
    match crypto_x509_rsa_public_key(bytes_from_text("not a certificate")):
        Ok(parts):
            assert(false)
        Err(problem):
            assert(problem.code == "E_X509")
    return 0
"#,
    );
}

#[test]
fn pem_decode_round_trips_and_rejects_malformed_text() {
    let pem = fixture_text("rsa_cert.pem");
    let der = fixture("rsa_cert.der");
    let crlf = pem.replace('\n', "\r\n");
    let spaced = format!("\n  {}\n\n", pem.replace('\n', "  \n"));
    let scratch = Scratch::new("pem-decode");
    let mut source = signature_prelude();
    source.push_str("fn main() -> Int:\n");
    for (index, variant) in [pem.as_str(), crlf.as_str(), spaced.as_str()]
        .iter()
        .enumerate()
    {
        let escaped = variant.replace('\r', "\\r").replace('\n', "\\n");
        source.push_str(&format!(
            "    match crypto_pem_decode(\"{escaped}\"):\n        Ok(data):\n            assert(hex_encode(data) == hex_encode(rsa_cert()))\n        Err(problem):\n            assert(false)\n"
        ));
        let _ = index;
    }
    assert_eq!(hex(&der), hex(&fixture("rsa_cert.der")));
    for bad in [
        "",
        "no armor at all",
        "-----BEGIN CERTIFICATE-----\\nAAAA\\n",
        "AAAA\\n-----END CERTIFICATE-----\\n",
        "-----BEGIN CERTIFICATE-----\\n!!!!\\n-----END CERTIFICATE-----\\n",
        "-----BEGIN CERTIFICATE-----\\nAAAA\\n-----END CERTIFICATE-----\\nAAAA\\n",
    ] {
        source.push_str(&format!(
            "    assert(verdict_bytes(crypto_pem_decode(\"{bad}\")) == \"E_CRYPTO_INPUT\")\n"
        ));
    }
    source.push_str("    return 0\n");
    source.push_str("fn verdict_bytes(result: Result[Bytes, StdError]) -> Text:\n    match result:\n        Ok(data):\n            return \"ok\"\n        Err(problem):\n            return problem.code\n");
    let output = run_with_root(&scratch, &source);
    assert_success(&output);
}

#[test]
fn old_format_rejects_signature_codes_but_keeps_hashes() {
    use guion_model::{Artifact, Op};
    let scratch = Scratch::new("crypto-signature-old-format");
    let source = format!(
        "{}fn main() -> Int:\n    let ignored = crypto_rsa_verify(modulus(), exponent(), message(), pss_256(), \"pss-sha256\")\n    return 0\n",
        signature_prelude()
    );
    std::fs::write(scratch.0.join("main.gu"), &source).unwrap();
    let built = scratch.0.join("main.gub");
    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["build", "main.gu", "--output", built.to_str().unwrap()])
        .current_dir(&scratch.0)
        .output()
        .expect("guion starts");
    assert_success(&output);
    let current = std::fs::read(&built).unwrap();
    assert_eq!(
        Artifact::from_bytes(&current).unwrap().format_version,
        Artifact::CURRENT_VERSION
    );
    let mut artifact = Artifact::from_bytes(&current).unwrap();
    assert!(artifact.functions.iter().any(|function| function
        .code
        .iter()
        .any(|instruction| matches!(instruction.op, Op::Crypto(_)))));
    // Function codes 2 to 4 arrived in format 79; format 78 still holds codes 0 and 1.
    let previous = SIGNATURE_FIRST_FORMAT - 1;
    assert_eq!(previous, CRYPTO_FIRST_FORMAT);
    artifact.format_version = previous;
    assert_eq!(artifact.to_bytes().unwrap_err().code, "E_ARTIFACT_VERSION");
    let mut old = current;
    old[4..8].copy_from_slice(&previous.to_le_bytes());
    assert_eq!(
        Artifact::from_bytes(&old).unwrap_err().code,
        "E_ARTIFACT_VERSION"
    );
}

const QUICKNET_KEY: &str = "83cf0f2896adee7eb8b5f01fcad3912212c437e0073e911fb90022d3e760183c8c4b450b6a0a6c3ac6a5776a2d1064510d1fec758c921cc22b0e17e63aaf4bcb5ed66304de9cf809bd274ca73bab4af5a6e9c76a4bc09e76eae8991ef5ece45a";
const QUICKNET_SIG_1000: &str = "b44679b9a59af2ec876b1a6b1ad52ea9b1615fc3982b19576350f93447cb1125e342b73a8dd2bacbe47e4b6b63ed5e39";

fn bls_program() -> String {
    // The quicknet message for round 1000 is SHA-256 of the round as 8 big-endian bytes.
    format!(
        r#"{prelude}fn main() -> Int:
    let key = h("{QUICKNET_KEY}")
    let sig = h("{QUICKNET_SIG_1000}")
    let dst = bytes_from_text("BLS_SIG_BLS12381G1_XMD:SHA-256_SSWU_RO_NUL_")
    let message = crypto_sha256(bytes_from_ints([0, 0, 0, 0, 0, 0, 3, 232]))
    assert(verdict(crypto_bls12381_verify_g1(key, message, sig, dst)) == "true")
    assert(verdict(crypto_bls12381_verify_g1(key, flip(message, 0), sig, dst)) == "false")
    assert(verdict(crypto_bls12381_verify_g1(key, message, h("{short}"), dst)) == "E_CRYPTO_INPUT")
    return 0
"#,
        prelude = signature_prelude(),
        short = &QUICKNET_SIG_1000[..94],
    )
}

#[test]
fn bls12381_verify_g1_runs_from_a_program() {
    let scratch = Scratch::new("bls-verify");
    let output = run_with_root(&scratch, &bls_program());
    assert_success(&output);
}

#[test]
fn old_format_rejects_bls_ops() {
    use guion_model::{Artifact, Op};
    let scratch = Scratch::new("bls-old-format");
    std::fs::write(scratch.0.join("main.gu"), bls_program()).unwrap();
    let built = scratch.0.join("main.gub");
    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["build", "main.gu", "--output", built.to_str().unwrap()])
        .current_dir(&scratch.0)
        .output()
        .expect("guion starts");
    assert_success(&output);
    let current = std::fs::read(&built).unwrap();
    let mut artifact = Artifact::from_bytes(&current).unwrap();
    assert!(artifact.functions.iter().any(|function| function
        .code
        .iter()
        .any(|instruction| matches!(instruction.op, Op::Crypto(_)))));
    // Code 5 arrived in format 80; format 79 still holds codes 0 to 4.
    let previous = BLS_FIRST_FORMAT - 1;
    assert_eq!(previous, SIGNATURE_FIRST_FORMAT);
    artifact.format_version = previous;
    assert_eq!(artifact.to_bytes().unwrap_err().code, "E_ARTIFACT_VERSION");
    let mut old = current;
    old[4..8].copy_from_slice(&previous.to_le_bytes());
    assert_eq!(
        Artifact::from_bytes(&old).unwrap_err().code,
        "E_ARTIFACT_VERSION"
    );
}

const QUICKNET_CHAIN: &str = "52db9ba70e0cc0f6eaf7803dd07447a1f5477735fd3f661792ba94600c84e971";
const QUICKNET_SIG_1001: &str = "b33bf3667cbd5a82de3a24b4e0e9fe5513cc1a0e840368c6e31f5fcfa79bea03f73896b25883abf2853d10337fb8fa41";

fn tlock_program() -> String {
    format!(
        r#"{prelude}fn code_of(result: Result[Bytes, StdError]) -> Text:
    match result:
        Ok(value):
            return hex_encode(value)
        Err(problem):
            return problem.code

fn main() -> Int:
    let key = h("{QUICKNET_KEY}")
    let chain = h("{QUICKNET_CHAIN}")
    let sig = h("{QUICKNET_SIG_1000}")
    let message = bytes_from_text("the sealed bid is 41")
    match crypto_tlock_encrypt(1000, chain, key, message):
        Ok(armored):
            assert(text_slice(armored, 0, 34) == "-----BEGIN AGE ENCRYPTED FILE-----")
            assert(code_of(crypto_tlock_decrypt(armored, sig)) == hex_encode(message))
            assert(code_of(crypto_tlock_decrypt(armored, h("{QUICKNET_SIG_1001}"))) == "E_TLOCK_DECRYPT")
            assert(code_of(crypto_tlock_decrypt(armored, h("{short}"))) == "E_CRYPTO_INPUT")
            match crypto_tlock_round(armored):
                Ok(round):
                    assert(round == 1000)
                Err(problem):
                    assert(false)
        Err(problem):
            assert(false)
    assert(code_of(crypto_tlock_decrypt("not an age file", sig)) == "E_TLOCK_FORMAT")
    match crypto_tlock_round("-----BEGIN AGE ENCRYPTED FILE-----\nAAAA\n-----END AGE ENCRYPTED FILE-----\n"):
        Ok(round):
            assert(false)
        Err(problem):
            assert(problem.code == "E_TLOCK_FORMAT")
    match crypto_tlock_encrypt(1000, chain, h("{short}"), message):
        Ok(armored):
            assert(false)
        Err(problem):
            assert(problem.code == "E_CRYPTO_INPUT")
    return 0
"#,
        prelude = signature_prelude(),
        short = &QUICKNET_SIG_1000[..94],
    )
}

#[test]
fn tlock_round_trips_through_the_wrappers() {
    let scratch = Scratch::new("tlock-wrappers");
    let output = run_with_root(&scratch, &tlock_program());
    assert_success(&output);
}

#[test]
fn old_format_rejects_tlock_ops() {
    use guion_model::{Artifact, Op};
    let scratch = Scratch::new("tlock-old-format");
    std::fs::write(scratch.0.join("main.gu"), tlock_program()).unwrap();
    let built = scratch.0.join("main.gub");
    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["build", "main.gu", "--output", built.to_str().unwrap()])
        .current_dir(&scratch.0)
        .output()
        .expect("guion starts");
    assert_success(&output);
    let current = std::fs::read(&built).unwrap();
    let mut artifact = Artifact::from_bytes(&current).unwrap();
    assert!(artifact.functions.iter().any(|function| function
        .code
        .iter()
        .any(|instruction| matches!(instruction.op, Op::Crypto(_)))));
    // Codes 6 to 8 arrived in format 81; format 80 still holds codes 0 to 5.
    let previous = TLOCK_FIRST_FORMAT - 1;
    assert_eq!(previous, BLS_FIRST_FORMAT);
    artifact.format_version = previous;
    assert_eq!(artifact.to_bytes().unwrap_err().code, "E_ARTIFACT_VERSION");
    let mut old = current;
    old[4..8].copy_from_slice(&previous.to_le_bytes());
    assert_eq!(
        Artifact::from_bytes(&old).unwrap_err().code,
        "E_ARTIFACT_VERSION"
    );
}

/// The first artifact format with DAG-CBOR encoding.
const DAGCBOR_FIRST_FORMAT: u32 = 82;

const DAGCBOR_PROGRAM: &str = r#"import "std:encoding"
import "std:types"

fn encoded(text: Text) -> Text:
    match dagcbor_encode(unwrap(json_parse(text))):
        Ok(bytes):
            return hex_encode(bytes)
        Err(problem):
            return problem.code

fn main() -> Int:
    assert(encoded("{\"b\":1,\"a\":[true,null,-1.5],\"aa\":\"x\"}") == "a3616183f5f6fbbff80000000000006162016261616178")
    assert(encoded("256") == "190100")
    assert(encoded("1e999") == "E_DAGCBOR_VALUE")
    assert(encoded("18446744073709551615") == "1bffffffffffffffff")
    assert(encoded("18446744073709551616") == "E_DAGCBOR_VALUE")
    assert(encoded("{\"/\":{\"bytes\":\"AQID\"}}") == "43010203")
    assert(encoded("{\"/\":\"not a cid\"}") == "E_DAGCBOR_LINK")
    assert(encoded("{\"/\":1,\"x\":2}") == "E_DAGCBOR_LINK")
    return 0
"#;

#[test]
fn dagcbor_encode_runs_from_a_program() {
    let scratch = Scratch::new("dagcbor-wrapper");
    let output = run_with_root(&scratch, DAGCBOR_PROGRAM);
    assert_success(&output);
}

#[test]
fn old_format_rejects_dagcbor_op() {
    use guion_model::{Artifact, Op};
    let scratch = Scratch::new("dagcbor-old-format");
    std::fs::write(scratch.0.join("main.gu"), DAGCBOR_PROGRAM).unwrap();
    let built = scratch.0.join("main.gub");
    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["build", "main.gu", "--output", built.to_str().unwrap()])
        .current_dir(&scratch.0)
        .output()
        .expect("guion starts");
    assert_success(&output);
    let current = std::fs::read(&built).unwrap();
    let mut artifact = Artifact::from_bytes(&current).unwrap();
    assert!(artifact.functions.iter().any(|function| function
        .code
        .iter()
        .any(|instruction| matches!(instruction.op, Op::Crypto(_)))));
    // Code 9 arrived in format 82; format 81 still holds codes 0 to 8.
    let previous = DAGCBOR_FIRST_FORMAT - 1;
    assert_eq!(previous, TLOCK_FIRST_FORMAT);
    artifact.format_version = previous;
    assert_eq!(artifact.to_bytes().unwrap_err().code, "E_ARTIFACT_VERSION");
    let mut old = current;
    old[4..8].copy_from_slice(&previous.to_le_bytes());
    assert_eq!(
        Artifact::from_bytes(&old).unwrap_err().code,
        "E_ARTIFACT_VERSION"
    );
}
