//! RFC 3161 vectors: live tokens from DigiCert, Sectigo and FreeTSA (each also checked with
//! `openssl ts -verify` or `openssl cms -verify`, see fixtures/tsa/README.md) and tokens from a local
//! test TSA (fixtures/tsa/local/make.sh) for every way a token can be wrong.

use guion_crypto::{
    sha512, tsa_certificates, tsa_request, tsa_response_token, tsa_verify, TsaToken,
};

fn path(relative: &str) -> String {
    format!(
        "{}/tests/fixtures/tsa/{relative}",
        env!("CARGO_MANIFEST_DIR")
    )
}

fn file(relative: &str) -> Vec<u8> {
    let path = path(relative);
    std::fs::read(&path).unwrap_or_else(|error| panic!("{path}: {error}"))
}

fn base64_decode(text: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let mut accumulator = 0u32;
    let mut bits = 0;
    for byte in text.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => continue,
        };
        accumulator = (accumulator << 6) | u32::from(value);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((accumulator >> bits) as u8);
        }
    }
    out
}

/// The DER of the first certificate in a PEM file.
fn pem(relative: &str) -> Vec<u8> {
    let text = String::from_utf8(file(relative)).expect("PEM is text");
    let body: String = text
        .lines()
        .skip_while(|line| !line.starts_with("-----BEGIN CERTIFICATE"))
        .skip(1)
        .take_while(|line| !line.starts_with("-----END"))
        .collect();
    base64_decode(&body)
}

fn digest(relative: &str) -> Vec<u8> {
    sha512(&file(relative)).to_vec()
}

fn token_of(relative: &str) -> Vec<u8> {
    tsa_response_token(&file(relative)).unwrap_or_else(|error| panic!("{relative}: {error}"))
}

const LIVE: [&str; 3] = ["digicert", "sectigo", "freetsa"];

fn local(token: &str, tsa: &str) -> Result<Option<TsaToken>, guion_crypto::CryptoError> {
    tsa_verify(
        &token_of(&format!("local/{token}.tsr")),
        &digest("local/message.txt"),
        &pem(&format!("local/{tsa}.pem")),
        &pem("local/ca.pem"),
    )
}

fn local_ok(token: &str) -> TsaToken {
    local(token, "tsa1")
        .unwrap_or_else(|error| panic!("{token}: {error}"))
        .unwrap_or_else(|| panic!("{token} did not verify"))
}

fn local_none(token: &str, tsa: &str) {
    assert_eq!(
        local(token, tsa).unwrap_or_else(|error| panic!("{token}: {error}")),
        None,
        "{token} against {tsa}"
    );
}

#[test]
fn request_matches_openssl_without_nonce() {
    for provider in LIVE {
        let message = digest(&format!("{provider}/message.txt"));
        assert_eq!(
            tsa_request(&message, &[]).unwrap(),
            file(&format!("{provider}/request.tsq")),
            "{provider}"
        );
    }
    for (message, request) in [
        ("local/message.txt", "local/request.tsq"),
        ("local/message2.txt", "local/request2.tsq"),
    ] {
        assert_eq!(
            tsa_request(&digest(message), &[]).unwrap(),
            file(request),
            "{request}"
        );
    }
}

/// The nonce INTEGER of a request, read back with a minimal DER walk.
fn request_nonce(request: &[u8]) -> Option<Vec<u8>> {
    // SEQUENCE { INTEGER 1, SEQUENCE imprint, [INTEGER nonce], BOOLEAN }
    assert_eq!(request[0], 0x30);
    let mut position = 2; // the outer length is short (< 0x80) for these requests
    let mut nonce = None;
    while position < request.len() {
        let tag = request[position];
        let length = usize::from(request[position + 1]);
        let content = &request[position + 2..position + 2 + length];
        if tag == 0x02 && position > 2 {
            nonce = Some(content.to_vec());
        }
        position += 2 + length;
    }
    assert_eq!(position, request.len());
    nonce
}

#[test]
fn request_nonce_round_trips() {
    let message = digest("digicert/message.txt");
    let nonce: Vec<u8> = (1..=8).collect();
    let request = tsa_request(&message, &nonce).unwrap();
    assert_eq!(request_nonce(&request), Some(nonce));
    let longest = vec![0x7f; 32];
    let request = tsa_request(&message, &longest).unwrap();
    assert_eq!(request[1] as usize + 2, request.len());
    assert!(request
        .windows(34)
        .any(|window| window[0] == 2 && window[1] == 32 && window[2..] == longest[..]));
    assert_eq!(request_nonce(&tsa_request(&message, &[]).unwrap()), None);
    for bad in [
        vec![1u8; 33],
        vec![0, 1],
        vec![0x80],
        vec![0xff, 1],
        vec![0],
    ] {
        assert_eq!(
            tsa_request(&message, &bad).unwrap_err().code,
            "E_TSA_NONCE",
            "{bad:?}"
        );
    }
}

