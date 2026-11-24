//! Ed25519 (RFC 8032, pure Ed25519: no Ed25519ph, no context).
//!
//! Signing is deterministic, so every function here is pure: the 32-byte seed arrives as an
//! argument and a Guion program draws it with `crypto_random_bytes(32)`. Verification is
//! `verify_strict`, which rejects small-order public keys and non-canonical `R` or `S` as
//! `Ok(false)`.

use crate::CryptoError;
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};

fn signing_key(seed: &[u8]) -> Result<SigningKey, CryptoError> {
    let seed: [u8; 32] = seed.try_into().map_err(|_| {
        CryptoError::new(
            "E_CRYPTO_INPUT",
            format!("an Ed25519 seed is 32 bytes, got {}", seed.len()),
        )
    })?;
    Ok(SigningKey::from_bytes(&seed))
}

/// The 32-byte public key for a 32-byte seed.
pub fn ed25519_public_key(seed: &[u8]) -> Result<[u8; 32], CryptoError> {
    Ok(signing_key(seed)?.verifying_key().to_bytes())
}

/// The 64-byte signature of `message` under the key derived from `seed`.
pub fn ed25519_sign(seed: &[u8], message: &[u8]) -> Result<[u8; 64], CryptoError> {
    Ok(signing_key(seed)?.sign(message).to_bytes())
}

/// Whether `signature` is a valid strict Ed25519 signature of `message` under `public_key`.
/// A wrong length, or a 32-byte key that is not a curve point, is `E_CRYPTO_INPUT`; a
/// well-formed signature that does not verify is `Ok(false)`.
pub fn ed25519_verify(
    public_key: &[u8],
    message: &[u8],
    signature: &[u8],
) -> Result<bool, CryptoError> {
    let key: [u8; 32] = public_key.try_into().map_err(|_| {
        CryptoError::new(
            "E_CRYPTO_INPUT",
            format!(
                "an Ed25519 public key is 32 bytes, got {}",
                public_key.len()
            ),
        )
    })?;
    let signature: [u8; 64] = signature.try_into().map_err(|_| {
        CryptoError::new(
            "E_CRYPTO_INPUT",
            format!("an Ed25519 signature is 64 bytes, got {}", signature.len()),
        )
    })?;
    let key = VerifyingKey::from_bytes(&key).map_err(|_| {
        CryptoError::new(
            "E_CRYPTO_INPUT",
            "the 32 bytes are not an Ed25519 public key (no such curve point)",
        )
    })?;
    Ok(key
        .verify_strict(message, &Signature::from_bytes(&signature))
        .is_ok())
}
