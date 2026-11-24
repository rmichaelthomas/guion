//! DAG-CBOR encoding of a JSON value, as IPLD's DAG-JSON to DAG-CBOR mapping defines it.
//!
//! Deterministic CBOR: shortest integer and length forms, definite lengths, map keys sorted by
//! encoded length and then bytewise, 64-bit floats only. Beyond plain JSON, DAG-JSON's two reserved
//! shapes are understood: `{"/": "<cid>"}` becomes tag 42 around `0x00 || <binary CID>`, and
//! `{"/": {"bytes": "<base64>"}}` becomes a byte string.
//!
//! Numbers: serde_json keeps a number's source text (the workspace enables `arbitrary_precision`),
//! so a number spelled as an integer (`-?digits`) is a CBOR integer when it fits major type 0 or 1
//! (-2^64 to 2^64 - 1) and anything else (a point or an exponent) is a 64-bit float. NaN and the
//! infinities (an overflowing literal like `1e999` parses to one) are refused.

use crate::CryptoError;
use serde_json::Value;

/// Nesting deeper than this is refused rather than risking the stack.
const MAX_DEPTH: usize = 128;

fn value_error(message: impl Into<String>) -> CryptoError {
    CryptoError::new("E_DAGCBOR_VALUE", message)
}

fn link_error(message: impl Into<String>) -> CryptoError {
    CryptoError::new("E_DAGCBOR_LINK", message)
}

/// Encodes `value` as canonical DAG-CBOR.
pub fn dagcbor_encode(value: &Value) -> Result<Vec<u8>, CryptoError> {
    let mut out = Vec::new();
    encode(value, &mut out, 0)?;
    Ok(out)
}

/// Writes a major type with its shortest-form argument.
fn head(out: &mut Vec<u8>, major: u8, argument: u64) {
    let major = major << 5;
    if argument < 24 {
        out.push(major | argument as u8);
    } else if argument <= 0xff {
        out.push(major | 24);
        out.push(argument as u8);
    } else if argument <= 0xffff {
        out.push(major | 25);
        out.extend_from_slice(&(argument as u16).to_be_bytes());
    } else if argument <= 0xffff_ffff {
        out.push(major | 26);
        out.extend_from_slice(&(argument as u32).to_be_bytes());
    } else {
        out.push(major | 27);
        out.extend_from_slice(&argument.to_be_bytes());
    }
}

fn encode(value: &Value, out: &mut Vec<u8>, depth: usize) -> Result<(), CryptoError> {
    if depth > MAX_DEPTH {
        return Err(value_error(format!(
            "value is nested deeper than {MAX_DEPTH} levels"
        )));
    }
    match value {
        Value::Null => out.push(0xf6),
        Value::Bool(false) => out.push(0xf4),
        Value::Bool(true) => out.push(0xf5),
        Value::Number(number) => encode_number(&number.to_string(), out)?,
        Value::String(text) => {
            head(out, 3, text.len() as u64);
            out.extend_from_slice(text.as_bytes());
        }
        Value::Array(items) => {
            head(out, 4, items.len() as u64);
            for item in items {
                encode(item, out, depth + 1)?;
            }
        }
        Value::Object(map) => {
            if let Some(link) = map.get("/") {
                return encode_link(map.len(), link, out);
            }
            let mut entries: Vec<(Vec<u8>, &Value)> = Vec::with_capacity(map.len());
            for (key, item) in map {
                let mut encoded = Vec::with_capacity(key.len() + 9);
                head(&mut encoded, 3, key.len() as u64);
                encoded.extend_from_slice(key.as_bytes());
                entries.push((encoded, item));
            }
            entries.sort_by(|a, b| a.0.len().cmp(&b.0.len()).then_with(|| a.0.cmp(&b.0)));
            head(out, 5, entries.len() as u64);
            for (key, item) in entries {
                out.extend_from_slice(&key);
                encode(item, out, depth + 1)?;
            }
        }
    }
    Ok(())
}

fn encode_number(text: &str, out: &mut Vec<u8>) -> Result<(), CryptoError> {
    let digits = text.strip_prefix('-').unwrap_or(text);
    if !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit()) {
        let n: i128 = text
            .parse()
            .map_err(|_| value_error("integer is outside the CBOR range (-2^64 to 2^64 - 1)"))?;
        if n >= 0 {
            let argument = u64::try_from(n).map_err(|_| {
                value_error("integer is outside the CBOR range (-2^64 to 2^64 - 1)")
            })?;
            head(out, 0, argument);
        } else {
            let argument = u64::try_from(-1 - n).map_err(|_| {
                value_error("integer is outside the CBOR range (-2^64 to 2^64 - 1)")
            })?;
            head(out, 1, argument);
        }
        return Ok(());
    }
    let float: f64 = text
        .parse()
        .map_err(|_| value_error("number is not a valid float"))?;
    if !float.is_finite() {
        return Err(value_error("NaN and infinities cannot be encoded"));
    }
    out.push(0xfb);
    out.extend_from_slice(&float.to_bits().to_be_bytes());
    Ok(())
}