#[test]
fn request_bad_digest_is_error() {
    for length in [0usize, 32, 63, 65] {
        assert_eq!(
            tsa_request(&vec![7; length], &[]).unwrap_err().code,
            "E_TSA_DIGEST"
        );
    }
}

/// (provider, `genTime` as printed by `openssl ts -reply -text`, serial in hex, policy OID).
const LIVE_EXPECTED: [(&str, i64, &str, &str); 3] = [
    // Nov 23 19:40:59 2026 GMT
    (
        "digicert",
        1_795_462_859_000,
        "33a1b07c58ab304d70f5dc92fe4a5ae5",
        "2.16.840.1.114412.7.1",
    ),
    // Nov 23 19:41:01 2026 GMT
    (
        "sectigo",
        1_795_462_861_000,
        "f29c67357aff76a335019e2574ed5c19e8925e06",
        "1.3.6.1.4.1.6449.2.1.1",
    ),
    // Nov 23 19:41:03 2026 GMT
    ("freetsa", 1_795_462_863_000, "0a76a713", "1.2.3.4.1"),
];

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[test]
fn live_tokens_verify() {
    for (provider, gen_time_ms, serial, policy) in LIVE_EXPECTED {
        let token = token_of(&format!("{provider}/response.tsr"));
        let verified = tsa_verify(
            &token,
            &digest(&format!("{provider}/message.txt")),
            &pem(&format!("{provider}/tsa.pem")),
            &pem(&format!("{provider}/issuer.pem")),
        )
        .unwrap_or_else(|error| panic!("{provider}: {error}"))
        .unwrap_or_else(|| panic!("{provider} did not verify"));
        assert_eq!(verified.gen_time_ms, gen_time_ms, "{provider}");
        assert_eq!(verified.accuracy_ms, 0, "{provider}");
        assert_eq!(hex(&verified.serial), serial, "{provider}");
        assert_eq!(verified.policy, policy, "{provider}");
        assert!(verified.nonce.is_empty(), "{provider}");
    }
}

#[test]
fn live_tokens_do_not_verify_for_another_provider() {
    let token = token_of("digicert/response.tsr");
    let message = digest("digicert/message.txt");
    // Another provider's certificate and issuer.
    assert_eq!(
        tsa_verify(
            &token,
            &message,
            &pem("sectigo/tsa.pem"),
            &pem("sectigo/issuer.pem")
        )
        .unwrap(),
        None
    );
    // The right certificate under the wrong issuer.
    assert_eq!(
        tsa_verify(
            &token,
            &message,
            &pem("digicert/tsa.pem"),
            &pem("sectigo/issuer.pem")
        )
        .unwrap(),
        None
    );
    // Another message.
    assert_eq!(
        tsa_verify(
            &token,
            &digest("sectigo/message.txt"),
            &pem("digicert/tsa.pem"),
            &pem("digicert/issuer.pem")
        )
        .unwrap(),
        None
    );
}

#[test]
fn wrong_digest_is_none() {
    local_none("other-digest", "tsa1");
    // The good token, but a digest of different bytes.
    assert_eq!(
        tsa_verify(
            &token_of("local/good-v2.tsr"),
            &digest("local/message2.txt"),
            &pem("local/tsa1.pem"),
            &pem("local/ca.pem"),
        )
        .unwrap(),
        None
    );
    // A token whose imprint is SHA-256 of the same message.
    local_none("sha256-imprint", "tsa1");
}

#[test]
fn other_tsa_cert_same_issuer_is_none() {
    // A token from TSA 2, checked against the pinned TSA 1 certificate, and the other way round.
    local_none("tsa2", "tsa1");
    local_none("good-v2", "tsa2");
    assert!(local("tsa2", "tsa2").unwrap().is_some());
}

#[test]
fn wrong_issuer_is_none() {
    let verified = tsa_verify(
        &token_of("local/good-v2.tsr"),
        &digest("local/message.txt"),
        &pem("local/tsa1.pem"),
        &pem("local/other-ca.pem"),
    )
    .unwrap();
    assert_eq!(verified, None);
}

#[test]
fn missing_eku_is_none() {
    // Well signed, ESS id right, certificate issued by the CA, but no timeStamping EKU.
    local_none("noeku", "tsa-noeku");
}

#[test]
fn ess_cert_id_mismatch_and_absence_are_none() {
    // Signed by TSA 1 with a valid signature, but the ESS id names TSA 2's certificate.
    local_none("ess-mismatch", "tsa1");
    // No signingCertificate attribute at all.
    local_none("no-ess", "tsa1");
}

