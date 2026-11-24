use guion_crypto::{ecdsa_p256_verify, rsa_verify, x509_rsa_public_key, RsaScheme};

fn fixture(name: &str) -> Vec<u8> {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|error| panic!("{path}: {error}"))
}

fn hex_fixture(name: &str) -> Vec<u8> {
    let text = String::from_utf8(fixture(name)).unwrap();
    let text = text.trim();
    (0..text.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&text[index..index + 2], 16).unwrap())
        .collect()
}

struct Rsa {
    modulus: Vec<u8>,
    exponent: Vec<u8>,
    message: Vec<u8>,
}

fn rsa() -> Rsa {
    Rsa {
        modulus: hex_fixture("rsa_modulus.hex"),
        exponent: hex_fixture("rsa_exponent.hex"),
        message: fixture("message.bin"),
    }
}

const SCHEMES: [(RsaScheme, &str); 3] = [
    (RsaScheme::Pkcs1v15Sha256, "rsa_pkcs1_sha256.sig"),
    (RsaScheme::Pkcs1v15Sha512, "rsa_pkcs1_sha512.sig"),
    (RsaScheme::PssSha256, "rsa_pss_sha256.sig"),
];

#[test]
fn rsa_valid_signatures_verify() {
    let key = rsa();
    for (scheme, file) in SCHEMES {
        let signature = fixture(file);
        assert_eq!(
            rsa_verify(
                &key.modulus,
                &key.exponent,
                &key.message,
                &signature,
                scheme
            ),
            Ok(true),
            "{file}"
        );
    }
}

#[test]
fn rsa_rejects_tampered_message_and_signature() {
    let key = rsa();
    for (scheme, file) in SCHEMES {
        let signature = fixture(file);
        let mut message = key.message.clone();
        message[0] ^= 1;
        assert_eq!(
            rsa_verify(&key.modulus, &key.exponent, &message, &signature, scheme),
            Ok(false),
            "tampered message {file}"
        );
        let mut tampered = signature.clone();
        tampered[10] ^= 1;
        assert_eq!(
            rsa_verify(&key.modulus, &key.exponent, &key.message, &tampered, scheme),
            Ok(false),
            "tampered signature {file}"
        );
    }
}

#[test]
fn rsa_wrong_scheme_is_false() {
    let key = rsa();
    let pkcs1 = fixture("rsa_pkcs1_sha256.sig");
    assert_eq!(
        rsa_verify(
            &key.modulus,
            &key.exponent,
            &key.message,
            &pkcs1,
            RsaScheme::PssSha256
        ),
        Ok(false)
    );
    let pss = fixture("rsa_pss_sha256.sig");
    assert_eq!(
        rsa_verify(
            &key.modulus,
            &key.exponent,
            &key.message,
            &pss,
            RsaScheme::Pkcs1v15Sha256
        ),
        Ok(false)
    );
}

#[test]
fn rsa_bad_lengths_are_errors() {
    let key = rsa();
    let signature = fixture("rsa_pkcs1_sha256.sig");
    let scheme = RsaScheme::Pkcs1v15Sha256;
    let short = &signature[..255];
    let mut long = signature.clone();
    long.push(0);
    for bad in [short, &long[..], &[][..]] {
        let error = rsa_verify(&key.modulus, &key.exponent, &key.message, bad, scheme).unwrap_err();
        assert_eq!(error.code, "E_CRYPTO_INPUT");
    }
    // Empty modulus or exponent, an even modulus, and a modulus of 1 are malformed keys.
    for (modulus, exponent) in [
        (&[][..], &key.exponent[..]),
        (&key.modulus[..], &[][..]),
        (&[0u8, 0, 0][..], &key.exponent[..]),
        (&[1u8][..], &[3u8][..]),
    ] {
        let error = rsa_verify(modulus, exponent, &key.message, &signature, scheme).unwrap_err();
        assert_eq!(error.code, "E_CRYPTO_INPUT");
    }
}

#[test]
fn rsa_leading_zero_bytes_in_key_parts_are_accepted() {
    let key = rsa();
    let mut modulus = vec![0u8];
    modulus.extend_from_slice(&key.modulus);
    let mut exponent = vec![0u8, 0];
    exponent.extend_from_slice(&key.exponent);
    let signature = fixture("rsa_pkcs1_sha256.sig");
    assert_eq!(
        rsa_verify(
            &modulus,
            &exponent,
            &key.message,
            &signature,
            RsaScheme::Pkcs1v15Sha256
        ),
        Ok(true)
    );
}

#[test]
fn ecdsa_valid_verifies() {
    let key = fixture("ec_public_sec1.bin");
    let message = fixture("message.bin");
    let signature = fixture("ec_signature_rs.bin");
    assert_eq!(key.len(), 65);
    assert_eq!(signature.len(), 64);
    assert_eq!(ecdsa_p256_verify(&key, &message, &signature), Ok(true));
}

