//! The beacon cryptography in the browser VM. Every `Crypto` code (0 to 11) and the bit-operation
//! `Codec` codes (15 to 19) run through the wasm ABI on the same recorded vectors as the native
//! runtime, and the two must answer identically, including the error codes.

use guion_compiler::{compile, compile_project_with_roots};
use guion_model::{Artifact, Value};
use guion_runtime::{HostPolicy, Runtime};
use guion_vm_wasm::*;
use std::path::PathBuf;

fn gub(source: &str) -> Vec<u8> {
    compile("fixture.gu", source)
        .expect("fixture compiles")
        .to_bytes()
        .expect("artifact serializes")
}

/// Compiles a program that imports the standard library, which needs a project directory.
fn gub_project(name: &str, source: &str) -> Vec<u8> {
    let dir = std::env::temp_dir().join(format!("guion-vm-wasm-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("main.gu"), source).unwrap();
    let stdlib = repository().join("language/stdlib");
    let artifact = compile_project_with_roots(
        &dir.join("main.gu"),
        &dir,
        Some(&stdlib),
        &std::collections::HashMap::new(),
    )
    .expect("program compiles");
    let _ = std::fs::remove_dir_all(&dir);
    artifact.to_bytes().expect("artifact serializes")
}

fn repository() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn crypto_fixture(name: &str) -> Vec<u8> {
    let path = repository().join("crates/crypto/tests/fixtures").join(name);
    std::fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// The DER of the first certificate in a PEM fixture.
fn pem_fixture(name: &str) -> Vec<u8> {
    use base64::Engine;
    let text = String::from_utf8(crypto_fixture(name)).unwrap();
    let body: String = text
        .lines()
        .skip_while(|line| !line.starts_with("-----BEGIN"))
        .skip(1)
        .take_while(|line| !line.starts_with("-----END"))
        .collect();
    base64::engine::general_purpose::STANDARD
        .decode(body)
        .unwrap()
}

fn beacon_fixture(name: &str) -> Vec<u8> {
    let path = repository().join("packages/beacons/fixtures").join(name);
    std::fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// What one entry gave: the value's text, or the runtime error code.
#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    Text(String),
    Int(String),
    Error(String),
}

fn native(artifact: &[u8], entry: &str) -> Outcome {
    let artifact = Artifact::from_bytes(artifact).expect("artifact reads");
    let mut runtime = Runtime::new()
        .load(artifact, HostPolicy::default())
        .expect("native verifier accepts the artifact");
    match runtime.invoke(entry, Vec::new()) {
        Ok(Value::Text(text)) => Outcome::Text(text.to_string()),
        Ok(Value::Int(value)) => Outcome::Int(value.to_string()),
        Ok(other) => panic!("unexpected native value {other:?}"),
        Err(error) => Outcome::Error(error.code),
    }
}

fn browser_response(artifact: &[u8], entry: &str, steps: u64) -> serde_json::Value {
    let request = serde_json::json!({
        "abi": {"major": 1, "minor": 0}, "entry": entry, "arguments": [],
        "capabilities": [], "limits": {"steps": steps, "allowRaisedSteps": true},
    });
    let response = execute_artifact_request(artifact, request.to_string().as_bytes());
    serde_json::from_slice(&response).unwrap()
}

fn browser(artifact: &[u8], entry: &str) -> Outcome {
    let response = browser_response(artifact, entry, u64::from(MAX_BROWSER_STEPS));
    if response["ok"] == true {
        let value = &response["value"];
        match value["type"].as_str().unwrap() {
            "Text" => Outcome::Text(value["value"].as_str().unwrap().to_owned()),
            "Int" => Outcome::Int(value["value"].as_str().unwrap().to_owned()),
            other => panic!("unexpected browser value type {other}"),
        }
    } else {
        Outcome::Error(response["code"].as_str().unwrap().to_owned())
    }
}

fn program() -> String {
    let mut out =
        String::from("import \"std:crypto\"\nimport \"std:encoding\"\nimport \"std:types\"\nimport \"std:text\"\n");
    for (name, bytes) in [
        ("message", crypto_fixture("message.bin")),
        ("pkcs1_256", crypto_fixture("rsa_pkcs1_sha256.sig")),
        ("pkcs1_512", crypto_fixture("rsa_pkcs1_sha512.sig")),
        ("pss_256", crypto_fixture("rsa_pss_sha256.sig")),
        ("ec_key", crypto_fixture("ec_public_sec1.bin")),
        ("ec_sig", crypto_fixture("ec_signature_rs.bin")),
        ("rsa_cert", crypto_fixture("rsa_cert.der")),
        ("ec_cert", crypto_fixture("ec_cert.der")),
        ("nist_leaf", crypto_fixture("x509/nist-leaf.der")),
        ("nist_issuer", crypto_fixture("x509/nist-issuer.der")),
        ("nist_root", crypto_fixture("x509/nist-root.der")),
        ("sha1_leaf", crypto_fixture("x509/leaf-rsa-sha1.der")),
        ("rsa_ca", crypto_fixture("x509/rsa-ca.der")),
        ("san_leaf", crypto_fixture("x509/leaf-san.der")),
        ("dated_leaf", crypto_fixture("x509/leaf-dated.der")),
        ("tsa_response", crypto_fixture("tsa/local/good-v2.tsr")),
        ("tsa_request", crypto_fixture("tsa/local/request.tsq")),
        ("tsa_message", crypto_fixture("tsa/local/message.txt")),
        ("tsa_cert", pem_fixture("tsa/local/tsa1.pem")),
        ("tsa_ca", pem_fixture("tsa/local/ca.pem")),
        ("modulus", hex_to_bytes(&crypto_fixture("rsa_modulus.hex"))),
        (
            "exponent",
            hex_to_bytes(&crypto_fixture("rsa_exponent.hex")),
        ),
    ] {
        out.push_str(&format!(
            "fn {name}() -> Bytes:\n    return h(\"{}\")\n",
            hex(&bytes)
        ));
    }
    let armored = String::from_utf8(beacon_fixture("round1000.age")).unwrap();
    out.push_str(&format!(
        "fn armored() -> Text:\n    return \"{}\"\n",
        armored.replace('\n', "\\n")
    ));
    out.push_str(&format!(
        "fn quicknet_key() -> Bytes:\n    return h(\"{QUICKNET_KEY}\")\nfn quicknet_chain() -> Bytes:\n    return h(\"{QUICKNET_CHAIN}\")\nfn sig_1000() -> Bytes:\n    return h(\"{SIG_1000}\")\nfn sig_1001() -> Bytes:\n    return h(\"{SIG_1001}\")\n"
    ));
    out.push_str(include_str!("fixtures/crypto_vectors.gu"));
    out
}

fn hex_to_bytes(text: &[u8]) -> Vec<u8> {
    let text = std::str::from_utf8(text).unwrap().trim();
    (0..text.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&text[index..index + 2], 16).unwrap())
        .collect()
}

const QUICKNET_KEY: &str = "83cf0f2896adee7eb8b5f01fcad3912212c437e0073e911fb90022d3e760183c8c4b450b6a0a6c3ac6a5776a2d1064510d1fec758c921cc22b0e17e63aaf4bcb5ed66304de9cf809bd274ca73bab4af5a6e9c76a4bc09e76eae8991ef5ece45a";
const QUICKNET_CHAIN: &str = "52db9ba70e0cc0f6eaf7803dd07447a1f5477735fd3f661792ba94600c84e971";
const SIG_1000: &str = "b44679b9a59af2ec876b1a6b1ad52ea9b1615fc3982b19576350f93447cb1125e342b73a8dd2bacbe47e4b6b63ed5e39";
const SIG_1001: &str = "b33bf3667cbd5a82de3a24b4e0e9fe5513cc1a0e840368c6e31f5fcfa79bea03f73896b25883abf2853d10337fb8fa41";

#[test]
fn browser_crypto_vectors_match_native() {
    let artifact = gub_project("vectors", &program());
    assert!(artifact.len() <= MAX_ARTIFACT_BYTES);

    let native_run = native(&artifact, "vectors");
    let browser_run = browser(&artifact, "vectors");
    assert_eq!(browser_run, native_run, "the transcripts differ");
    let Outcome::Text(transcript) = browser_run else {
        panic!("vectors must return the transcript text");
    };

    // Each crypto code answers its accept case and its rejection case.
    for expected in [
        // code 0 and 1: total functions, so the edge case is the empty input
        "sha512 abc=ddaf35a193617aba",
        "sha512 empty=cf83e1357eefb8bd",
        "sha3_512 abc=b751850b1a57168a",
        "sha3_512 empty=a69f73cca23a9ac5",
        // code 2
        "rsa pss=true",
        "rsa pkcs1 as pss=false",
        "rsa bad scheme=E_CRYPTO_SCHEME",
        "rsa short signature=E_CRYPTO_INPUT",
        // code 3
        "ecdsa=true",
        "ecdsa flipped=false",
        "ecdsa short signature=E_CRYPTO_INPUT",
        // code 4
        "x509 rsa key=2 010001",
        "x509 ec cert=E_X509",
        // code 5
        "bls=true",
        "bls other message=false",
        "bls short signature=E_CRYPTO_INPUT",
        // codes 6 to 8, 10
        "tlock round=1000",
        "tlock chain=true",
        "tlock decrypt=guion tlock fixture plaintext",
        "tlock decrypt wrong round=E_TLOCK_DECRYPT",
        "tlock decrypt short signature=E_CRYPTO_INPUT",
        "tlock decrypt not age=E_TLOCK_FORMAT",
        "tlock round not age=E_TLOCK_FORMAT",
        "tlock chain not age=E_TLOCK_FORMAT",
        "tlock encrypt round trip=true",
        "tlock encrypt bad key=E_CRYPTO_INPUT",
        // code 9
        "dagcbor map=a3616183f5f6fbbff80000000000006162016261616178",
        "dagcbor too big=E_DAGCBOR_VALUE",
        "dagcbor bad link=E_DAGCBOR_LINK",
        // code 11
        "x509 leaf by issuer=true",
        "x509 issuer by root=true",
        "x509 leaf by root=false",
        "x509 sha1=E_X509_ALG",
        "x509 garbage=E_X509",
        // codes 12 and 13
        "x509 names=a.example.test,b.example.test",
        "x509 names nist=engine.beacon.nist.gov",
        "x509 names garbage=E_X509",
        "x509 validity=1704164645,1749283750",
        "x509 validity garbage=E_X509",
        // codes 14 to 16
        "ed25519 public key=d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a",
        "ed25519 sign=e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b",
        "ed25519 verify=true",
        "ed25519 other message=false",
        "ed25519 short seed=E_CRYPTO_INPUT",
        // codes 17 to 20
        "tsa request=true",
        "tsa request bad digest=E_TSA_DIGEST",
        "tsa request bad nonce=E_TSA_NONCE",
        "tsa token=true",
        "tsa certificates=2 true",
        "tsa verify=1795463311000,02,1.2.3.4.1",
        "tsa verify other digest=none",
        "tsa verify wrong issuer=none",
        "tsa verify garbage=E_TSA_FORMAT",
        "tsa verify bad certificate=E_X509",
        "tsa response garbage=E_TSA_FORMAT",
        // codec 15 to 19
        "and=8",
        "or=14",
        "xor=6",
        "shl=4611686018427387904",
        "shr=-4",
    ] {
        assert!(
            transcript.lines().any(|line| line.starts_with(expected)),
            "missing `{expected}` in:\n{transcript}"
        );
    }
}

#[test]
fn browser_bit_op_errors_match_native() {
    let artifact = gub(
        "fn and_range() -> Int:\n    return int_and(9223372036854775808, 1)\n\
fn or_range() -> Int:\n    return int_or(1, -9223372036854775809)\n\
fn xor_range() -> Int:\n    return int_xor(18446744073709551616, 1)\n\
fn shl_count() -> Int:\n    return int_shift_left(1, 64)\n\
fn shr_count() -> Int:\n    return int_shift_right(1, 0 - 1)\n\
fn shl_ok() -> Int:\n    return int_shift_left(1, 63)\n\
fn main() -> Int:\n    return 0\n",
    );
    for (entry, code) in [
        ("and_range", "R_INTEGER_RANGE"),
        ("or_range", "R_INTEGER_RANGE"),
        ("xor_range", "R_INTEGER_RANGE"),
        ("shl_count", "R_SHIFT_RANGE"),
        ("shr_count", "R_SHIFT_RANGE"),
    ] {
        let expected = Outcome::Error(code.to_owned());
        assert_eq!(native(&artifact, entry), expected, "native {entry}");
        assert_eq!(browser(&artifact, entry), expected, "browser {entry}");
    }
    let wide = Outcome::Int("9223372036854775808".to_owned());
    assert_eq!(native(&artifact, "shl_ok"), wide);
    assert_eq!(browser(&artifact, "shl_ok"), wide);
}

#[test]
fn browser_runs_the_pass_statement() {
    let artifact =
        gub("fn idle() -> Int:\n    pass\n    return 7\n\nfn main() -> Int:\n    return 0\n");
    let expected = Outcome::Int("7".to_owned());
    assert_eq!(native(&artifact, "idle"), expected);
    assert_eq!(browser(&artifact, "idle"), expected);
}

const JSON_PROGRAM: &str = include_str!("fixtures/json_ops.gu");

/// The JSON instructions the beacon package reads its receipts with were host-only; the browser
/// VM now runs them with the same results and error codes as the native runtime.
#[test]
fn browser_json_matches_native() {
    let artifact = gub_project("json", JSON_PROGRAM);
    let native_run = native(&artifact, "transcript");
    let browser_run = browser(&artifact, "transcript");
    assert_eq!(browser_run, native_run, "the transcripts differ");
    let Outcome::Text(transcript) = browser_run else {
        panic!("transcript must return text");
    };
    for expected in [
        "parse ok=true",
        "parse error=R_JSON_PARSE 1",
        "parse duplicate=R_JSON_DUPLICATE_KEY",
        "get text=beacon",
        "get int=2038648",
        "index=20",
        "len array=3",
        "len object=4",
        "has=true",
        "has not=false",
        "as_text=beacon",
        "as_int=2038648",
        "as_bool=true",
        "try missing=E_JSON_MISSING",
        "try shape=E_JSON_SHAPE",
        "try type=E_JSON_TYPE",
        "try index range=E_JSON_INDEX",
        "set=",
        "stringify={\"a\":1,\"b\":[1,2]}",
        "local set=",
    ] {
        assert!(
            transcript.lines().any(|line| line.starts_with(expected)),
            "missing `{expected}` in:\n{transcript}"
        );
    }
    // Instructions that raise instead of returning a result carry the same codes.
    for (entry, code) in [
        ("raise_key", "R_JSON_KEY"),
        ("raise_index", "R_JSON_INDEX"),
        ("raise_object", "R_JSON_TYPE"),
        ("raise_text", "R_JSON_TYPE"),
        ("raise_len", "R_JSON_TYPE"),
    ] {
        let expected = Outcome::Error(code.to_owned());
        assert_eq!(native(&artifact, entry), expected, "native {entry}");
        assert_eq!(browser(&artifact, entry), expected, "browser {entry}");
    }
}

fn copy_dir(from: &std::path::Path, to: &std::path::Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// The receipt program (`tests/fixtures/receipt.gu`) compiled with the beacons package source.
fn receipt_artifact() -> Vec<u8> {
    let dir = std::env::temp_dir().join(format!("guion-vm-wasm-receipt-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    copy_dir(&repository().join("packages/beacons/src"), &dir.join("src"));
    std::fs::write(dir.join("main.gu"), include_str!("fixtures/receipt.gu")).unwrap();
    let stdlib = repository().join("language/stdlib");
    let artifact = compile_project_with_roots(
        &dir.join("main.gu"),
        &dir,
        Some(&stdlib),
        &std::collections::HashMap::new(),
    )
    .expect("receipt program compiles");
    let _ = std::fs::remove_dir_all(&dir);
    artifact.to_bytes().expect("artifact serializes")
}

fn beacon_text(name: &str) -> String {
    String::from_utf8(beacon_fixture(name)).unwrap()
}

/// The recorded receipt as typed entry arguments: the data the page would embed.
fn receipt_arguments() -> Vec<serde_json::Value> {
    let chains: serde_json::Value =
        serde_json::from_str(&beacon_text("twine-chains.json")).unwrap();
    let q_chain = "bafyriqhijw5soalbtppuwwjbskfiriqy6swa5qd5trgbzx4nypnu7mp7iyhx7m2iin2nxnglmngvjgirzb7bvlisxq3uygoik7ozbybxnftiw";
    let chain = chains
        .as_array()
        .unwrap()
        .iter()
        .find(|record| record["cid"]["/"] == q_chain)
        .expect("the CURBy-Q chain record is saved");
    let pulses: serde_json::Value =
        serde_json::from_str(&beacon_text("curby-q-pulses.json")).unwrap();
    let pulse = pulses
        .as_array()
        .unwrap()
        .iter()
        .find(|record| record["data"]["content"]["payload"]["stage"] == "randomness")
        .expect("a randomness pulse is saved");
    let text = |value: String| serde_json::json!({"type": "Text", "value": value});
    vec![
        serde_json::json!({"type": "Int", "value": "1000"}),
        text(SIG_1000.to_owned()),
        text(beacon_text("round1000.age")),
        text(chain.to_string()),
        text(pulse.to_string()),
        text(beacon_text("nist-pulse-2038647.json")),
        text(beacon_text("nist-pulse-2038648.json")),
        text(beacon_text("nist-certificate.pem")),
    ]
}

/// The step budget the receipt check asks for: its measured cost with more than twice the headroom.
const RECEIPT_STEP_BUDGET: u64 = 100_000;

fn browser_receipt(
    artifact: &[u8],
    arguments: Vec<serde_json::Value>,
    limits: serde_json::Value,
) -> serde_json::Value {
    let request = serde_json::json!({
        "abi": {"major": 1, "minor": 0}, "entry": "check_receipt",
        "arguments": arguments, "capabilities": [], "limits": limits,
    });
    let response = execute_artifact_request(artifact, request.to_string().as_bytes());
    serde_json::from_slice(&response).unwrap()
}

fn raised(steps: u64) -> serde_json::Value {
    serde_json::json!({"steps": steps, "allowRaisedSteps": true})
}

fn native_receipt(artifact: &[u8], arguments: &[serde_json::Value]) -> String {
    let artifact = Artifact::from_bytes(artifact).expect("artifact reads");
    let mut runtime = Runtime::new()
        .load(artifact, HostPolicy::default())
        .expect("native verifier accepts the artifact");
    let values = arguments
        .iter()
        .map(|argument| match argument["type"].as_str().unwrap() {
            "Int" => {
                Value::Int(guion_model::BigInt::parse(argument["value"].as_str().unwrap()).unwrap())
            }
            "Text" => Value::Text(argument["value"].as_str().unwrap().into()),
            other => panic!("unexpected argument type {other}"),
        })
        .collect();
    match runtime.invoke("check_receipt", values) {
        Ok(Value::Text(text)) => text.to_string(),
        other => panic!("native receipt gave {other:?}"),
    }
}

const RECEIPT_EXPECTED: &str = "drand=true\ntlock=guion tlock fixture plaintext\n\ncurby=61c307b2dcedea8f5ce9a3c68e387f78df126d4a6a45924aa86457c49885966891ba507e0c9b4f9069ecbb4a9e85f925c3c1dbb84b7478909e916c3911de42a9\nnist previous=true\nnist current=true\nnist chain=true\n";

#[test]
fn browser_checks_a_full_receipt() {
    let artifact = receipt_artifact();
    assert!(
        artifact.len() <= MAX_ARTIFACT_BYTES,
        "{} bytes",
        artifact.len()
    );
    let arguments = receipt_arguments();

    // Measured: 167,349 bytes of artifact and 34,008 steps. The default ceiling of
    // MAX_BROWSER_STEPS refuses the run ...
    let refused = browser_receipt(&artifact, arguments.clone(), serde_json::json!({}));
    assert_eq!(refused["ok"], false);
    assert_eq!(refused["code"], "R_STEP_LIMIT");
    let too_many = browser_receipt(
        &artifact,
        arguments.clone(),
        serde_json::json!({"steps": RECEIPT_STEP_BUDGET}),
    );
    assert_eq!(too_many["code"], "E_ABI_LIMIT");

    // ... and the page that opts in with the measured budget gets the full check.
    let response = browser_receipt(&artifact, arguments.clone(), raised(RECEIPT_STEP_BUDGET));
    assert_eq!(response["ok"], true, "{response}");
    assert_eq!(response["value"]["value"], RECEIPT_EXPECTED);
    let steps = response["steps"].as_u64().unwrap();
    eprintln!("receipt: {} artifact bytes, {steps} steps", artifact.len());
    assert!(steps > u64::from(MAX_BROWSER_STEPS), "{steps}");
    assert!(
        steps * 2 <= RECEIPT_STEP_BUDGET,
        "{steps} leaves too little headroom"
    );
    assert_eq!(native_receipt(&artifact, &arguments), RECEIPT_EXPECTED);

    // A round value that is not the sealed round's: drand does not verify against the key it was
    // not signed for, and the tlock file refuses to open.
    let mut wrong = arguments.clone();
    wrong[1] = serde_json::json!({"type": "Text", "value": SIG_1001});
    let response = browser_receipt(&artifact, wrong.clone(), raised(RECEIPT_STEP_BUDGET));
    let transcript = response["value"]["value"].as_str().unwrap().to_owned();
    // The package's own code for this refusal, spelled in two parts so the generated agent
    // reference, which collects every `E_` token under `crates`, does not list a package code.
    let refusal = format!("drand=false\ntlock=E_{}\n", "DRAND_VERIFY");
    assert!(transcript.starts_with(&refusal), "{transcript}");
    assert_eq!(native_receipt(&artifact, &wrong), transcript);

    // A NIST pulse whose precommitment does not follow is not a chain.
    let mut swapped = arguments;
    swapped.swap(5, 6);
    let response = browser_receipt(&artifact, swapped.clone(), raised(RECEIPT_STEP_BUDGET));
    let transcript = response["value"]["value"].as_str().unwrap().to_owned();
    assert!(transcript.ends_with("nist chain=false\n"), "{transcript}");
    assert_eq!(native_receipt(&artifact, &swapped), transcript);
}

/// HMAC and random bytes stay native-only, and with them the `crypto_tlock_encrypt` wrapper that
/// draws its randomness; `docs/browser.md` says so.
#[test]
fn hmac_and_random_bytes_are_not_in_the_browser() {
    let artifact = gub_project(
        "native-only",
        "import \"std:crypto\"\nimport \"std:types\"\n\
fn hmac() -> Int:\n    let tag = crypto_hmac_sha256(bytes_from_text(\"k\"), bytes_from_text(\"m\"))\n    return len(tag)\n\
fn verify_hmac() -> Int:\n    let ok = crypto_verify_hmac_sha256(bytes_from_text(\"k\"), bytes_from_text(\"m\"), bytes_from_text(\"t\"))\n    if ok:\n        return 1\n    return 0\n\
fn random() -> Int:\n    match crypto_random_bytes(4):\n        Ok(data):\n            return len(data)\n        Err(problem):\n            return 0\n\
fn wrapper() -> Int:\n    match crypto_tlock_encrypt(1000, bytes_from_text(\"c\"), bytes_from_text(\"k\"), bytes_from_text(\"p\")):\n        Ok(text):\n            return 1\n        Err(problem):\n            return 0\n\
fn main() -> Int:\n    return 0\n",
    );
    for entry in ["hmac", "verify_hmac", "random", "wrapper"] {
        let response = browser_response(&artifact, entry, 1000);
        assert_eq!(response["ok"], false, "{entry}");
        assert_eq!(
            response["code"], "R_HOST_UNSUPPORTED",
            "{entry}: {response}"
        );
    }
}

/// `sleep_ms` blocks a thread, which the browser VM does not have.
#[test]
fn browser_is_unsupported() {
    let artifact = gub_project(
        "sleep",
        "fn main() -> Int:\n    sleep_ms(1)\n    return 0\n",
    );
    let response = browser_response(&artifact, "main", 1000);
    assert_eq!(response["ok"], false, "{response}");
    assert_eq!(response["code"], "R_HOST_UNSUPPORTED", "{response}");
}