#[test]
fn unsupported_signer_digest_is_an_alg_error() {
    let error = local("sha1-signer", "tsa1").unwrap_err();
    assert_eq!(error.code, "E_TSA_ALG");
}

#[test]
fn rebuilt_token_matches_the_forging_script() {
    // forge.py rebuilds a token around good-v2's TSTInfo with the same signer; it verifies, so the
    // negative forged tokens differ from it only in the one thing each test names.
    assert!(local("rebuilt", "tsa1").unwrap().is_some());
}

/// The offset of `needle` in `haystack`.
fn find(haystack: &[u8], needle: &[u8]) -> usize {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
        .expect("needle present")
}

#[test]
fn tampered_tstinfo_byte_is_none_or_format() {
    let token = token_of("local/good-v2.tsr");
    let digest = digest("local/message.txt");
    let (tsa, ca) = (pem("local/tsa1.pem"), pem("local/ca.pem"));
    // Every byte of the genTime and the serial number, and one of the policy.
    let time = find(&token, b"20261123");
    let mut offsets: Vec<usize> = (time..time + 15).collect();
    offsets.push(find(&token, &[0x06, 0x04, 0x2a, 0x03, 0x04, 0x01]) + 5);
    let mut changed = 0;
    for offset in offsets {
        let mut bytes = token.clone();
        bytes[offset] ^= 0x01;
        match tsa_verify(&bytes, &digest, &tsa, &ca) {
            Ok(None) => changed += 1,
            Err(error) => {
                assert_eq!(error.code, "E_TSA_FORMAT", "offset {offset}");
                changed += 1;
            }
            Ok(Some(_)) => panic!("a tampered TSTInfo verified at offset {offset}"),
        }
    }
    assert_eq!(changed, 16);
}

#[test]
fn tampered_signature_is_none() {
    let mut token = token_of("local/good-v2.tsr");
    let last = token.len() - 1;
    token[last] ^= 0x01;
    assert_eq!(
        tsa_verify(
            &token,
            &digest("local/message.txt"),
            &pem("local/tsa1.pem"),
            &pem("local/ca.pem")
        )
        .unwrap(),
        None
    );
    // And one bit in the middle of the signature value.
    let mut token = token_of("local/good-v2.tsr");
    let middle = token.len() - 100;
    token[middle] ^= 0x80;
    assert_eq!(
        tsa_verify(
            &token,
            &digest("local/message.txt"),
            &pem("local/tsa1.pem"),
            &pem("local/ca.pem")
        )
        .unwrap(),
        None
    );
}

#[test]
fn tampered_signed_attributes_are_none_or_format() {
    // The signed attributes follow the certificates; flip a byte of the messageDigest value.
    let token = token_of("local/good-v2.tsr");
    let marker = find(
        &token,
        &[
            0x06, 0x09, 0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x09, 0x04,
        ],
    );
    let mut bytes = token.clone();
    bytes[marker + 20] ^= 0x01;
    match tsa_verify(
        &bytes,
        &digest("local/message.txt"),
        &pem("local/tsa1.pem"),
        &pem("local/ca.pem"),
    ) {
        Ok(None) => {}
        Err(error) => assert_eq!(error.code, "E_TSA_FORMAT"),
        Ok(Some(_)) => panic!("tampered signed attributes verified"),
    }
}

#[test]
fn gen_time_outside_cert_validity_is_none() {
    // The TSA certificate was valid only in 2020; the token says 2026.
    let token = token_of("local/expired.tsr");
    let verified = tsa_verify(
        &token,
        &digest("local/message.txt"),
        &pem("local/tsa-expired.pem"),
        &pem("local/ca.pem"),
    )
    .unwrap();
    assert_eq!(verified, None);
}

#[test]
fn ess_cert_id_v1_and_v2_both_accepted() {
    let v2 = local_ok("good-v2");
    let v1 = local_ok("good-v1");
    // Nov 23 19:48:31 2026 GMT (1795463311), serials 2 and 3.
    assert_eq!(v2.gen_time_ms, 1_795_463_311_000);
    assert_eq!(v1.gen_time_ms, 1_795_463_311_000);
    assert_eq!(v2.serial, vec![2]);
    assert_eq!(v1.serial, vec![3]);
    assert_eq!(v2.policy, "1.2.3.4.1");
}

