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
use sha2::{Digest, Sha256, Sha384, Sha512};
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
    /// RSASSA-PKCS1-v1_5 with SHA-384. Reached by certificate checks, not by name from a program.
    Pkcs1v15Sha384,
    /// RSASSA-PSS with SHA-384 and MGF1(SHA-384). Reached by certificate checks only.
    PssSha384,
    /// RSASSA-PSS with SHA-512 and MGF1(SHA-512). Reached by certificate checks only.
    PssSha512,
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
        RsaScheme::Pkcs1v15Sha384 => pkcs1v15::Signature::try_from(signature)
            .and_then(|parsed| pkcs1v15::VerifyingKey::<Sha384>::new(key).verify(message, &parsed))
            .is_ok(),
        RsaScheme::PssSha384 => pss::Signature::try_from(signature)
            .and_then(|parsed| pss::VerifyingKey::<Sha384>::new(key).verify(message, &parsed))
            .is_ok(),
        RsaScheme::PssSha512 => pss::Signature::try_from(signature)
            .and_then(|parsed| pss::VerifyingKey::<Sha512>::new(key).verify(message, &parsed))
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

const RSA_ENCRYPTION_OID: &str = "1.2.840.113549.1.1.1";
const EC_PUBLIC_KEY_OID: &str = "1.2.840.10045.2.1";
const P256_OID: &str = "1.2.840.10045.3.1.7";
const P384_OID: &str = "1.3.132.0.34";
const RSA_PSS_OID: &str = "1.2.840.113549.1.1.10";
const MGF1_OID: &str = "1.2.840.113549.1.1.8";
const SHA256_OID: &str = "2.16.840.1.101.3.4.2.1";
const SHA384_OID: &str = "2.16.840.1.101.3.4.2.2";
const SHA512_OID: &str = "2.16.840.1.101.3.4.2.3";

/// A hash named by an X.509 signature algorithm.
#[derive(Clone, Copy, PartialEq, Eq)]
enum CertHash {
    Sha256,
    Sha384,
    Sha512,
}

impl CertHash {
    fn from_oid(oid: &str) -> Option<CertHash> {
        match oid {
            SHA256_OID => Some(CertHash::Sha256),
            SHA384_OID => Some(CertHash::Sha384),
            SHA512_OID => Some(CertHash::Sha512),
            _ => None,
        }
    }

    fn digest(self, message: &[u8]) -> Vec<u8> {
        match self {
            CertHash::Sha256 => Sha256::digest(message).to_vec(),
            CertHash::Sha384 => Sha384::digest(message).to_vec(),
            CertHash::Sha512 => Sha512::digest(message).to_vec(),
        }
    }
}

/// How a certificate says it was signed.
enum CertSignature {
    RsaPkcs1v15(CertHash),
    RsaPss(CertHash),
    Ecdsa(CertHash),
}

fn x509_error(message: impl Into<String>) -> CryptoError {
    CryptoError::new("E_X509", message)
}

fn x509_alg(message: impl Into<String>) -> CryptoError {
    CryptoError::new("E_X509_ALG", message)
}

