//! tlock interoperability with Go `tle` v1.2.0 (fixtures made by `fixtures/tlock/make_fixtures.sh`)
//! and the encrypt/decrypt round trip. Quicknet, rounds 1000 and 1001.

use guion_crypto::{tlock_decrypt, tlock_encrypt, tlock_round};

const PUBLIC_KEY: &str = "83cf0f2896adee7eb8b5f01fcad3912212c437e0073e911fb90022d3e760183c8c4b450b6a0a6c3ac6a5776a2d1064510d1fec758c921cc22b0e17e63aaf4bcb5ed66304de9cf809bd274ca73bab4af5a6e9c76a4bc09e76eae8991ef5ece45a";
const CHAIN_HASH: &str = "52db9ba70e0cc0f6eaf7803dd07447a1f5477735fd3f661792ba94600c84e971";
const SIGNATURE_1000: &str = "b44679b9a59af2ec876b1a6b1ad52ea9b1615fc3982b19576350f93447cb1125e342b73a8dd2bacbe47e4b6b63ed5e39";
const SIGNATURE_1001: &str = "b33bf3667cbd5a82de3a24b4e0e9fe5513cc1a0e840368c6e31f5fcfa79bea03f73896b25883abf2853d10337fb8fa41";

fn unhex(text: &str) -> Vec<u8> {
    (0..text.len() / 2)
        .map(|i| u8::from_str_radix(&text[2 * i..2 * i + 2], 16).unwrap())
        .collect()
}

