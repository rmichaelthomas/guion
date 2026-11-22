//! drand timelock encryption (tlock) for G1-signature networks such as quicknet
//! (`bls-unchained-g1-rfc9380`), in the age v1 format. The construction, with a source reference
//! for every hash, tag, label and byte order, is in `tlock.md`; the age container is in `age.rs`.
//!
//! Randomness arrives as an argument: [`tlock_encrypt`] derives the file key, the IBE `sigma` and
//! the STREAM nonce from 48 caller supplied bytes, so it is deterministic and needs no entropy
//! source. Nothing here panics; failures are [`CryptoError`]s with `E_TLOCK_FORMAT` (malformed
//! armor, header or stanza), `E_TLOCK_DECRYPT` (wrong round signature or tampering) or
//! `E_CRYPTO_INPUT` (a key or signature of the wrong length or not a valid point).

use ark_bls12_381::{Bls12_381, Fq12, Fr, G2Affine, G2Projective};
use ark_ec::pairing::{Pairing, PairingOutput};
use ark_ec::{AffineRepr, CurveGroup};
use ark_ff::{BigInt, BigInteger, PrimeField};
use ark_serialize::CanonicalSerialize;
use hkdf::Hkdf;
use sha2::{Digest, Sha256};

use crate::age::{self, Stanza, FILE_KEY_LEN, STREAM_NONCE_LEN};
use crate::bls::{decode_g1, decode_g2, hash_to_g1, G1_COMPRESSED_LEN, G2_COMPRESSED_LEN};
use crate::CryptoError;

/// The domain separation tag of the quicknet G1 hash to curve (`tlock.md` section 2).
const G1_DST: &[u8] = b"BLS_SIG_BLS12381G1_XMD:SHA-256_SSWU_RO_NUL_";
const CHAIN_HASH_LEN: usize = 32;
/// `V` and `W` are both the length of the 16-byte file key.
const SIGMA_LEN: usize = FILE_KEY_LEN;
const BODY_LEN: usize = G2_COMPRESSED_LEN + SIGMA_LEN + SIGMA_LEN;

fn input(message: &str) -> CryptoError {
    CryptoError::new("E_CRYPTO_INPUT", message)
}

fn sha256(parts: &[&[u8]]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part);
    }
    hasher.finalize().into()
}

/// The identity of a round: SHA-256 of the round as 8 big-endian bytes.
fn round_identity(round: u64) -> [u8; 32] {
    sha256(&[&round.to_be_bytes()])
}

fn xor(a: &[u8], b: &[u8]) -> Vec<u8> {
    a.iter().zip(b).map(|(x, y)| x ^ y).collect()
}

/// `H3(sigma, msg)` of the IBE: rejection sampled scalar (`tlock.md` section 4).
fn h3(sigma: &[u8], msg: &[u8]) -> Result<Fr, CryptoError> {
    let buffer = sha256(&[b"IBE-H3", sigma, msg]);
    for i in 1u16..u16::MAX {
        let mut hashed = sha256(&[&i.to_le_bytes(), &buffer]);
        // The scalar field is 255 bits in a 32 byte big-endian encoding: clear the top bit.
        hashed[0] >>= 1;
        let mut limbs = [0u64; 4];
        for (k, limb) in limbs.iter_mut().enumerate() {
            let end = 32 - 8 * k;
            let mut word = [0u8; 8];
            word.copy_from_slice(&hashed[end - 8..end]);
            *limb = u64::from_be_bytes(word);
        }
        if let Some(r) = Fr::from_bigint(BigInt::<4>(limbs)) {
            return Ok(r);
        }
    }
    Err(input("rejection sampling failure"))
}

fn h4(sigma: &[u8]) -> Vec<u8> {
    sha256(&[b"IBE-H4", sigma])[..SIGMA_LEN].to_vec()
}

/// `H2` of the IBE: SHA-256 of the tag and the 576 byte GT serialization (`tlock.md` section 4).
fn h2(gt: &PairingOutput<Bls12_381>) -> Vec<u8> {
    let Fq12 { c0, c1 } = gt.0;
    let mut bytes = Vec::with_capacity(576);
    for six in [c1, c0] {
        for two in [six.c2, six.c1, six.c0] {
            for fp in [two.c1, two.c0] {
                let be = fp.into_bigint().to_bytes_be();
                bytes.extend(std::iter::repeat(0u8).take(48 - be.len()));
                bytes.extend_from_slice(&be);
            }
        }
    }
    sha256(&[b"IBE-H2", &bytes])[..SIGMA_LEN].to_vec()
}

fn derive<const N: usize>(randomness: &[u8; 48], label: &[u8]) -> Result<[u8; N], CryptoError> {
    let mut out = [0u8; N];
    Hkdf::<Sha256>::new(None, randomness)
        .expand(label, &mut out)
        .map_err(|_| input("cannot derive randomness"))?;
    Ok(out)
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(DIGITS[(b >> 4) as usize] as char);
        out.push(DIGITS[(b & 15) as usize] as char);
    }
    out
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    let bytes = text.as_bytes();
    if bytes.len() % 2 != 0 {
        return None;
    }
    let nibble = |c: u8| match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    };
    bytes
        .chunks(2)
        .map(|p| Some(nibble(p[0])? << 4 | nibble(p[1])?))
        .collect()
}

