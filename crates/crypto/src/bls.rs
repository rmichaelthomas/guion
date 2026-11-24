//! BLS12-381 min-signature verification: signatures are compressed G1 points (48 bytes) and
//! public keys compressed G2 points (96 bytes), in the ZCash encoding that drand uses.
//!
//! The pieces are public so a later feature (timelock encryption) can reuse them:
//! [`decode_g1`] and [`decode_g2`] (validated decoding), [`hash_to_g1`] (RFC 9380 hash to
//! the curve) and [`pairing_product_is_one`]. Malformed input is a [`CryptoError`] with code
//! `E_CRYPTO_INPUT`; a well-formed signature that does not verify is `Ok(false)`. Nothing here
//! panics.

use ark_bls12_381::{g1, Bls12_381, Fq, G1Affine, G1Projective, G2Affine};
use ark_ec::hashing::curve_maps::wb::WBMap;
use ark_ec::hashing::map_to_curve_hasher::MapToCurveBasedHasher;
use ark_ec::hashing::HashToCurve;
use ark_ec::pairing::Pairing;
use ark_ec::AffineRepr;
use ark_ff::field_hashers::DefaultFieldHasher;
use ark_ff::{BigInteger, PrimeField, Zero};
use ark_serialize::CanonicalDeserialize;
use sha2::Sha256;

use crate::CryptoError;

pub use ark_bls12_381::{G1Affine as G1Point, G2Affine as G2Point};

/// Length of a compressed G1 point (a min-signature BLS signature).
pub const G1_COMPRESSED_LEN: usize = 48;
/// Length of a compressed G2 point (a min-signature BLS public key).
pub const G2_COMPRESSED_LEN: usize = 96;

fn input(message: &str) -> CryptoError {
    CryptoError::new("E_CRYPTO_INPUT", message)
}

/// Decodes a compressed G1 point. The length, flag bits, field range, curve equation and
/// subgroup membership are all checked.
pub fn decode_g1(bytes: &[u8]) -> Result<G1Affine, CryptoError> {
    if bytes.len() != G1_COMPRESSED_LEN {
        return Err(input("a G1 point is 48 bytes (compressed)"));
    }
    G1Affine::deserialize_compressed(bytes)
        .map_err(|_| input("not a valid compressed G1 point in the prime-order subgroup"))
}

/// Decodes a compressed G2 point, with the same checks as [`decode_g1`].
pub fn decode_g2(bytes: &[u8]) -> Result<G2Affine, CryptoError> {
    if bytes.len() != G2_COMPRESSED_LEN {
        return Err(input("a G2 point is 96 bytes (compressed)"));
    }
    G2Affine::deserialize_compressed(bytes)
        .map_err(|_| input("not a valid compressed G2 point in the prime-order subgroup"))
}

/// RFC 9380 hash to G1 (`hash_to_curve` with expand_message_xmd over SHA-256 and the
/// simplified SWU map with the 11-isogeny, so suite `BLS12381G1_XMD:SHA-256_SSWU_RO_`) using
/// the caller's domain separation tag.
pub fn hash_to_g1(message: &[u8], dst: &[u8]) -> Result<G1Affine, CryptoError> {
    if dst.is_empty() {
        return Err(input("the domain separation tag must not be empty"));
    }
    let hasher = MapToCurveBasedHasher::<
        G1Projective,
        DefaultFieldHasher<Sha256, 128>,
        WBMap<g1::Config>,
    >::new(dst)
    .map_err(|_| input("the domain separation tag is not usable"))?;
    hasher
        .hash(message)
        .map_err(|_| input("hash to curve failed"))
}

/// The affine coordinates of a G1 point as 48-byte big-endian integers; the point at infinity
/// is `(0, 0)`.
pub fn g1_coordinates(point: &G1Affine) -> ([u8; 48], [u8; 48]) {
    let encode = |value: &Fq| {
        let mut out = [0u8; 48];
        let bytes = value.into_bigint().to_bytes_be();
        out[48 - bytes.len()..].copy_from_slice(&bytes);
        out
    };
    match point.xy() {
        Some((x, y)) => (encode(&x), encode(&y)),
        None => ([0; 48], [0; 48]),
    }
}

/// True when the product of pairings `e(g1[0], g2[0]) * e(g1[1], g2[1]) * ...` is the identity.
/// The slices must have the same length.
pub fn pairing_product_is_one(g1_points: &[G1Affine], g2_points: &[G2Affine]) -> bool {
    if g1_points.len() != g2_points.len() {
        return false;
    }
    Bls12_381::multi_pairing(g1_points.iter().copied(), g2_points.iter().copied()).is_zero()
}

/// Verifies a min-signature BLS signature: `e(signature, g2_generator) == e(H(message), public_key)`
/// where `H` is [`hash_to_g1`] with `dst`. `public_key_g2` is 96 bytes and `signature_g1` 48 bytes,
/// both compressed; a point that is not on the curve, not in the subgroup, or the identity key is
/// `E_CRYPTO_INPUT`.
pub fn bls12381_verify_g1(
    public_key_g2: &[u8],
    message: &[u8],
    signature_g1: &[u8],
    dst: &[u8],
) -> Result<bool, CryptoError> {
    let public_key = decode_g2(public_key_g2)?;
    if public_key.is_zero() {
        return Err(input("the public key must not be the point at infinity"));
    }
    let signature = decode_g1(signature_g1)?;
    let hashed = hash_to_g1(message, dst)?;
    // e(signature, g2) * e(-H(m), pk) == 1
    let negated = -hashed;
    Ok(pairing_product_is_one(
        &[signature, negated],
        &[G2Affine::generator(), public_key],
    ))
}