/// A map that has a `"/"` key must be exactly one of the two DAG-JSON link shapes.
fn encode_link(map_len: usize, link: &Value, out: &mut Vec<u8>) -> Result<(), CryptoError> {
    if map_len != 1 {
        return Err(link_error(
            "an object with a \"/\" key must have no other keys",
        ));
    }
    match link {
        Value::String(text) => {
            let cid = decode_cid(text)?;
            out.push(0xd8);
            out.push(42);
            head(out, 2, cid.len() as u64 + 1);
            out.push(0x00);
            out.extend_from_slice(&cid);
            Ok(())
        }
        Value::Object(inner) => match (inner.len(), inner.get("bytes")) {
            (1, Some(Value::String(text))) => {
                let bytes = base64_decode(text)?;
                head(out, 2, bytes.len() as u64);
                out.extend_from_slice(&bytes);
                Ok(())
            }
            _ => Err(link_error(
                "\"/\" holds an object that is not {\"bytes\": \"<base64>\"}",
            )),
        },
        _ => Err(link_error(
            "\"/\" must hold a CID string or {\"bytes\": ...}",
        )),
    }
}

/// Reads an unsigned LEB128 varint (at most 9 bytes), returning it and the bytes consumed.
fn varint(bytes: &[u8]) -> Option<(u64, usize)> {
    let mut value = 0u64;
    for (index, byte) in bytes.iter().take(9).enumerate() {
        value |= u64::from(byte & 0x7f) << (7 * index);
        if byte & 0x80 == 0 {
            return Some((value, index + 1));
        }
    }
    None
}

/// Multibase `b` (base32 lower, no padding) CIDv1 text to its binary form, checked to be
/// `<version 1><codec><multihash code><digest length><digest>`.
fn decode_cid(text: &str) -> Result<Vec<u8>, CryptoError> {
    let body = text
        .strip_prefix('b')
        .ok_or_else(|| link_error("a CID link must be multibase base32 lower (prefix \"b\")"))?;
    let bytes = base32_lower_decode(body)
        .ok_or_else(|| link_error("a CID link is not valid unpadded lower-case base32"))?;
    let shape = || link_error("a CID link must be a well-formed CIDv1");
    let mut at = 0;
    for field in 0..4 {
        let (value, used) = varint(&bytes[at..]).ok_or_else(shape)?;
        at += used;
        match field {
            0 if value != 1 => return Err(shape()),
            3 if (bytes.len() - at) as u64 != value || value == 0 => return Err(shape()),
            _ => {}
        }
    }
    Ok(bytes)
}

fn base32_lower_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() * 5 / 8);
    let (mut buffer, mut bits) = (0u32, 0u32);
    for byte in text.bytes() {
        let value = match byte {
            b'a'..=b'z' => byte - b'a',
            b'2'..=b'7' => byte - b'2' + 26,
            _ => return None,
        };
        buffer = (buffer << 5) | u32::from(value);
        bits += 5;
        if bits >= 8 {
            out.push((buffer >> (bits - 8)) as u8);
            bits -= 8;
            buffer &= (1 << bits) - 1;
        }
    }
    // Leftover bits must be zero padding, and fewer than a whole character's worth.
    if bits >= 5 || buffer != 0 {
        return None;
    }
    Some(out)
}

/// Standard base64 (alphabet `+/`), padding optional, no whitespace.
fn base64_decode(text: &str) -> Result<Vec<u8>, CryptoError> {
    let bad = || link_error("\"bytes\" is not valid standard base64");
    let trimmed = text
        .strip_suffix("==")
        .or_else(|| text.strip_suffix('='))
        .unwrap_or(text);
    let mut out = Vec::with_capacity(trimmed.len() * 3 / 4);
    let (mut buffer, mut bits) = (0u32, 0u32);
    for byte in trimmed.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return Err(bad()),
        };
        buffer = (buffer << 6) | u32::from(value);
        bits += 6;
        if bits >= 8 {
            out.push((buffer >> (bits - 8)) as u8);
            bits -= 8;
            buffer &= (1 << bits) - 1;
        }
    }
    if bits >= 6 || buffer != 0 || trimmed.len() % 4 == 1 {
        return Err(bad());
    }
    if text.len() != trimmed.len() && !text.len().is_multiple_of(4) {
        return Err(bad());
    }
    Ok(out)
}
