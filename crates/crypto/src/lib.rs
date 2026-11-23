//! Pure cryptography for Guion's `Crypto` instruction.
//!
//! This crate does no I/O, reads no clock and never draws randomness: anything that needs
//! entropy receives it as an argument. It has no C dependency, so it builds for
//! `wasm32-unknown-unknown` and the browser VM gets every function through `guion-vm-core`.
//! No function here panics on any input; a function that can fail returns [`CryptoError`],
//! which the VM turns into a Guion `StdError` carrying the same `code`.
//!
//! To add a function: write it here, add a `CryptoFunction` variant in `guion-model`, a dispatch
//! arm in `guion_vm_core::apply_crypto` and a wrapper in `language/stdlib/crypto.gu`.

mod age;
pub mod bls;
mod dagcbor;
mod ed25519;
mod hash;
mod signature;
mod tlock;

pub use bls::bls12381_verify_g1;
pub use dagcbor::dagcbor_encode;
pub use ed25519::{ed25519_public_key, ed25519_sign, ed25519_verify};
pub use hash::{sha3_512, sha512};
pub use signature::{
    ecdsa_p256_verify, rsa_verify, x509_dns_names, x509_rsa_public_key, x509_validity,
    x509_verify_issued_by, RsaScheme,
};
pub use tlock::{tlock_chain_hash, tlock_decrypt, tlock_encrypt, tlock_round};

/// A failed cryptographic operation: a stable machine-readable `code` and a human `message`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CryptoError {
    pub code: &'static str,
    pub message: String,
}

impl CryptoError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        CryptoError {
            code,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for CryptoError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for CryptoError {}
