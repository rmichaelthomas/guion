//! BLS12-381 min-signature verification (G1 signatures, G2 public keys) against drand quicknet
//! and the RFC 9380 hash-to-G1 vectors.

use guion_crypto::bls::{g1_coordinates, hash_to_g1};
use guion_crypto::bls12381_verify_g1;
use sha2::{Digest, Sha256};

const PUBLIC_KEY: &str = "83cf0f2896adee7eb8b5f01fcad3912212c437e0073e911fb90022d3e760183c8c4b450b6a0a6c3ac6a5776a2d1064510d1fec758c921cc22b0e17e63aaf4bcb5ed66304de9cf809bd274ca73bab4af5a6e9c76a4bc09e76eae8991ef5ece45a";
const SIG_1000: &str = "b44679b9a59af2ec876b1a6b1ad52ea9b1615fc3982b19576350f93447cb1125e342b73a8dd2bacbe47e4b6b63ed5e39";
const SIG_34173416: &str = "b889e8224d9d10eafdd637a2cdc6042108bb8aac8a6e899e9bc0e8be88e8af2b359cc82b70447dac08b7f4f5f0deb832";
const DST: &[u8] = b"BLS_SIG_BLS12381G1_XMD:SHA-256_SSWU_RO_NUL_";
// The compressed G2 generator.
const G2_GENERATOR: &str = "93e02b6052719f607dacd3a088274f65596bd0d09920b61ab5da61bbdc7f5049334cf11213945d57e5ac7d055d042b7e024aa2b2f08f0a91260805272dc51051c6e47ad4fa403b02b4510b647ae3d1770bac0326a805bbefd48056c8c121bdb8";

fn unhex(text: &str) -> Vec<u8> {
    assert!(text.len().is_multiple_of(2));
    (0..text.len() / 2)
        .map(|index| u8::from_str_radix(&text[index * 2..index * 2 + 2], 16).unwrap())
        .collect()
}

fn hex(data: &[u8]) -> String {
    data.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// drand quicknet signs SHA-256 of the round as an 8-byte big-endian integer.
fn round_message(round: u64) -> Vec<u8> {
    Sha256::digest(round.to_be_bytes()).to_vec()
}

fn verify(key: &[u8], message: &[u8], signature: &[u8]) -> Result<bool, guion_crypto::CryptoError> {
    bls12381_verify_g1(key, message, signature, DST)
}

#[test]
fn quicknet_round_1000_verifies() {
    assert_eq!(
        verify(&unhex(PUBLIC_KEY), &round_message(1000), &unhex(SIG_1000)),
        Ok(true)
    );
}

#[test]
fn quicknet_round_34173416_verifies() {
    assert_eq!(
        verify(
            &unhex(PUBLIC_KEY),
            &round_message(34173416),
            &unhex(SIG_34173416)
        ),
        Ok(true)
    );
}

#[test]
fn wrong_round_is_false() {
    assert_eq!(
        verify(&unhex(PUBLIC_KEY), &round_message(1001), &unhex(SIG_1000)),
        Ok(false)
    );
}

#[test]
fn flipped_bit_never_true() {
    let signature = unhex(SIG_1000);
    for index in 0..signature.len() {
        for bit in 0..8 {
            let mut flipped = signature.clone();
            flipped[index] ^= 1 << bit;
            let outcome = verify(&unhex(PUBLIC_KEY), &round_message(1000), &flipped);
            assert!(
                matches!(outcome, Ok(false) | Err(_)),
                "byte {index} bit {bit}"
            );
        }
    }
}

#[test]
fn wrong_key_is_false() {
    assert_eq!(
        verify(&unhex(G2_GENERATOR), &round_message(1000), &unhex(SIG_1000)),
        Ok(false)
    );
}

#[test]
fn wrong_dst_is_false() {
    assert_eq!(
        bls12381_verify_g1(
            &unhex(PUBLIC_KEY),
            &round_message(1000),
            &unhex(SIG_1000),
            b"BLS_SIG_BLS12381G2_XMD:SHA-256_SSWU_RO_NUL_"
        ),
        Ok(false)
    );
}

#[test]
fn bad_lengths_are_errors() {
    let key = unhex(PUBLIC_KEY);
    let signature = unhex(SIG_1000);
    let message = round_message(1000);
    let code = |outcome: Result<bool, guion_crypto::CryptoError>| outcome.unwrap_err().code;
    assert_eq!(
        code(verify(&key, &message, &signature[..47])),
        "E_CRYPTO_INPUT"
    );
    let mut long = signature.clone();
    long.push(0);
    assert_eq!(code(verify(&key, &message, &long)), "E_CRYPTO_INPUT");
    assert_eq!(
        code(verify(&key[..95], &message, &signature)),
        "E_CRYPTO_INPUT"
    );
    assert_eq!(code(verify(&[], &message, &signature)), "E_CRYPTO_INPUT");
}

#[test]
fn malformed_points_are_errors() {
    let key = unhex(PUBLIC_KEY);
    let message = round_message(1000);
    // Compression flag cleared: an uncompressed encoding of the wrong length class.
    let mut no_flag = unhex(SIG_1000);
    no_flag[0] &= 0x7f;
    assert_eq!(
        verify(&key, &message, &no_flag).unwrap_err().code,
        "E_CRYPTO_INPUT"
    );
    // x coordinate not below the field modulus.
    let mut big_x = vec![0xff; 48];
    big_x[0] = 0x9f;
    assert_eq!(
        verify(&key, &message, &big_x).unwrap_err().code,
        "E_CRYPTO_INPUT"
    );
    // The point at infinity is not a public key.
    let mut infinity = vec![0u8; 96];
    infinity[0] = 0xc0;
    assert_eq!(
        verify(&infinity, &message, &unhex(SIG_1000))
            .unwrap_err()
            .code,
        "E_CRYPTO_INPUT"
    );
}

/// RFC 9380 appendix J.9.1: BLS12381G1_XMD:SHA-256_SSWU_RO_.
#[test]
fn rfc9380_hash_to_g1_vectors() {
    let dst = b"QUUX-V01-CS02-with-BLS12381G1_XMD:SHA-256_SSWU_RO_";
    let cases: [(&[u8], &str, &str); 3] = [
        (
            b"",
            "052926add2207b76ca4fa57a8734416c8dc95e24501772c814278700eed6d1e4e8cf62d9c09db0fac349612b759e79a1",
            "08ba738453bfed09cb546dbb0783dbb3a5f1f566ed67bb6be0e8c67e2e81a4cc68ee29813bb7994998f3eae0c9c6a265",
        ),
        (
            b"abc",
            "03567bc5ef9c690c2ab2ecdf6a96ef1c139cc0b2f284dca0a9a7943388a49a3aee664ba5379a7655d3c68900be2f6903",
            "0b9c15f3fe6e5cf4211f346271d7b01c8f3b28be689c8429c85b67af215533311f0b8dfaaa154fa6b88176c229f2885d",
        ),
        (
            b"abcdef0123456789",
            "11e0b079dea29a68f0383ee94fed1b940995272407e3bb916bbf268c263ddd57a6a27200a784cbc248e84f357ce82d98",
            "03a87ae2caf14e8ee52e51fa2ed8eefe80f02457004ba4d486d6aa1f517c0889501dc7413753f9599b099ebcbbd2d709",
        ),
    ];
    for (message, x, y) in cases {
        let point = hash_to_g1(message, dst).unwrap();
        let (got_x, got_y) = g1_coordinates(&point);
        assert_eq!(hex(&got_x), x, "x for {message:?}");
        assert_eq!(hex(&got_y), y, "y for {message:?}");
    }
}