#[test]
fn accuracy_and_fraction_to_ms() {
    // accuracy secs:1, millisecs:500, microsecs:100 -> 1000 + 500 + ceil(0.1) = 1501;
    // genTime 19:48:31.414 (three fractional digits).
    let accurate = local_ok("accuracy");
    assert_eq!(accurate.accuracy_ms, 1501);
    assert_eq!(accurate.gen_time_ms, 1_795_463_311_414);
    assert_eq!(accurate.serial, vec![8]);
    // Six fractional digits (.427545) truncate to milliseconds.
    let precise = local_ok("precision6");
    assert_eq!(precise.gen_time_ms, 1_795_463_311_427);
    assert_eq!(precise.accuracy_ms, 0);
}

#[test]
fn every_supported_signature_algorithm_verifies() {
    // The same TSTInfo, signed by forge.py with each algorithm (also accepted by `openssl cms -verify`).
    for token in ["rsa-sha512", "rsa-sha384", "pss"] {
        let verified = local_ok(token);
        assert_eq!(verified.gen_time_ms, 1_795_463_311_000, "{token}");
    }
    for (token, cert) in [("p256", "tsa-p256"), ("p384", "tsa-p384")] {
        let verified = local(token, cert)
            .unwrap_or_else(|error| panic!("{token}: {error}"))
            .unwrap_or_else(|| panic!("{token} did not verify"));
        assert_eq!(verified.gen_time_ms, 1_795_463_311_000, "{token}");
    }
    // A P-256 token against the RSA certificate, and the reverse, is not a match.
    local_none("p256", "tsa1");
    local_none("good-v2", "tsa-p256");
}

#[test]
fn rejection_status_is_error() {
    let response = file("local/rejection.tsr");
    let error = tsa_response_token(&response).unwrap_err();
    assert_eq!(error.code, "E_TSA_STATUS");
    assert!(error.message.contains("status 2"), "{}", error.message);
    assert!(error.message.contains("rejection"), "{}", error.message);
    assert!(
        error.message.contains("Requested policy is not supported"),
        "{}",
        error.message
    );
    assert!(
        error.message.contains("unacceptedPolicy"),
        "{}",
        error.message
    );
}

#[test]
fn garbage_is_format_error() {
    let digest = digest("local/message.txt");
    let (tsa, ca) = (pem("local/tsa1.pem"), pem("local/ca.pem"));
    let check = |input: &[u8], what: &str| {
        let error =
            tsa_verify(input, &digest, &tsa, &ca).expect_err(&format!("{what} should be an error"));
        assert_eq!(error.code, "E_TSA_FORMAT", "{what}: {error}");
        assert_eq!(
            tsa_certificates(input).unwrap_err().code,
            "E_TSA_FORMAT",
            "{what}"
        );
    };
    check(&[], "empty");
    check(&[0x30], "one byte");
    check(&tsa, "a certificate as the token");
    let token = token_of("local/good-v2.tsr");
    for cut in 1..=10 {
        let length = token.len() * cut / 11;
        check(&token[..length], &format!("token cut at {length}"));
    }
    // A response is not a token, and a token is not a response.
    check(&file("local/good-v2.tsr"), "a whole response as the token");
    assert_eq!(tsa_response_token(&[]).unwrap_err().code, "E_TSA_FORMAT");
    assert_eq!(
        tsa_response_token(&[0x30]).unwrap_err().code,
        "E_TSA_FORMAT"
    );
    assert_eq!(tsa_response_token(&token).unwrap_err().code, "E_TSA_FORMAT");
    let response = file("local/good-v2.tsr");
    for cut in 1..=10 {
        let length = response.len() * cut / 11;
        assert_eq!(
            tsa_response_token(&response[..length]).unwrap_err().code,
            "E_TSA_FORMAT"
        );
    }
}

#[test]
fn unreadable_certificates_are_x509_errors() {
    let token = token_of("local/good-v2.tsr");
    let digest = digest("local/message.txt");
    let ca = pem("local/ca.pem");
    assert_eq!(
        tsa_verify(&token, &digest, &[1, 2, 3], &ca)
            .unwrap_err()
            .code,
        "E_X509"
    );
    assert_eq!(
        tsa_verify(&token, &digest, &pem("local/tsa1.pem"), &[])
            .unwrap_err()
            .code,
        "E_X509"
    );
}

#[test]
fn certificates_extracted() {
    let token = token_of("digicert/response.tsr");
    let certificates = tsa_certificates(&token).unwrap();
    assert!(!certificates.is_empty());
    assert_eq!(certificates[0], pem("digicert/tsa.pem"));
    assert_eq!(certificates[1], pem("digicert/issuer.pem"));
    // A token with no certificates field gives none: the local TSA embeds the TSA certificate.
    assert_eq!(
        tsa_certificates(&token_of("local/good-v2.tsr")).unwrap()[0],
        pem("local/tsa1.pem")
    );
}
