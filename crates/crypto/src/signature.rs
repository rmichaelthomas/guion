//! Signature verification: RSA (PKCS#1 v1.5 and PSS), ECDSA P-256 and RSA keys from X.509.
//!
//! Everything here only checks signatures against public keys. A malformed key, signature
//! length or certificate is a `CryptoError`; a well-formed input that does not verify is
//! `Ok(false)`. Nothing here panics.

use p256::ecdsa::signature::Verifier;
use p256::ecdsa::{Signature as EcSignature, VerifyingKey as EcVerifyingKey};
use rsa::pkcs1::der::Decode;
use rsa::pkcs1::RsaPublicKey as Pkcs1PublicKey;
use rsa::traits::PublicKeyParts;
use rsa::{pkcs1v15, pss, BigUint, RsaPublicKey};
use sha2::{Sha256, Sha512};
use x509_cert::Certificate;

use crate::CryptoError;

/// An RSA signature scheme accepted by [`rsa_verify`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RsaScheme {
    /// RSASSA-PKCS1-v1_5 with SHA-256.
    Pkcs1v15Sha256,
    /// RSASSA-PKCS1-v1_5 with SHA-512.
    Pkcs1v15Sha512,
    /// RSASSA-PSS with SHA-256 and MGF1(SHA-256); the salt length is taken from the signature.
    PssSha256,
}

impl RsaScheme {
    /// The scheme named by a Guion program: `pkcs1v15-sha256`, `pkcs1v15-sha512` or `pss-sha256`.
    pub fn from_name(name: &str) -> Result<RsaScheme, CryptoError> {
        match name {
            "pkcs1v15-sha256" => Ok(RsaScheme::Pkcs1v15Sha256),
            "pkcs1v15-sha512" => Ok(RsaScheme::Pkcs1v15Sha512),
            "pss-sha256" => Ok(RsaScheme::PssSha256),
            _ => Err(CryptoError::new(
                "E_CRYPTO_SCHEME",
                "unknown RSA scheme; use \"pkcs1v15-sha256\", \"pkcs1v15-sha512\" or \"pss-sha256\"",
            )),
        }
    }
}

const MIN_MODULUS_BITS: usize = 1024;
const MAX_MODULUS_BITS: usize = 8192;
const MAX_EXPONENT_BYTES: usize = 5;

fn input(message: &str) -> CryptoError {
    CryptoError::new("E_CRYPTO_INPUT", message)
}

fn strip_leading_zeros(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|byte| *byte != 0)
        .unwrap_or(bytes.len());
    &bytes[start..]
}

/// Verifies an RSA signature. `modulus` and `exponent` are big-endian (leading zero bytes are
/// ignored); `signature` must be exactly as long as the modulus.
pub fn rsa_verify(
    modulus: &[u8],
    exponent: &[u8],
    message: &[u8],
    signature: &[u8],
    scheme: RsaScheme,
) -> Result<bool, CryptoError> {
    let modulus = strip_leading_zeros(modulus);
    let exponent = strip_leading_zeros(exponent);
    if modulus.is_empty() || exponent.is_empty() {
        return Err(input("RSA modulus and exponent must not be empty or zero"));
    }
    if modulus.len() * 8 < MIN_MODULUS_BITS || modulus.len() * 8 > MAX_MODULUS_BITS + 8 {
        return Err(input("RSA modulus must be between 1024 and 8192 bits"));
    }
    if modulus[modulus.len() - 1] & 1 == 0 {
        return Err(input("RSA modulus must be odd"));
    }
    if exponent.len() > MAX_EXPONENT_BYTES
        || exponent[exponent.len() - 1] & 1 == 0
        || (exponent.len() == 1 && exponent[0] < 3)
    {
        return Err(input(
            "RSA exponent must be an odd number of at least 3 and under 2^33",
        ));
    }
    let key = RsaPublicKey::new(
        BigUint::from_bytes_be(modulus),
        BigUint::from_bytes_be(exponent),
    )
    .map_err(|error| input(&format!("invalid RSA public key: {error}")))?;
    if signature.len() != key.size() {
        return Err(input(&format!(
            "RSA signature must be {} bytes, the length of the modulus; got {}",
            key.size(),
            signature.len()
        )));
    }
    let valid = match scheme {
        RsaScheme::Pkcs1v15Sha256 => pkcs1v15::Signature::try_from(signature)
            .and_then(|parsed| pkcs1v15::VerifyingKey::<Sha256>::new(key).verify(message, &parsed))
            .is_ok(),
        RsaScheme::Pkcs1v15Sha512 => pkcs1v15::Signature::try_from(signature)
            .and_then(|parsed| pkcs1v15::VerifyingKey::<Sha512>::new(key).verify(message, &parsed))
            .is_ok(),
        RsaScheme::PssSha256 => pss::Signature::try_from(signature)
            .and_then(|parsed| pss::VerifyingKey::<Sha256>::new(key).verify(message, &parsed))
            .is_ok(),
    };
    Ok(valid)
}

/// Verifies an ECDSA P-256 signature over SHA-256 of `message`. `sec1_public_key` is a SEC1
/// encoded point (uncompressed 65 bytes, or compressed 33) that must lie on the curve;
/// `signature_r_s` is the raw 64-byte `r || s` form that JWS uses.
pub fn ecdsa_p256_verify(
    sec1_public_key: &[u8],
    message: &[u8],
    signature_r_s: &[u8],
) -> Result<bool, CryptoError> {
    let key = EcVerifyingKey::from_sec1_bytes(sec1_public_key).map_err(|_| {
        input("not a valid SEC1 P-256 public key (wrong length or not on the curve)")
    })?;
    if signature_r_s.len() != 64 {
        return Err(input(&format!(
            "ECDSA P-256 signature must be 64 bytes (r then s); got {}",
            signature_r_s.len()
        )));
    }
    // An r or s of zero, or at least the group order, is well-formed in length but never valid.
    let Ok(signature) = EcSignature::from_slice(signature_r_s) else {
        return Ok(false);
    };
    Ok(key.verify(message, &signature).is_ok())
}

/// The RSA public key of an X.509 certificate (DER) as big-endian `(modulus, exponent)` with no
/// leading zero bytes. A certificate that does not parse, or whose key is not RSA, is `E_X509`.
pub fn x509_rsa_public_key(der: &[u8]) -> Result<(Vec<u8>, Vec<u8>), CryptoError> {
    const RSA_ENCRYPTION: &str = "1.2.840.113549.1.1.1";
    let x509 = |message: String| CryptoError::new("E_X509", message);
    let certificate = Certificate::from_der(der)
        .map_err(|error| x509(format!("not a valid DER X.509 certificate: {error}")))?;
    let info = &certificate.tbs_certificate.subject_public_key_info;
    if info.algorithm.oid.to_string() != RSA_ENCRYPTION {
        return Err(x509(format!(
            "certificate key is not RSA (algorithm {})",
            info.algorithm.oid
        )));
    }
    let raw = info
        .subject_public_key
        .as_bytes()
        .ok_or_else(|| x509("certificate RSA key is not a whole number of bytes".to_string()))?;
    let key = Pkcs1PublicKey::from_der(raw)
        .map_err(|error| x509(format!("certificate RSA key is malformed: {error}")))?;
    Ok((
        strip_leading_zeros(key.modulus.as_bytes()).to_vec(),
        strip_leading_zeros(key.public_exponent.as_bytes()).to_vec(),
    ))
}