#[test]
fn ecdsa_flipped_bit_false() {
    let key = fixture("ec_public_sec1.bin");
    let message = fixture("message.bin");
    let signature = fixture("ec_signature_rs.bin");
    let mut tampered = signature.clone();
    tampered[40] ^= 1;
    assert_eq!(ecdsa_p256_verify(&key, &message, &tampered), Ok(false));
    let mut other = message.clone();
    other[3] ^= 1;
    assert_eq!(ecdsa_p256_verify(&key, &other, &signature), Ok(false));
    // r = 0 and s = 0 are well-formed length but never valid.
    assert_eq!(ecdsa_p256_verify(&key, &message, &[0u8; 64]), Ok(false));
}

#[test]
fn ecdsa_off_curve_key_error() {
    let key = fixture("ec_public_sec1.bin");
    let message = fixture("message.bin");
    let signature = fixture("ec_signature_rs.bin");
    let mut off_curve = key.clone();
    off_curve[64] ^= 1;
    for bad in [&off_curve[..], &key[..64], &[][..], &[4u8; 65][..]] {
        let error = ecdsa_p256_verify(bad, &message, &signature).unwrap_err();
        assert_eq!(error.code, "E_CRYPTO_INPUT");
    }
    let error = ecdsa_p256_verify(&key, &message, &signature[..63]).unwrap_err();
    assert_eq!(error.code, "E_CRYPTO_INPUT");
}

#[test]
fn x509_extracts_rsa_key() {
    let (modulus, exponent) = x509_rsa_public_key(&fixture("rsa_cert.der")).unwrap();
    // `openssl x509 -modulus` on the certificate prints the same modulus as the key file.
    assert_eq!(modulus, hex_fixture("rsa_modulus.hex"));
    assert_eq!(exponent, vec![1, 0, 1]);
    let error = x509_rsa_public_key(&fixture("ec_cert.der")).unwrap_err();
    assert_eq!(error.code, "E_X509");
    for bad in [&[][..], &[0x30, 0x03, 1, 2, 3][..], b"not a certificate"] {
        assert_eq!(x509_rsa_public_key(bad).unwrap_err().code, "E_X509");
    }
}

#[test]
fn x509_rsa_key_verifies_a_signature() {
    let (modulus, exponent) = x509_rsa_public_key(&fixture("rsa_cert.der")).unwrap();
    assert_eq!(
        rsa_verify(
            &modulus,
            &exponent,
            &fixture("message.bin"),
            &fixture("rsa_pss_sha256.sig"),
            RsaScheme::PssSha256
        ),
        Ok(true)
    );
}

#[test]
fn rsa_8192_bit_keys_verify_and_larger_moduli_are_refused() {
    let modulus = hex_fixture("rsa8192_modulus.hex");
    assert_eq!(modulus.len(), 1024);
    assert!(
        modulus[0] & 0x80 != 0,
        "the fixture modulus is 8192 bits exactly"
    );
    let message = fixture("message.bin");
    let signature = fixture("rsa8192_pkcs1_sha256.sig");
    assert_eq!(
        rsa_verify(
            &modulus,
            &[1, 0, 1],
            &message,
            &signature,
            RsaScheme::Pkcs1v15Sha256
        ),
        Ok(true)
    );
    // 8193 bits: one more bit than allowed. 8200 bits: a full extra byte.
    for first in [0x01u8, 0xFF] {
        let mut too_big = vec![first];
        too_big.extend_from_slice(&modulus);
        too_big[1024] |= 1;
        let error = rsa_verify(
            &too_big,
            &[1, 0, 1],
            &message,
            &signature,
            RsaScheme::Pkcs1v15Sha256,
        )
        .unwrap_err();
        assert_eq!(error.code, "E_CRYPTO_INPUT", "{first:#x}");
        assert!(error.message.contains("8192"), "{}", error.message);
    }
}

#[test]
fn rsa_pss_uses_the_digest_length_as_the_salt() {
    // The fixture signature was made with salt length 32 (the SHA-256 digest length).
    let key = rsa();
    let signature = fixture("rsa_pss_sha256.sig");
    assert_eq!(
        rsa_verify(
            &key.modulus,
            &key.exponent,
            &key.message,
            &signature,
            RsaScheme::PssSha256
        ),
        Ok(true)
    );
}

#[test]
fn ecdsa_accepts_the_malleated_high_s_twin_of_a_valid_signature() {
    // Documented behaviour: the primitive is permissive, so (r, n - s) verifies as well as (r, s).
    // Signature bytes are therefore not identifiers.
    let key = fixture("ec_public_sec1.bin");
    let message = fixture("message.bin");
    let signature = fixture("ec_signature_rs.bin");
    assert_eq!(ecdsa_p256_verify(&key, &message, &signature), Ok(true));
    let parsed = p256::ecdsa::Signature::from_slice(&signature).unwrap();
    let (r, s) = parsed.split_scalars();
    let twin = p256::ecdsa::Signature::from_scalars(r.to_bytes(), (-*s).to_bytes()).unwrap();
    assert_ne!(twin.to_bytes().as_slice(), signature.as_slice());
    assert_eq!(
        ecdsa_p256_verify(&key, &message, &twin.to_bytes()),
        Ok(true)
    );
}