fn certificate_signature_algorithm(
    certificate: &Certificate,
) -> Result<CertSignature, CryptoError> {
    use rsa::pkcs1::der::Encode;
    let algorithm = &certificate.signature_algorithm;
    let oid = algorithm.oid.to_string();
    let rsa = |hash| Ok(CertSignature::RsaPkcs1v15(hash));
    let ecdsa = |hash| Ok(CertSignature::Ecdsa(hash));
    match oid.as_str() {
        "1.2.840.113549.1.1.11" => rsa(CertHash::Sha256),
        "1.2.840.113549.1.1.12" => rsa(CertHash::Sha384),
        "1.2.840.113549.1.1.13" => rsa(CertHash::Sha512),
        "1.2.840.10045.4.3.2" => ecdsa(CertHash::Sha256),
        "1.2.840.10045.4.3.3" => ecdsa(CertHash::Sha384),
        "1.2.840.10045.4.3.4" => ecdsa(CertHash::Sha512),
        RSA_PSS_OID => {
            let parameters = algorithm
                .parameters
                .as_ref()
                .ok_or_else(|| x509_alg("RSASSA-PSS certificate without parameters (SHA-1 default)"))?;
            let encoded = parameters
                .to_der()
                .map_err(|error| x509_error(format!("RSASSA-PSS parameters: {error}")))?;
            let parsed = rsa::pkcs1::RsaPssParams::from_der(&encoded)
                .map_err(|error| x509_error(format!("RSASSA-PSS parameters: {error}")))?;
            let hash = CertHash::from_oid(&parsed.hash.oid.to_string())
                .ok_or_else(|| x509_alg(format!("RSASSA-PSS hash {} is not supported", parsed.hash.oid)))?;
            let mask_hash = parsed
                .mask_gen
                .parameters
                .as_ref()
                .and_then(|inner| CertHash::from_oid(&inner.oid.to_string()));
            if parsed.mask_gen.oid.to_string() != MGF1_OID || mask_hash != Some(hash) {
                return Err(x509_alg(
                    "RSASSA-PSS needs MGF1 with the same hash as the message digest",
                ));
            }
            Ok(CertSignature::RsaPss(hash))
        }
        other => Err(x509_alg(format!(
            "certificate signature algorithm {other} is not supported (RSA PKCS#1 v1.5 or PSS with SHA-256, SHA-384 or SHA-512, ECDSA with SHA-256, SHA-384 or SHA-512)"
        ))),
    }
}

/// The bytes of the `tbsCertificate` exactly as they appear in the certificate (a signature covers
/// the encoding that was signed, so it is cut from the input rather than encoded again).
fn raw_tbs_certificate(der: &[u8]) -> Option<&[u8]> {
    fn header(data: &[u8]) -> Option<(usize, usize)> {
        // Returns (header length, content length) of the DER element at the start of `data`.
        let first = *data.get(1)?;
        if first < 0x80 {
            return Some((2, usize::from(first)));
        }
        let count = usize::from(first & 0x7F);
        if count == 0 || count > 4 {
            return None;
        }
        let mut length = 0usize;
        for byte in data.get(2..2 + count)? {
            length = (length << 8) | usize::from(*byte);
        }
        Some((2 + count, length))
    }
    if *der.first()? != 0x30 {
        return None;
    }
    let (outer_header, _) = header(der)?;
    let inner = der.get(outer_header..)?;
    if *inner.first()? != 0x30 {
        return None;
    }
    let (inner_header, content) = header(inner)?;
    inner.get(..inner_header.checked_add(content)?)
}

