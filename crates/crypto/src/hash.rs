use sha2::{Digest, Sha512};
use sha3::Sha3_512;

/// SHA-512 (FIPS 180-4) of `data`.
pub fn sha512(data: &[u8]) -> [u8; 64] {
    Sha512::digest(data).into()
}

/// SHA3-512 (FIPS 202) of `data`.
pub fn sha3_512(data: &[u8]) -> [u8; 64] {
    Sha3_512::digest(data).into()
}
