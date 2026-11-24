use guion_crypto::x509_verify_issued_by;

fn der(name: &str) -> Vec<u8> {
    let path = format!(
        "{}/tests/fixtures/x509/{name}.der",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read(&path).unwrap_or_else(|error| panic!("{path}: {error}"))
}

fn issued(cert: &str, issuer: &str) -> bool {
    x509_verify_issued_by(&der(cert), &der(issuer))
        .unwrap_or_else(|error| panic!("{cert}: {error}"))
}

fn code(cert: &str, issuer: &str) -> &'static str {
    x509_verify_issued_by(&der(cert), &der(issuer))
        .expect_err("expected an error")
        .code
}

#[test]
fn rsa_pkcs1_v15_with_sha256_384_and_512() {
    for leaf in ["leaf-rsa-sha256", "leaf-rsa-sha384", "leaf-rsa-sha512"] {
        assert!(issued(leaf, "rsa-ca"), "{leaf}");
    }
}

#[test]
fn rsa_pss_with_sha256_and_sha384() {
    assert!(issued("leaf-rsa-pss-sha256", "rsa-ca"));
    assert!(issued("leaf-rsa-pss-sha384", "rsa-ca"));
}

#[test]
fn ecdsa_p256_and_p384() {
    assert!(issued("leaf-p256-sha256", "p256-ca"));
    assert!(issued("leaf-p256-sha384", "p256-ca"));
    assert!(issued("leaf-p384-sha384", "p384-ca"));
    assert!(issued("leaf-p384-sha256", "p384-ca"));
}

#[test]
fn a_ca_with_the_same_name_but_another_key_does_not_verify() {
    assert!(!issued("leaf-rsa-sha256", "rsa-other-ca"));
    assert!(!issued("leaf-rsa-pss-sha256", "rsa-other-ca"));
    // Same subject name on every test CA; a key of the wrong type is a plain false.
    assert!(!issued("leaf-p256-sha256", "rsa-ca"));
    assert!(!issued("leaf-rsa-sha256", "p256-ca"));
    assert!(!issued("leaf-p256-sha256", "p384-ca"));
}

#[test]
fn a_different_issuer_name_is_false() {
    // NIST's certificate names DigiCert's intermediate, not the root.
    assert!(!issued("nist-leaf", "nist-root"));
    assert!(!issued("nist-leaf", "rsa-ca"));
}

#[test]
fn nists_real_chain() {
    assert!(issued("nist-leaf", "nist-issuer"));
    assert!(issued("nist-issuer", "nist-root"));
    // The root signs itself.
    assert!(issued("nist-root", "nist-root"));
}

#[test]
fn unsupported_algorithms_are_e_x509_alg() {
    assert_eq!(code("leaf-rsa-sha1", "rsa-ca"), "E_X509_ALG");
    assert_eq!(code("leaf-p521-sha512", "p521-ca"), "E_X509_ALG");
    assert_eq!(code("leaf-ed25519", "ed25519-ca"), "E_X509_ALG");
}

#[test]
fn a_modified_certificate_is_false() {
    let issuer = der("rsa-ca");
    let original = der("leaf-rsa-sha256");
    // Flip one byte at a time: inside the to-be-signed part and inside the signature.
    for index in [
        40,
        60,
        100,
        original.len() / 2,
        original.len() - 5,
        original.len() - 1,
    ] {
        let mut changed = original.clone();
        changed[index] ^= 1;
        if let Ok(valid) = x509_verify_issued_by(&changed, &issuer) {
            assert!(!valid, "byte {index}");
        }
    }
    let ec_issuer = der("p256-ca");
    let ec_original = der("leaf-p256-sha256");
    let mut changed = ec_original.clone();
    let last = changed.len() - 3;
    changed[last] ^= 1;
    assert!(!x509_verify_issued_by(&changed, &ec_issuer).unwrap_or(false));
}

#[test]
fn malformed_input_is_e_x509_and_never_panics() {
    let good = der("leaf-rsa-sha256");
    let issuer = der("rsa-ca");
    assert_eq!(
        x509_verify_issued_by(&[], &issuer).unwrap_err().code,
        "E_X509"
    );
    assert_eq!(
        x509_verify_issued_by(&good, b"junk").unwrap_err().code,
        "E_X509"
    );
    assert_eq!(
        x509_verify_issued_by(b"\x30\x00", &issuer)
            .unwrap_err()
            .code,
        "E_X509"
    );
    for cut in (0..good.len()).step_by(7) {
        let _ = x509_verify_issued_by(&good[..cut], &issuer);
        let _ = x509_verify_issued_by(&good, &issuer[..cut.min(issuer.len())]);
    }
}

#[test]
fn rsa_pss_honours_the_certificates_salt_length() {
    // Salt lengths 0, 20 (the RFC 4055 default, omitted from the parameters) and 64.
    for leaf in ["leaf-pss-salt0", "leaf-pss-salt20", "leaf-pss-salt64"] {
        assert!(issued(leaf, "name-rsa-ca"), "{leaf}");
    }
    // The existing fixtures use salt = digest length and still verify.
    assert!(issued("leaf-rsa-pss-sha256", "rsa-ca"));
    assert!(issued("leaf-name-rsa-sha256", "name-rsa-ca"));
}

/// Finds `needle` in `haystack` at or after `from`.
fn find(haystack: &[u8], needle: &[u8], from: usize) -> usize {
    from + haystack[from..]
        .windows(needle.len())
        .position(|window| window == needle)
        .expect("pattern present")
}

#[test]
fn an_outer_algorithm_that_differs_from_the_signed_one_is_false() {
    let issuer = der("rsa-ca");
    let original = der("leaf-rsa-sha256");
    assert_eq!(x509_verify_issued_by(&original, &issuer), Ok(true));
    // sha256WithRSAEncryption AlgorithmIdentifier with its NULL parameters.
    let with_null: &[u8] = &[
        0x30, 0x0d, 0x06, 0x09, 0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x0b, 0x05, 0x00,
    ];
    let without_null: &[u8] = &[
        0x30, 0x0b, 0x06, 0x09, 0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x0b,
    ];
    // The first occurrence is inside tbsCertificate, the last is the outer signatureAlgorithm.
    let first = find(&original, with_null, 0);
    let outer = find(&original, with_null, first + with_null.len());
    let mut patched = original[..outer].to_vec();
    patched.extend_from_slice(without_null);
    patched.extend_from_slice(&original[outer + with_null.len()..]);
    // Fix the outer SEQUENCE length (a 2-byte long form: 0x30 0x82 hi lo).
    assert_eq!(patched[1], 0x82);
    let length = u16::from_be_bytes([patched[2], patched[3]]) - 2;
    patched[2..4].copy_from_slice(&length.to_be_bytes());
    // Same key, same signed bytes, valid signature: only the two identifiers differ.
    assert_eq!(x509_verify_issued_by(&patched, &issuer), Ok(false));
}
