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
fn browser_bytes_index_and_nested_match_match_native() {
    let artifact = gub(
        "fn data() -> Bytes:\n    return bytes_from_ints([0, 7, 255])\n\
fn sum() -> Int:\n    let d = data()\n    return d[0] + d[1] + d[2]\n\
fn past() -> Int:\n    return data()[3]\n\
fn negative() -> Int:\n    return data()[0 - 1]\n\
fn look(n: Int) -> Result[Option[Int], Text]:\n    if n < 0:\n        return Err(\"low\")\n    return Ok(half(n))\n\
fn half(n: Int) -> Option[Int]:\n    if n == 0:\n        return None\n    return Some(n // 2)\n\
fn label(n: Int) -> Text:\n    match look(n):\n        Ok(Some(x)):\n            return int_to_text(x)\n        Ok(None):\n            return \"none\"\n        Err(e):\n            return e\n\
fn labels() -> Text:\n    return label(8) + \",\" + label(0) + \",\" + label(0 - 1)\n\
fn main() -> Int:\n    return 0\n",
    );
    for (entry, expected) in [
        ("sum", Outcome::Int("262".to_owned())),
        ("past", Outcome::Error("R_INDEX_BOUNDS".to_owned())),
        ("negative", Outcome::Error("R_INDEX_INVALID".to_owned())),
        ("labels", Outcome::Text("4,none,low".to_owned())),
    ] {
        assert_eq!(native(&artifact, entry), expected, "native {entry}");
        assert_eq!(browser(&artifact, entry), expected, "browser {entry}");
    }
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

/// Random bytes stay native-only, and with them the `crypto_tlock_encrypt` wrapper that draws its
/// randomness; `docs/browser.md` says so. (HMAC runs in the browser; see `pure_ops.rs`.)
#[test]
fn random_bytes_are_not_in_the_browser() {
    let artifact = gub_project(
        "native-only",
        "import \"std:crypto\"\nimport \"std:types\"\n\
fn random() -> Int:\n    match crypto_random_bytes(4):\n        Ok(data):\n            return len(data)\n        Err(problem):\n            return 0\n\
fn wrapper() -> Int:\n    match crypto_tlock_encrypt(1000, bytes_from_text(\"c\"), bytes_from_text(\"k\"), bytes_from_text(\"p\")):\n        Ok(text):\n            return 1\n        Err(problem):\n            return 0\n\
fn main() -> Int:\n    return 0\n",
    );
    for entry in ["random", "wrapper"] {
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

// ---- Ed25519 and RFC 3161 timestamps: every Task 1 and Task 2 vector, browser against native ----

fn json_fixture(name: &str) -> Vec<serde_json::Value> {
    serde_json::from_slice(&crypto_fixture(name)).unwrap()
}

fn field(case: &serde_json::Value, name: &str) -> Vec<u8> {
    hex_to_bytes(case[name].as_str().unwrap().as_bytes())
}

/// What a `Result[Bool, _]` becomes in the transcript.
fn flag_text(result: Result<bool, guion_crypto::CryptoError>) -> String {
    match result {
        Ok(value) => value.to_string(),
        Err(error) => error.code.to_owned(),
    }
}

fn bytes_text(result: Result<impl AsRef<[u8]>, guion_crypto::CryptoError>) -> String {
    match result {
        Ok(value) => hex(value.as_ref()),
        Err(error) => error.code.to_owned(),
    }
}

/// One timestamp case: a response, the message whose digest it must stamp, and the certificates.
struct TsaCase {
    name: &'static str,
    response: &'static str,
    message: &'static str,
    tsa: &'static str,
    issuer: &'static str,
}

const fn tsa_case(
    name: &'static str,
    response: &'static str,
    message: &'static str,
    tsa: &'static str,
    issuer: &'static str,
) -> TsaCase {
    TsaCase {
        name,
        response,
        message,
        tsa,
        issuer,
    }
}

fn tsa_cases() -> Vec<TsaCase> {
    let (m, m2, t1, ca) = ("message.txt", "message2.txt", "tsa1.pem", "ca.pem");
    vec![
        tsa_case("good v2", "good-v2.tsr", m, t1, ca),
        tsa_case("good v1 ess", "good-v1.tsr", m, t1, ca),
        tsa_case("rebuilt", "rebuilt.tsr", m, t1, ca),
        tsa_case("accuracy", "accuracy.tsr", m, t1, ca),
        tsa_case("six digit fraction", "precision6.tsr", m, t1, ca),
        tsa_case("rsa sha384", "rsa-sha384.tsr", m, t1, ca),
        tsa_case("rsa sha512", "rsa-sha512.tsr", m, t1, ca),
        tsa_case("pss", "pss.tsr", m, t1, ca),
        tsa_case("p256", "p256.tsr", m, "tsa-p256.pem", ca),
        tsa_case("p384", "p384.tsr", m, "tsa-p384.pem", ca),
        tsa_case("other digest", "other-digest.tsr", m, t1, ca),
        tsa_case("good token, other message", "good-v2.tsr", m2, t1, ca),
        tsa_case("sha256 imprint", "sha256-imprint.tsr", m, t1, ca),
        tsa_case("second tsa certificate", "good-v2.tsr", m, "tsa2.pem", ca),
        tsa_case("second tsa token", "tsa2.tsr", m, t1, ca),
        tsa_case("other issuer", "good-v2.tsr", m, t1, "other-ca.pem"),
        tsa_case("tsa as issuer", "good-v2.tsr", m, t1, t1),
        tsa_case("no eku", "noeku.tsr", m, "tsa-noeku.pem", ca),
        tsa_case("expired tsa", "expired.tsr", m, "tsa-expired.pem", ca),
        tsa_case("ess mismatch", "ess-mismatch.tsr", m, t1, ca),
        tsa_case("no ess", "no-ess.tsr", m, t1, ca),
        tsa_case("sha1 signer", "sha1-signer.tsr", m, t1, ca),
        tsa_case("p256 token, rsa certificate", "p256.tsr", m, t1, ca),
        tsa_case(
            "rsa token, p256 certificate",
            "good-v2.tsr",
            m,
            "tsa-p256.pem",
            ca,
        ),
        tsa_case("rejection", "rejection.tsr", m, t1, ca),
    ]
}

/// What `tsa_verify` gives, as the transcript spells it.
fn tsa_text(response: &[u8], digest: &[u8], tsa: &[u8], issuer: &[u8]) -> String {
    let token = match guion_crypto::tsa_response_token(response) {
        Ok(token) => token,
        Err(error) => return error.code.to_owned(),
    };
    match guion_crypto::tsa_verify(&token, digest, tsa, issuer) {
        Ok(Some(stamp)) => format!(
            "{},{},{},{}",
            stamp.gen_time_ms,
            hex(&stamp.serial),
            stamp.policy,
            stamp.accuracy_ms
        ),
        Ok(None) => "none".to_owned(),
        Err(error) => error.code.to_owned(),
    }
}

fn ed25519_and_tsa_program(cases: &[TsaCase]) -> (String, Vec<String>) {
    use std::collections::BTreeSet;
    let mut out = String::from(
        "import \"std:crypto\"\nimport \"std:encoding\"\nimport \"std:types\"\nimport \"std:text\"\n",
    );
    out.push_str(include_str!("fixtures/crypto_vectors_helpers.gu"));
    let mut expected = Vec::new();
    let mut body = String::from("fn transcript() -> Text:\n    var out = \"\"\n");
    let mut line = |label: &str, call: &str, answer: String| {
        body.push_str(&format!("    set out = out + line(\"{label}\", {call})\n"));
        expected.push(format!("{label}={answer}"));
    };

    // Ed25519: RFC 8032 section 7.1, every vector through all three codes.
    for (index, case) in json_fixture("ed25519/rfc8032_section_7_1.json")
        .iter()
        .enumerate()
    {
        let (seed, key, message, signature) = (
            field(case, "seed"),
            field(case, "public_key"),
            field(case, "message"),
            field(case, "signature"),
        );
        let (hk, hm, hs, hseed) = (hex(&key), hex(&message), hex(&signature), hex(&seed));
        let answer = bytes_text(guion_crypto::ed25519_public_key(&seed));
        line(
            &format!("rfc {index} public key"),
            &format!("octets(crypto_ed25519_public_key(h(\"{hseed}\")))"),
            answer,
        );
        let answer = bytes_text(guion_crypto::ed25519_sign(&seed, &message));
        line(
            &format!("rfc {index} sign"),
            &format!("octets(crypto_ed25519_sign(h(\"{hseed}\"), h(\"{hm}\")))"),
            answer,
        );
        let answer = flag_text(guion_crypto::ed25519_verify(&key, &message, &signature));
        assert_eq!(answer, "true", "rfc {index}");
        line(
            &format!("rfc {index} verify"),
            &format!("flag(crypto_ed25519_verify(h(\"{hk}\"), h(\"{hm}\"), h(\"{hs}\")))"),
            answer,
        );
        let mut tampered = signature.clone();
        tampered[10] ^= 1;
        let answer = flag_text(guion_crypto::ed25519_verify(&key, &message, &tampered));
        assert_eq!(answer, "false", "rfc {index} tampered");
        line(
            &format!("rfc {index} tampered"),
            &format!(
                "flag(crypto_ed25519_verify(h(\"{hk}\"), h(\"{hm}\"), h(\"{}\")))",
                hex(&tampered)
            ),
            answer,
        );
    }
    // The twelve cases of "Taming the Many EdDSAs": small-order, non-canonical and mixed-order
    // inputs. Only case 3 verifies under strict verification.
    for case in json_fixture("ed25519/speccheck_cases.json") {
        let number = case["number"].as_u64().unwrap();
        let (key, message, signature) = (
            field(&case, "pub_key"),
            field(&case, "message"),
            field(&case, "signature"),
        );
        let answer = flag_text(guion_crypto::ed25519_verify(&key, &message, &signature));
        assert_eq!(answer, case["verify_strict"].to_string(), "case {number}");
        line(
            &format!("speccheck {number}"),
            &format!(
                "flag(crypto_ed25519_verify(h(\"{}\"), h(\"{}\"), h(\"{}\")))",
                hex(&key),
                hex(&message),
                hex(&signature)
            ),
            answer,
        );
    }
    // Wrong lengths and a key that is not a curve point.
    let rfc = &json_fixture("ed25519/rfc8032_section_7_1.json")[0];
    let (seed, key, signature) = (
        field(rfc, "seed"),
        field(rfc, "public_key"),
        field(rfc, "signature"),
    );
    for length in [0usize, 31, 33] {
        let bad = vec![7u8; length];
        let answer = bytes_text(guion_crypto::ed25519_public_key(&bad));
        line(
            &format!("seed of {length}"),
            &format!("octets(crypto_ed25519_public_key(h(\"{}\")))", hex(&bad)),
            answer,
        );
        let answer = bytes_text(guion_crypto::ed25519_sign(&bad, b""));
        line(
            &format!("sign seed of {length}"),
            &format!("octets(crypto_ed25519_sign(h(\"{}\"), h(\"\")))", hex(&bad)),
            answer,
        );
    }
    for length in [31usize, 33] {
        let bad = vec![1u8; length];
        let answer = flag_text(guion_crypto::ed25519_verify(&bad, b"", &signature));
        line(
            &format!("key of {length}"),
            &format!(
                "flag(crypto_ed25519_verify(h(\"{}\"), h(\"\"), h(\"{}\")))",
                hex(&bad),
                hex(&signature)
            ),
            answer,
        );
    }
    for length in [63usize, 65] {
        let bad = vec![1u8; length];
        let answer = flag_text(guion_crypto::ed25519_verify(&key, b"", &bad));
        line(
            &format!("signature of {length}"),
            &format!(
                "flag(crypto_ed25519_verify(h(\"{}\"), h(\"\"), h(\"{}\")))",
                hex(&key),
                hex(&bad)
            ),
            answer,
        );
    }
    let off_curve = [0xffu8; 32];
    let answer = flag_text(guion_crypto::ed25519_verify(&off_curve, b"", &signature));
    line(
        "key off the curve",
        &format!(
            "flag(crypto_ed25519_verify(h(\"{}\"), h(\"\"), h(\"{}\")))",
            hex(&off_curve),
            hex(&signature)
        ),
        answer,
    );
    let _ = seed;

    // RFC 3161: requests.
    let message = crypto_fixture("tsa/local/message.txt");
    let digest = guion_crypto::sha512(&message);
    let nonce = [0x01u8, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef];
    let answer = bytes_text(guion_crypto::tsa_request(&digest, &[]));
    assert_eq!(answer, hex(&crypto_fixture("tsa/local/request.tsq")));
    line(
        "request",
        "octets(crypto_tsa_request(crypto_sha512(m0()), h(\"\")))",
        answer,
    );
    let answer = bytes_text(guion_crypto::tsa_request(&digest, &nonce));
    line(
        "request nonce",
        &format!(
            "octets(crypto_tsa_request(crypto_sha512(m0()), h(\"{}\")))",
            hex(&nonce)
        ),
        answer,
    );
    for length in [0usize, 20, 32, 63, 65] {
        let bad = vec![3u8; length];
        let answer = bytes_text(guion_crypto::tsa_request(&bad, &[]));
        line(
            &format!("request digest of {length}"),
            &format!("octets(crypto_tsa_request(h(\"{}\"), h(\"\")))", hex(&bad)),
            answer,
        );
    }
    for (name, bad) in [
        ("of 32", vec![0x7fu8; 32]),
        ("of 33", vec![0x7fu8; 33]),
        ("starting 0", vec![0u8, 1]),
        ("starting 0x80", vec![0x80u8, 1]),
    ] {
        let answer = bytes_text(guion_crypto::tsa_request(&digest, &bad));
        line(
            &format!("request nonce {name}"),
            &format!(
                "octets(crypto_tsa_request(crypto_sha512(m0()), h(\"{}\")))",
                hex(&bad)
            ),
            answer,
        );
    }

    // RFC 3161: tokens, certificates, refusals and every Ok(None) path.
    let mut files = BTreeSet::new();
    for case in cases {
        files.insert(format!("tsa/local/{}", case.response));
        files.insert(format!("tsa/local/{}", case.message));
    }
    let _ = files;
    for case in cases {
        let response = crypto_fixture(&format!("tsa/local/{}", case.response));
        let message = crypto_fixture(&format!("tsa/local/{}", case.message));
        let tsa = pem_fixture(&format!("tsa/local/{}", case.tsa));
        let issuer = pem_fixture(&format!("tsa/local/{}", case.issuer));
        let digest = guion_crypto::sha512(&message);
        let answer = tsa_text(&response, &digest, &tsa, &issuer);
        line(
            &format!("tsa {}", case.name),
            &format!(
                "tsa_case(h(\"{}\"), h(\"{}\"), h(\"{}\"), h(\"{}\"))",
                hex(&response),
                hex(&digest),
                hex(&tsa),
                hex(&issuer)
            ),
            answer,
        );
    }
    // The refusal is reported by the response step, with the status in the message.
    let rejection = crypto_fixture("tsa/local/rejection.tsr");
    let answer = bytes_text(guion_crypto::tsa_response_token(&rejection));
    assert_eq!(answer, "E_TSA_STATUS");
    line(
        "tsa rejection token",
        &format!(
            "octets(crypto_tsa_response_token(h(\"{}\")))",
            hex(&rejection)
        ),
        answer,
    );
    let good = crypto_fixture("tsa/local/good-v2.tsr");
    let tsa = pem_fixture("tsa/local/tsa1.pem");
    let ca = pem_fixture("tsa/local/ca.pem");
    let token = guion_crypto::tsa_response_token(&good).unwrap();
    for (name, bytes) in [
        ("empty", Vec::new()),
        ("zero", vec![0u8]),
        ("truncated", good[..good.len() / 2].to_vec()),
    ] {
        let answer = bytes_text(guion_crypto::tsa_response_token(&bytes));
        line(
            &format!("tsa response {name}"),
            &format!("octets(crypto_tsa_response_token(h(\"{}\")))", hex(&bytes)),
            answer,
        );
    }
    let certificates = guion_crypto::tsa_certificates(&token).unwrap();
    line(
        "tsa certificates",
        &format!("certificate_hashes(h(\"{}\"))", hex(&token)),
        certificates
            .iter()
            .map(|der| hex(&guion_crypto::sha512(der)))
            .collect::<Vec<_>>()
            .join(","),
    );
    let answer = match guion_crypto::tsa_certificates(&[0u8]) {
        Ok(_) => "ok".to_owned(),
        Err(error) => error.code.to_owned(),
    };
    line(
        "tsa certificates garbage",
        "certificate_hashes(h(\"00\"))",
        answer,
    );
    for (name, tsa_bytes, issuer_bytes) in [
        ("tsa certificate garbage", vec![0u8], ca.clone()),
        ("issuer certificate garbage", tsa.clone(), vec![0u8]),
    ] {
        let answer = tsa_text(&good, &digest, &tsa_bytes, &issuer_bytes);
        line(
            &format!("tsa {name}"),
            &format!(
                "tsa_case(h(\"{}\"), h(\"{}\"), h(\"{}\"), h(\"{}\"))",
                hex(&good),
                hex(&digest),
                hex(&tsa_bytes),
                hex(&issuer_bytes)
            ),
            answer,
        );
    }
    let answer = {
        let verified = guion_crypto::tsa_verify(&token, &digest[..32], &tsa, &ca);
        match verified {
            Ok(Some(_)) => "some".to_owned(),
            Ok(None) => "none".to_owned(),
            Err(error) => error.code.to_owned(),
        }
    };
    assert_eq!(answer, "E_TSA_DIGEST", "a 32-byte digest is an input error");
    line(
        "tsa digest of 32",
        &format!(
            "tsa_case(h(\"{}\"), h(\"{}\"), h(\"{}\"), h(\"{}\"))",
            hex(&good),
            hex(&digest[..32]),
            hex(&tsa),
            hex(&ca)
        ),
        answer,
    );
    body.push_str("    return out\n\nfn main() -> Int:\n    return 0\n");

    // The message is shared by the request lines.
    out.push_str(&format!(
        "fn m0() -> Bytes:\n    return h(\"{}\")\n",
        hex(&crypto_fixture("tsa/local/message.txt"))
    ));
    out.push_str(&body);
    (out, expected)
}

/// The steps one verify costs: its entry less the same entry without the call.
fn steps_of(artifact: &[u8], entry: &str) -> u64 {
    let response = browser_response(artifact, entry, u64::from(MAX_BROWSER_STEPS));
    assert_eq!(response["ok"], true, "{entry}: {response}");
    response["steps"].as_u64().unwrap()
}

const TRANSCRIPT_STEP_BUDGET: u64 = 2_000_000;

#[test]
fn browser_ed25519_and_tsa_match_native() {
    let cases = tsa_cases();
    let (source, expected) = ed25519_and_tsa_program(&cases);
    let artifact = gub_project("ed25519-tsa", &source);
    assert!(
        artifact.len() <= MAX_ARTIFACT_BYTES,
        "{} bytes",
        artifact.len()
    );

    // The browser, the native runtime and the crypto crate called directly all agree, line for
    // line, on every value and every error code.
    let native_run = native(&artifact, "transcript");
    // The whole transcript is many verifications, so the page opts in to a raised budget.
    let response = browser_response(&artifact, "transcript", TRANSCRIPT_STEP_BUDGET);
    assert_eq!(response["ok"], true, "{response}");
    let browser_run = Outcome::Text(response["value"]["value"].as_str().unwrap().to_owned());
    assert_eq!(browser_run, native_run, "the transcripts differ");
    eprintln!(
        "transcript: {} checks, {} steps, {} artifact bytes",
        expected.len(),
        response["steps"],
        artifact.len()
    );
    let Outcome::Text(transcript) = browser_run else {
        panic!("the transcript must be text");
    };
    let lines: Vec<&str> = transcript.lines().collect();
    assert_eq!(lines.len(), expected.len(), "{transcript}");
    for (got, want) in lines.iter().zip(&expected) {
        assert_eq!(got, want, "in:\n{transcript}");
    }

    // The spelled-out outcomes the brief asks for: a rejection, the Ok(None) cases, the errors.
    for want in [
        "tsa rejection=E_TSA_STATUS",
        "tsa rejection token=E_TSA_STATUS",
        "tsa other digest=none",
        "tsa good token, other message=none",
        "tsa sha256 imprint=none",
        "tsa second tsa certificate=none",
        "tsa second tsa token=none",
        "tsa other issuer=none",
        "tsa no eku=none",
        "tsa ess mismatch=none",
        "tsa p256 token, rsa certificate=none",
        "tsa tsa certificate garbage=E_X509",
        "tsa response empty=E_TSA_FORMAT",
        "request digest of 32=E_TSA_DIGEST",
        "request nonce of 33=E_TSA_NONCE",
        "speccheck 3=true",
        "speccheck 0=false",
        "key off the curve=false",
    ] {
        assert!(lines.contains(&want), "missing `{want}` in:\n{transcript}");
    }
    let good = lines
        .iter()
        .find(|line| line.starts_with("tsa good v2="))
        .unwrap();
    assert!(good.contains("1795463311000"), "{good}");

    // Steps, measured: the entry with the call less the same entry without it.
    let digest = hex(&guion_crypto::sha512(&crypto_fixture(
        "tsa/local/message.txt",
    )));
    let response = hex(&crypto_fixture("tsa/local/good-v2.tsr"));
    let (tsa, ca) = (
        hex(&pem_fixture("tsa/local/tsa1.pem")),
        hex(&pem_fixture("tsa/local/ca.pem")),
    );
    let rfc = &json_fixture("ed25519/rfc8032_section_7_1.json")[0];
    let (key, signature) = (
        hex(&field(rfc, "public_key")),
        hex(&field(rfc, "signature")),
    );
    let measuring = gub_project(
        "ed25519-tsa-steps",
        &format!(
            "import \"std:crypto\"\nimport \"std:encoding\"\nimport \"std:types\"\n{}\
fn tsa_inputs() -> Bytes:\n    return h(\"{response}\")\n\
fn tsa_with() -> Int:\n    match crypto_tsa_response_token(tsa_inputs()):\n        Ok(token):\n            match crypto_tsa_verify(token, h(\"{digest}\"), h(\"{tsa}\"), h(\"{ca}\")):\n                Ok(found):\n                    return 1\n                Err(problem):\n                    return 2\n        Err(problem):\n            return 3\n\
fn tsa_without() -> Int:\n    return len(tsa_inputs()) + len(h(\"{digest}\")) + len(h(\"{tsa}\")) + len(h(\"{ca}\"))\n\
fn ed_with() -> Int:\n    match crypto_ed25519_verify(h(\"{key}\"), h(\"\"), h(\"{signature}\")):\n        Ok(ok):\n            return 1\n        Err(problem):\n            return 2\n\
fn ed_without() -> Int:\n    return len(h(\"{key}\")) + len(h(\"\")) + len(h(\"{signature}\"))\n\
fn main() -> Int:\n    return 0\n",
            include_str!("fixtures/crypto_vectors_helpers.gu")
        ),
    );
    let tsa_steps = steps_of(&measuring, "tsa_with") - steps_of(&measuring, "tsa_without");
    let ed_steps = steps_of(&measuring, "ed_with") - steps_of(&measuring, "ed_without");
    eprintln!(
        "steps: crypto_tsa_response_token plus crypto_tsa_verify, with the result matched, {tsa_steps}; one crypto_ed25519_verify, with the result matched, {ed_steps} (each beyond decoding its inputs from hex)"
    );
    // A crypto call is one instruction however long the verification takes (the timestamp figure is
    // mostly the std:crypto wrapper building the TsaToken record), so both are far below the
    // default browser budget of MAX_BROWSER_STEPS.
    assert!(tsa_steps < 300 && ed_steps < 50, "{tsa_steps} {ed_steps}");
}

const CASE_PROGRAM: &str = "import \"std:text\"

fn transcript() -> Text:
    return text_upper(\"Stra\u{df}e\") + \"|\" + text_lower(\"\u{3a3}\u{391}\u{3a3}\") + \"|\" + text_lower(\"ABC\") + \"|\" + text_upper(\"\")

fn main() -> Int:
    return 0
";

/// `text_lower` and `text_upper` are pure, so the browser VM runs them with the same results as the
/// native runtime.
#[test]
fn browser_text_case_matches_native() {
    let artifact = gub_project("case", CASE_PROGRAM);
    let expected = Outcome::Text("STRASSE|\u{3c3}\u{3b1}\u{3c2}|abc|".to_owned());
    assert_eq!(native(&artifact, "transcript"), expected, "native");
    assert_eq!(browser(&artifact, "transcript"), expected, "browser");
}

const KEYS_PROGRAM: &str = "import \"std:json\"
import \"std:text\"
import \"std:types\"

fn keys_of(text: Text) -> Text:
    match json_keys(unwrap(json_parse(text))):
        Ok(keys):
            return \"[\" + text_join(keys, \"|\") + \"]\"
        Err(problem):
            return json_error_code(problem)

fn transcript() -> Text:
    return keys_of(\"{\\\"b\\\":1,\\\"a\\\":{\\\"z\\\":2}}\") + keys_of(\"{}\") + keys_of(\"[1]\")

fn main() -> Int:
    return 0
";

/// `json_keys` runs in the browser VM with the same keys and error code as the native runtime.
#[test]
fn browser_json_keys_matches_native() {
    let artifact = gub_project("keys", KEYS_PROGRAM);
    let expected = Outcome::Text("[a|b][]R_JSON_TYPE".to_owned());
    assert_eq!(native(&artifact, "transcript"), expected, "native");
    assert_eq!(browser(&artifact, "transcript"), expected, "browser");
}

const PRETTY_PROGRAM: &str = "import \"std:json\"
import \"std:types\"

fn transcript() -> Text:
    let value = unwrap(json_parse(\"{\\\"b\\\":[1,{}],\\\"a\\\":\\\"caf\u{e9}\\\",\\\"c\\\":[]}\"))
    return json_stringify_pretty(value, 2)

fn bad_indent() -> Text:
    return json_stringify_pretty(json_object(), 17)

fn main() -> Int:
    return 0
";

/// `json_stringify_pretty` runs in the browser VM with the same text and error code as the native
/// runtime.
#[test]
fn browser_json_stringify_pretty_matches_native() {
    let artifact = gub_project("pretty", PRETTY_PROGRAM);
    let expected = Outcome::Text(
        "{\n  \"a\": \"caf\u{e9}\",\n  \"b\": [\n    1,\n    {}\n  ],\n  \"c\": []\n}".to_owned(),
    );
    assert_eq!(native(&artifact, "transcript"), expected, "native");
    assert_eq!(browser(&artifact, "transcript"), expected, "browser");
    let error = Outcome::Error("R_JSON_INDENT".to_owned());
    assert_eq!(native(&artifact, "bad_indent"), error, "native");
    assert_eq!(browser(&artifact, "bad_indent"), error, "browser");
}

/// `fs_set_mode` and `fs_mode` need a host filesystem, which the browser VM does not have.
#[test]
fn file_modes_are_unsupported_in_the_browser() {
    for (name, body) in [
        ("fs-set-mode", "unwrap(fs_set_mode(\"a\", 0o600))"),
        ("fs-mode", "print(unwrap(fs_mode(\"a\")))"),
    ] {
        let artifact = gub_project(
            name,
            &format!("import \"std:fs\"\nfn main() -> Int:\n    {body}\n    return 0\n"),
        );
        let response = browser_response(&artifact, "main", 1000);
        assert_eq!(response["ok"], false, "{response}");
        assert_eq!(response["code"], "R_HOST_UNSUPPORTED", "{response}");
    }
}

/// `fs_disk_usage`, `fs_free_space` and `fs_is_link` need a host filesystem, which the browser VM
/// does not have.
#[test]
fn disk_usage_free_space_and_links_are_unsupported_in_the_browser() {
    for (name, body) in [
        ("fs-disk-usage", "print(unwrap(fs_disk_usage(\"a\")))"),
        ("fs-free-space", "print(unwrap(fs_free_space(\".\")))"),
        ("fs-is-link", "print(unwrap(fs_is_link(\"a\")))"),
    ] {
        let artifact = gub_project(
            name,
            &format!("import \"std:fs\"\nfn main() -> Int:\n    {body}\n    return 0\n"),
        );
        let response = browser_response(&artifact, "main", 1000);
        assert_eq!(response["ok"], false, "{response}");
        assert_eq!(response["code"], "R_HOST_UNSUPPORTED", "{response}");
    }
}

/// `tcp_peer` needs a socket, which the browser VM does not have.
#[test]
fn tcp_peer_is_unsupported_in_the_browser() {
    let artifact = gub_project(
        "tcp-peer",
        "import \"std:net\"\nfn main() -> Int:\n    print(is_ok(tcp_peer(TcpConn(1))))\n    return 0\n",
    );
    let response = browser_response(&artifact, "main", 1000);
    assert_eq!(response["ok"], false, "{response}");
    assert_eq!(response["code"], "R_HOST_UNSUPPORTED", "{response}");
}

const RADIX_PROGRAM: &str = "import \"std:text\"

fn parsed(text: Text, radix: Int) -> Text:
    let result = text_parse_int_radix(text, radix)
    if is_ok(result):
        return int_to_text(unwrap(result))
    return \"err\"

fn transcript() -> Text:
    return int_to_text_radix(255, 16) + \"|\" + int_to_text_radix(-5, 2) + \"|\" + int_to_text_radix(0xFFFF_FFFF_FFFF_FFFF_FFFF, 36) + \"|\" + parsed(\"FF\", 16) + \"|\" + parsed(\"-zz\", 36) + \"|\" + parsed(\"ffffffffffffffffffff\", 16) + \"|\" + parsed(\"0x1f\", 16) + \"|\" + parsed(\"\", 10)

fn bad_radix() -> Text:
    return int_to_text_radix(1, 37)

fn bad_parse_radix() -> Text:
    return parsed(\"1\", 1)

fn main() -> Int:
    return 0
";

/// `int_to_text_radix` and `text_parse_int_radix` run in the browser VM with the same text and
/// error codes as the native runtime.
#[test]
fn browser_radix_functions_match_native() {
    let artifact = gub_project("radix", RADIX_PROGRAM);
    let expected = Outcome::Text(
        "ff|-101|5gv2rma270x9hhj3|255|-1295|1208925819614629174706175|err|err".to_owned(),
    );
    assert_eq!(native(&artifact, "transcript"), expected, "native");
    assert_eq!(browser(&artifact, "transcript"), expected, "browser");
    let error = Outcome::Error("R_RADIX_RANGE".to_owned());
    for function in ["bad_radix", "bad_parse_radix"] {
        assert_eq!(native(&artifact, function), error, "native {function}");
        assert_eq!(browser(&artifact, function), error, "browser {function}");
    }
}