fn fixture(name: &str) -> Vec<u8> {
    let path = format!("{}/tests/fixtures/tlock/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn fixture_text(name: &str) -> String {
    String::from_utf8(fixture(name)).unwrap()
}

fn randomness(seed: u8) -> [u8; 48] {
    let mut r = [0u8; 48];
    for (i, b) in r.iter_mut().enumerate() {
        *b = seed.wrapping_add(i as u8).wrapping_mul(7);
    }
    r
}

fn encrypt(plaintext: &[u8], seed: u8) -> String {
    tlock_encrypt(
        1000,
        &unhex(CHAIN_HASH),
        &unhex(PUBLIC_KEY),
        plaintext,
        &randomness(seed),
    )
    .unwrap()
}

#[test]
fn decrypts_tle_fixture_with_round_signature() {
    let plain = tlock_decrypt(&fixture_text("round1000.age"), &unhex(SIGNATURE_1000)).unwrap();
    assert_eq!(plain, fixture("plaintext.txt"));
}

#[test]
fn decrypts_multi_chunk_tle_fixture() {
    let plain = tlock_decrypt(&fixture_text("large1000.age"), &unhex(SIGNATURE_1000)).unwrap();
    assert_eq!(plain, fixture("large_plaintext.bin"));
}

#[test]
fn round_trip_guion_encrypt_decrypt() {
    let message = b"the sealed bid is 41";
    let armored = encrypt(message, 1);
    assert!(armored.starts_with("-----BEGIN AGE ENCRYPTED FILE-----\n"));
    assert!(armored.ends_with("-----END AGE ENCRYPTED FILE-----\n"));
    assert_eq!(
        tlock_decrypt(&armored, &unhex(SIGNATURE_1000)).unwrap(),
        message
    );
}

#[test]
fn round_trip_edge_sizes() {
    for len in [0usize, 1, 65535, 65536, 65537, 131072, 140000] {
        let message: Vec<u8> = (0..len).map(|i| (i % 253) as u8).collect();
        let armored = encrypt(&message, 2);
        assert_eq!(
            tlock_decrypt(&armored, &unhex(SIGNATURE_1000)).unwrap(),
            message,
            "length {len}"
        );
    }
}

#[test]
fn deterministic_given_randomness() {
    assert_eq!(encrypt(b"same", 3), encrypt(b"same", 3));
    assert_ne!(encrypt(b"same", 3), encrypt(b"same", 4));
}

#[test]
fn wrong_signature_is_decrypt_error() {
    let armored = encrypt(b"secret", 5);
    let error = tlock_decrypt(&armored, &unhex(SIGNATURE_1001)).unwrap_err();
    assert_eq!(error.code, "E_TLOCK_DECRYPT");
    let error = tlock_decrypt(&fixture_text("round1000.age"), &unhex(SIGNATURE_1001)).unwrap_err();
    assert_eq!(error.code, "E_TLOCK_DECRYPT");
}

/// Flips one base64 character of the armored text at `offset` within the line index `line`.
fn flip(armored: &str, line: usize, offset: usize) -> String {
    let mut lines: Vec<String> = armored.lines().map(str::to_string).collect();
    let mut chars: Vec<char> = lines[line].chars().collect();
    chars[offset] = if chars[offset] == 'A' { 'B' } else { 'A' };
    lines[line] = chars.into_iter().collect();
    lines.join("\n") + "\n"
}

#[test]
fn tampered_payload_is_decrypt_error() {
    let armored = encrypt(b"secret payload", 6);
    let last_body_line = armored.lines().count() - 2;
    let tampered = flip(&armored, last_body_line, 2);
    let error = tlock_decrypt(&tampered, &unhex(SIGNATURE_1000)).unwrap_err();
    assert_eq!(error.code, "E_TLOCK_DECRYPT");
}

#[test]
fn tampered_header_is_an_error() {
    let armored = encrypt(b"secret payload", 7);
    // Line 5 lies inside the stanza body (the IBE ciphertext) and line 8 inside the MAC line.
    for line in [3, 4, 5, 6, 7, 8] {
        let tampered = flip(&armored, line, 5);
        let error = tlock_decrypt(&tampered, &unhex(SIGNATURE_1000)).unwrap_err();
        assert!(
            ["E_TLOCK_DECRYPT", "E_TLOCK_FORMAT"].contains(&error.code),
            "line {line}: {error}"
        );
    }
}

#[test]
fn garbage_armor_is_format_error() {
    let long_armor = format!(
        "-----BEGIN AGE ENCRYPTED FILE-----\n{}",
        "A".repeat(100_000)
    );
    let cases = [
        "",
        "hello",
        "-----BEGIN AGE ENCRYPTED FILE-----\n-----END AGE ENCRYPTED FILE-----\n",
        "-----BEGIN AGE ENCRYPTED FILE-----\n!!!!\n-----END AGE ENCRYPTED FILE-----\n",
        "-----BEGIN AGE ENCRYPTED FILE-----\nYWdlLWVuY3J5cHRpb24ub3JnL3Yx\n-----END AGE ENCRYPTED FILE-----\n",
        long_armor.as_str(),
    ];
    for case in cases {
        let error = tlock_decrypt(case, &unhex(SIGNATURE_1000)).unwrap_err();
        assert_eq!(error.code, "E_TLOCK_FORMAT", "{case:?}");
        assert_eq!(tlock_round(case).unwrap_err().code, "E_TLOCK_FORMAT");
    }
}

#[test]
fn truncated_inputs_never_panic() {
    let armored = fixture_text("round1000.age");
    for cut in 0..armored.len() {
        if !armored.is_char_boundary(cut) {
            continue;
        }
        let _ = tlock_decrypt(&armored[..cut], &unhex(SIGNATURE_1000));
        let _ = tlock_round(&armored[..cut]);
    }
}

#[test]
fn bad_lengths_and_points_are_input_errors() {
    let armored = fixture_text("round1000.age");
    for bad in [&[][..], &[0u8; 47][..], &[0xFFu8; 48][..]] {
        assert_eq!(
            tlock_decrypt(&armored, bad).unwrap_err().code,
            "E_CRYPTO_INPUT"
        );
    }
    let key = unhex(PUBLIC_KEY);
    let chain = unhex(CHAIN_HASH);
    let r = randomness(0);
    assert_eq!(
        tlock_encrypt(1, &chain, &key[..95], b"x", &r)
            .unwrap_err()
            .code,
        "E_CRYPTO_INPUT"
    );
    assert_eq!(
        tlock_encrypt(1, &chain[..31], &key, b"x", &r)
            .unwrap_err()
            .code,
        "E_CRYPTO_INPUT"
    );
    assert_eq!(
        tlock_encrypt(1, &chain, &[0xFFu8; 96], b"x", &r)
            .unwrap_err()
            .code,
        "E_CRYPTO_INPUT"
    );
    // The point at infinity as a public key is refused.
    let mut infinity = vec![0u8; 96];
    infinity[0] = 0xC0;
    assert_eq!(
        tlock_encrypt(1, &chain, &infinity, b"x", &r)
            .unwrap_err()
            .code,
        "E_CRYPTO_INPUT"
    );
}

#[test]
fn round_reads_stanza() {
    let (round, chain) = tlock_round(&fixture_text("round1000.age")).unwrap();
    assert_eq!(round, 1000);
    assert_eq!(chain, unhex(CHAIN_HASH));
    let (round, _) = tlock_round(&encrypt(b"x", 9)).unwrap();
    assert_eq!(round, 1000);
    let (future, chain) = tlock_round(&fixture_text("future.age")).unwrap();
    assert_eq!(future.to_string(), fixture_text("future_round.txt"));
    assert_eq!(chain, unhex(CHAIN_HASH));
}

/// Needs Go `tle` v1.2.0 on PATH (or `TLE` set to it) and network access: `tle --decrypt` fetches
/// the round signature from api.drand.sh. Run with `cargo test -p guion-crypto --test
/// tlock_interop -- --ignored --nocapture`.
#[test]
#[ignore]
fn tle_decrypts_guion_output() {
    use std::io::Write;
    let tle = std::env::var("TLE").unwrap_or_else(|_| "tle".to_string());
    let message: Vec<u8> = (0..70_000usize).map(|i| (i % 249) as u8).collect();
    for plaintext in [b"hello from guion".to_vec(), Vec::new(), message] {
        let armored = encrypt(&plaintext, 11);
        let dir = std::env::temp_dir().join(format!("guion-tle-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let input = dir.join("in.age");
        let output = dir.join("out.bin");
        std::fs::File::create(&input)
            .unwrap()
            .write_all(armored.as_bytes())
            .unwrap();
        let status = std::process::Command::new(&tle)
            .args(["--decrypt", "-o"])
            .arg(&output)
            .arg(&input)
            .status()
            .expect("run tle");
        assert!(
            status.success(),
            "tle failed on {} byte plaintext",
            plaintext.len()
        );
        assert_eq!(std::fs::read(&output).unwrap(), plaintext);
        println!(
            "tle v1.2.0 decrypted Guion output: {} bytes OK",
            plaintext.len()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
