use guion_crypto::{ed25519_public_key, ed25519_sign, ed25519_verify};
use serde_json::Value;

fn unhex(text: &str) -> Vec<u8> {
    assert!(text.len() % 2 == 0, "odd hex length");
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
        .collect()
}

fn fixture(name: &str) -> Value {
    let path = format!(
        "{}/tests/fixtures/ed25519/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn field(case: &Value, key: &str) -> Vec<u8> {
    unhex(case[key].as_str().unwrap())
}

const SEED: &str = "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60";

#[test]
fn rfc8032_section_7_1_vectors() {
    let cases = fixture("rfc8032_section_7_1.json");
    let cases = cases.as_array().unwrap();
    assert_eq!(cases.len(), 5);
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let (seed, pk, message, sig) = (
            field(case, "seed"),
            field(case, "public_key"),
            field(case, "message"),
            field(case, "signature"),
        );
        assert_eq!(ed25519_public_key(&seed).unwrap().to_vec(), pk, "{name}");
        assert_eq!(
            ed25519_sign(&seed, &message).unwrap().to_vec(),
            sig,
            "{name}"
        );
        assert_eq!(ed25519_verify(&pk, &message, &sig), Ok(true), "{name}");
    }
    // The cross-check against the RFC text for TEST 1, independent of the fixture file.
    let seed = unhex(SEED);
    assert_eq!(
        ed25519_public_key(&seed).unwrap().to_vec(),
        unhex("d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a")
    );
    assert_eq!(
        ed25519_sign(&seed, b"").unwrap().to_vec(),
        unhex("e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b")
    );
}

#[test]
fn signing_is_deterministic() {
    let seed = unhex(SEED);
    assert_eq!(
        ed25519_sign(&seed, b"same message").unwrap(),
        ed25519_sign(&seed, b"same message").unwrap()
    );
}

#[test]
fn wrong_message_is_false() {
    let seed = unhex(SEED);
    let pk = ed25519_public_key(&seed).unwrap();
    let sig = ed25519_sign(&seed, b"message").unwrap();
    assert_eq!(ed25519_verify(&pk, b"message", &sig), Ok(true));
    assert_eq!(ed25519_verify(&pk, b"messagf", &sig), Ok(false));
}

#[test]
fn flipped_signature_bit_is_false_or_err() {
    let seed = unhex(SEED);
    let pk = ed25519_public_key(&seed).unwrap();
    let sig = ed25519_sign(&seed, b"message").unwrap();
    for bit in 0..512 {
        let mut bad = sig;
        bad[bit / 8] ^= 1 << (bit % 8);
        match ed25519_verify(&pk, b"message", &bad) {
            Ok(false) | Err(_) => {}
            Ok(true) => panic!("flipping bit {bit} still verifies"),
        }
    }
}

#[test]
fn other_key_is_false() {
    let seed = unhex(SEED);
    let sig = ed25519_sign(&seed, b"message").unwrap();
    let other = ed25519_public_key(&[7u8; 32]).unwrap();
    assert_eq!(ed25519_verify(&other, b"message", &sig), Ok(false));
}

#[test]
fn speccheck_cases_follow_verify_strict() {
    let cases = fixture("speccheck_cases.json");
    let cases = cases.as_array().unwrap();
    assert_eq!(cases.len(), 12);
    for (index, case) in cases.iter().enumerate() {
        let number = case["number"].as_u64().unwrap();
        assert_eq!(number as usize, index);
        let expected = case["verify_strict"].as_bool().unwrap();
        let got = ed25519_verify(
            &unhex(case["pub_key"].as_str().unwrap()),
            &unhex(case["message"].as_str().unwrap()),
            &unhex(case["signature"].as_str().unwrap()),
        );
        assert_eq!(got, Ok(expected), "speccheck case {number}");
    }
}

#[test]
fn bad_lengths_are_errors() {
    for len in [31usize, 33] {
        let bytes = vec![1u8; len];
        assert_eq!(
            ed25519_public_key(&bytes).unwrap_err().code,
            "E_CRYPTO_INPUT"
        );
        assert_eq!(
            ed25519_sign(&bytes, b"m").unwrap_err().code,
            "E_CRYPTO_INPUT"
        );
        assert_eq!(
            ed25519_verify(&bytes, b"m", &[0u8; 64]).unwrap_err().code,
            "E_CRYPTO_INPUT"
        );
    }
    for len in [63usize, 65] {
        let sig = vec![1u8; len];
        let pk = ed25519_public_key(&[1u8; 32]).unwrap();
        assert_eq!(
            ed25519_verify(&pk, b"m", &sig).unwrap_err().code,
            "E_CRYPTO_INPUT"
        );
    }
}

#[test]
fn undecodable_key_is_error() {
    // y = 2 has no x on the curve (x^2 = (y^2-1)/(d*y^2+1) is a non-residue).
    let mut y = [0u8; 32];
    y[0] = 2;
    let err = ed25519_verify(&y, b"m", &[0u8; 64]).unwrap_err();
    assert_eq!(err.code, "E_CRYPTO_INPUT");
}