/// Encrypts `plaintext` so that it can be decrypted once drand publishes the signature for
/// `round`. `public_key_g2` is the network's 96-byte compressed G2 key, `chain_hash` its 32-byte
/// chain hash (recorded in the stanza). Returns armored age.
pub fn tlock_encrypt(
    round: u64,
    chain_hash: &[u8],
    public_key_g2: &[u8],
    plaintext: &[u8],
    randomness: &[u8; 48],
) -> Result<String, CryptoError> {
    if chain_hash.len() != CHAIN_HASH_LEN {
        return Err(input("a chain hash is 32 bytes"));
    }
    let public_key = decode_g2(public_key_g2)?;
    if public_key.is_zero() {
        return Err(input("the public key must not be the point at infinity"));
    }
    let file_key: [u8; FILE_KEY_LEN] = derive(randomness, b"guion tlock v1 file key")?;
    let sigma: [u8; SIGMA_LEN] = derive(randomness, b"guion tlock v1 ibe sigma")?;
    let stream_nonce: [u8; STREAM_NONCE_LEN] = derive(randomness, b"guion tlock v1 stream nonce")?;

    // IBE (EncryptCCAonG2), msg = the file key.
    let q_id = hash_to_g1(&round_identity(round), G1_DST)?;
    let g_id = Bls12_381::pairing(q_id, public_key);
    let r = h3(&sigma, &file_key)?;
    let u = (G2Projective::from(G2Affine::generator()) * r).into_affine();
    let v = xor(&sigma, &h2(&(g_id * r)));
    let w = xor(&file_key, &h4(&sigma));

    let mut body = Vec::with_capacity(BODY_LEN);
    u.serialize_compressed(&mut body)
        .map_err(|_| input("cannot encode the ciphertext point"))?;
    body.extend_from_slice(&v);
    body.extend_from_slice(&w);

    let stanza = Stanza {
        kind: "tlock".to_string(),
        args: vec![round.to_string(), hex(chain_hash)],
        body,
    };
    let binary = age::seal(&file_key, &stanza, &stream_nonce, plaintext)?;
    Ok(age::armor(&binary))
}

/// The first `tlock` stanza (two arguments: round, chain hash) of a parsed header.
fn tlock_stanza(header: &age::Header) -> Result<(u64, Vec<u8>, &Stanza), CryptoError> {
    for stanza in &header.stanzas {
        if stanza.kind != "tlock" || stanza.args.len() != 2 {
            continue;
        }
        let round = stanza.args[0]
            .parse::<u64>()
            .map_err(|_| age::format_error("the tlock stanza round is not a number"))?;
        let chain_hash = unhex(&stanza.args[1])
            .filter(|h| h.len() == CHAIN_HASH_LEN)
            .ok_or_else(|| age::format_error("the tlock stanza chain hash is not 32 hex bytes"))?;
        return Ok((round, chain_hash, stanza));
    }
    Err(age::format_error("no tlock stanza"))
}

/// The round and chain hash named by the armored file's `tlock` stanza. The header is parsed but
/// cannot be authenticated without the round signature.
pub fn tlock_round(armored: &str) -> Result<(u64, Vec<u8>), CryptoError> {
    let binary = age::dearmor(armored)?;
    let header = age::parse_header(&binary)?;
    let (round, chain_hash, _) = tlock_stanza(&header)?;
    Ok((round, chain_hash))
}

/// Decrypts armored tlock age with the round's 48-byte compressed G1 signature. A wrong
/// signature, a modified header or a modified payload is `E_TLOCK_DECRYPT`.
pub fn tlock_decrypt(armored: &str, round_signature_g1: &[u8]) -> Result<Vec<u8>, CryptoError> {
    let binary = age::dearmor(armored)?;
    let header = age::parse_header(&binary)?;
    let (_, _, stanza) = tlock_stanza(&header)?;
    if stanza.body.len() != BODY_LEN {
        return Err(age::format_error("the tlock stanza body is not 128 bytes"));
    }
    let (u_bytes, rest) = stanza.body.split_at(G2_COMPRESSED_LEN);
    let (v, w) = rest.split_at(SIGMA_LEN);
    let u = decode_g2(u_bytes)
        .map_err(|_| age::format_error("the tlock stanza point is not a valid G2 point"))?;
    if round_signature_g1.len() != G1_COMPRESSED_LEN {
        return Err(input("a round signature is 48 bytes (compressed G1)"));
    }
    let signature = decode_g1(round_signature_g1)?;

    // IBE (DecryptCCAonG2).
    let g = Bls12_381::pairing(signature, u);
    let sigma = xor(&h2(&g), v);
    let file_key = xor(&h4(&sigma), w);
    let r = h3(&sigma, &file_key)?;
    let check = (G2Projective::from(G2Affine::generator()) * r).into_affine();
    if check != u {
        return Err(age::decrypt_error(
            "the round signature does not decrypt this file",
        ));
    }

    age::verify_header_mac(&file_key, &binary, &header)?;
    let payload = binary
        .get(header.payload_offset..)
        .ok_or_else(|| age::format_error("truncated age file"))?;
    age::stream_open(&file_key, payload)
}