/// Whether `certificate_der` was signed by the key in `issuer_der`: the certificate's issuer name
/// is the issuer certificate's subject name, and the signature over the certificate's
/// `tbsCertificate` verifies under the issuer's public key. Both must be DER X.509 certificates
/// (`E_X509` if not). The algorithm is the one the certificate names: RSA PKCS#1 v1.5 or PSS with
/// SHA-256, SHA-384 or SHA-512 under an RSA issuer key; ECDSA with SHA-256, SHA-384 or SHA-512
/// under a P-256 or P-384 issuer key. Any other algorithm or key type (SHA-1, P-521, Ed25519) is
/// `E_X509_ALG`. A name that does not match, a key of the wrong type for the algorithm or a
/// signature that does not verify is `Ok(false)`. Validity dates, extensions and revocation are
/// not examined.
pub fn x509_verify_issued_by(
    certificate_der: &[u8],
    issuer_der: &[u8],
) -> Result<bool, CryptoError> {
    use rsa::pkcs1::der::Encode;
    let certificate = Certificate::from_der(certificate_der)
        .map_err(|error| x509_error(format!("not a valid DER X.509 certificate: {error}")))?;
    let issuer = Certificate::from_der(issuer_der).map_err(|error| {
        x509_error(format!(
            "issuer is not a valid DER X.509 certificate: {error}"
        ))
    })?;
    let tbs = raw_tbs_certificate(certificate_der)
        .ok_or_else(|| x509_error("certificate structure is malformed"))?;
    let algorithm = certificate_signature_algorithm(&certificate)?;
    let issuer_name = certificate
        .tbs_certificate
        .issuer
        .to_der()
        .map_err(|error| x509_error(format!("certificate issuer name: {error}")))?;
    let subject_name = issuer
        .tbs_certificate
        .subject
        .to_der()
        .map_err(|error| x509_error(format!("issuer subject name: {error}")))?;
    let info = &issuer.tbs_certificate.subject_public_key_info;
    let key_oid = info.algorithm.oid.to_string();
    // Reject algorithms and curves outside the supported set before saying anything about names.
    let curve = if key_oid == EC_PUBLIC_KEY_OID {
        let parameters = info
            .algorithm
            .parameters
            .as_ref()
            .ok_or_else(|| x509_error("issuer EC key has no curve"))?;
        let curve: rsa::pkcs1::der::asn1::ObjectIdentifier = parameters
            .decode_as()
            .map_err(|error| x509_error(format!("issuer EC key curve: {error}")))?;
        let curve = curve.to_string();
        if curve != P256_OID && curve != P384_OID {
            return Err(x509_alg(format!(
                "issuer EC curve {curve} is not supported (P-256 and P-384 are)"
            )));
        }
        Some(curve)
    } else if key_oid == RSA_ENCRYPTION_OID {
        None
    } else {
        return Err(x509_alg(format!(
            "issuer key algorithm {key_oid} is not supported (RSA, P-256 and P-384 are)"
        )));
    };
    if issuer_name != subject_name {
        return Ok(false);
    }
    let signature = certificate
        .signature
        .as_bytes()
        .ok_or_else(|| x509_error("certificate signature is not a whole number of bytes"))?;
    match (algorithm, curve) {
        (rsa_algorithm @ (CertSignature::RsaPkcs1v15(_) | CertSignature::RsaPss(_)), None) => {
            let scheme = match rsa_algorithm {
                CertSignature::RsaPkcs1v15(CertHash::Sha256) => RsaScheme::Pkcs1v15Sha256,
                CertSignature::RsaPkcs1v15(CertHash::Sha384) => RsaScheme::Pkcs1v15Sha384,
                CertSignature::RsaPkcs1v15(CertHash::Sha512) => RsaScheme::Pkcs1v15Sha512,
                CertSignature::RsaPss(CertHash::Sha256) => RsaScheme::PssSha256,
                CertSignature::RsaPss(CertHash::Sha384) => RsaScheme::PssSha384,
                CertSignature::RsaPss(CertHash::Sha512) => RsaScheme::PssSha512,
                CertSignature::Ecdsa(_) => return Ok(false),
            };
            let (modulus, exponent) = x509_rsa_public_key(issuer_der)?;
            if signature.len() != modulus.len() {
                return Ok(false);
            }
            rsa_verify(&modulus, &exponent, tbs, signature, scheme)
                .map_err(|error| x509_error(format!("issuer RSA key: {}", error.message)))
        }
        (CertSignature::Ecdsa(hash), Some(curve)) => {
            let point = info
                .subject_public_key
                .as_bytes()
                .ok_or_else(|| x509_error("issuer EC key is not a whole number of bytes"))?;
            let digest = hash.digest(tbs);
            let verified = if curve == P256_OID {
                use p256::ecdsa::signature::hazmat::PrehashVerifier;
                let key = EcVerifyingKey::from_sec1_bytes(point)
                    .map_err(|_| x509_error("issuer P-256 key is not a valid point"))?;
                EcSignature::from_der(signature)
                    .map(|parsed| key.verify_prehash(&digest, &parsed).is_ok())
                    .unwrap_or(false)
            } else {
                use p384::ecdsa::signature::hazmat::PrehashVerifier;
                let key = p384::ecdsa::VerifyingKey::from_sec1_bytes(point)
                    .map_err(|_| x509_error("issuer P-384 key is not a valid point"))?;
                p384::ecdsa::Signature::from_der(signature)
                    .map(|parsed| key.verify_prehash(&digest, &parsed).is_ok())
                    .unwrap_or(false)
            };
            Ok(verified)
        }
        // An RSA algorithm under an EC key, or the other way round, cannot have been signed by it.
        _ => Ok(false),
    }
}
