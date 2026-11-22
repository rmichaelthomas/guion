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
        match x509_verify_issued_by(&changed, &issuer) {
            Ok(valid) => assert!(!valid, "byte {index}"),
            Err(_) => {}
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
