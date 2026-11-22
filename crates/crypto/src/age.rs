//! The age v1 container (header, header MAC, STREAM payload, armor) for exactly the shapes `tle`
//! reads and writes. See `tlock.md` section 6 for the source references. Nothing here panics: every
//! failure is a [`CryptoError`] with `E_TLOCK_FORMAT` (the file is not a well formed age file) or
//! `E_TLOCK_DECRYPT` (it is well formed but does not authenticate).

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Nonce};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::CryptoError;

const INTRO: &str = "age-encryption.org/v1\n";
const ARMOR_BEGIN: &str = "-----BEGIN AGE ENCRYPTED FILE-----";
const ARMOR_END: &str = "-----END AGE ENCRYPTED FILE-----";
const COLUMNS: usize = 64;
const BYTES_PER_LINE: usize = COLUMNS / 4 * 3;
const CHUNK: usize = 64 * 1024;
const TAG: usize = 16;
const MAX_WHITESPACE: usize = 1024;

pub(crate) const FILE_KEY_LEN: usize = 16;
pub(crate) const STREAM_NONCE_LEN: usize = 16;

pub(crate) fn format_error(message: &str) -> CryptoError {
    CryptoError::new("E_TLOCK_FORMAT", message)
}

pub(crate) fn decrypt_error(message: &str) -> CryptoError {
    CryptoError::new("E_TLOCK_DECRYPT", message)
}

/// A recipient stanza: `-> type arg...` and a binary body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Stanza {
    pub kind: String,
    pub args: Vec<String>,
    pub body: Vec<u8>,
}

// ---- base64 (strict, canonical) ----------------------------------------------------------

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub(crate) fn base64_encode(bytes: &[u8], padded: bool) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        let emit = chunk.len() + 1;
        for i in 0..4 {
            if i < emit {
                out.push(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else if padded {
                out.push('=');
            }
        }
    }
    out
}

fn sextet(c: u8) -> Option<u32> {
    match c {
        b'A'..=b'Z' => Some(u32::from(c - b'A')),
        b'a'..=b'z' => Some(u32::from(c - b'a') + 26),
        b'0'..=b'9' => Some(u32::from(c - b'0') + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

/// Strict base64: only alphabet characters, correct padding (required when `padded`, forbidden
/// otherwise) and zero trailing bits, so every byte string has exactly one encoding.
pub(crate) fn base64_decode(text: &[u8], padded: bool) -> Option<Vec<u8>> {
    let mut body = text;
    if padded {
        if text.len() % 4 != 0 {
            return None;
        }
        let pad = text.iter().rev().take_while(|&&c| c == b'=').count();
        if pad > 2 {
            return None;
        }
        body = &text[..text.len() - pad];
    }
    if body.len() % 4 == 1 {
        return None;
    }
    let mut out = Vec::with_capacity(body.len() / 4 * 3 + 2);
    for group in body.chunks(4) {
        let mut n = 0u32;
        for &c in group {
            n = (n << 6) | sextet(c)?;
        }
        let used = group.len();
        n <<= 6 * (4 - used) as u32;
        let produced = used * 6 / 8;
        let bytes = [(n >> 16) as u8, (n >> 8) as u8, n as u8];
        // Leftover bits must be zero.
        if bytes[produced..].iter().any(|&b| b != 0) {
            return None;
        }
        out.extend_from_slice(&bytes[..produced]);
    }
    Some(out)
}

/// Wraps base64 at 64 columns: every full line ends in `\n`; the last line is short and may be
/// empty, and gets no newline here (callers add it).
fn wrap64(encoded: &str) -> String {
    let mut out = String::with_capacity(encoded.len() + encoded.len() / COLUMNS + 2);
    let bytes = encoded.as_bytes();
    for line in bytes.chunks(COLUMNS) {
        out.push_str(std::str::from_utf8(line).unwrap_or(""));
        if line.len() == COLUMNS {
            out.push('\n');
        }
    }
    out
}

// ---- armor -------------------------------------------------------------------------------

pub(crate) fn armor(binary: &[u8]) -> String {
    let mut out = String::new();
    out.push_str(ARMOR_BEGIN);
    out.push('\n');
    let wrapped = wrap64(&base64_encode(binary, true));
    out.push_str(&wrapped);
    if !wrapped.is_empty() && !wrapped.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(ARMOR_END);
    out.push('\n');
    out
}

pub(crate) fn dearmor(text: &str) -> Result<Vec<u8>, CryptoError> {
    let bad = |m: &str| format_error(m);
    let mut rest = text.as_bytes();
    let next_line = |rest: &mut &[u8]| -> Option<Vec<u8>> {
        if rest.is_empty() {
            return None;
        }
        let end = rest.iter().position(|&b| b == b'\n');
        let (line, tail) = match end {
            Some(i) => (&rest[..i], &rest[i + 1..]),
            None => (&rest[..], &rest[rest.len()..]),
        };
        *rest = tail;
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        Some(line.to_vec())
    };
    let blank = |line: &[u8]| line.iter().all(|c| c.is_ascii_whitespace());
    let mut removed = 0usize;
    loop {
        let line = next_line(&mut rest).ok_or_else(|| bad("missing armor header"))?;
        if blank(&line) {
            removed += line.len() + 1;
            if removed > MAX_WHITESPACE {
                return Err(bad("too much leading whitespace"));
            }
            continue;
        }
        if line != ARMOR_BEGIN.as_bytes() {
            return Err(bad("invalid armor header"));
        }
        break;
    }
    let mut data = Vec::new();
    loop {
        let line = next_line(&mut rest).ok_or_else(|| bad("armor ends without a footer"))?;
        if line == ARMOR_END.as_bytes() {
            break;
        }
        if line.len() > COLUMNS {
            return Err(bad("armor column limit exceeded"));
        }
        let decoded = base64_decode(&line, true).ok_or_else(|| bad("invalid armor base64"))?;
        let short = decoded.len() < BYTES_PER_LINE;
        data.extend_from_slice(&decoded);
        if short {
            let footer = next_line(&mut rest).ok_or_else(|| bad("armor ends without a footer"))?;
            if footer != ARMOR_END.as_bytes() {
                return Err(bad("invalid armor closing line"));
            }
            break;
        }
    }
    if rest.len() >= MAX_WHITESPACE {
        return Err(bad("too much trailing whitespace"));
    }
    if !blank(rest) {
        return Err(bad("trailing data after armored file"));
    }
    Ok(data)
}

// ---- header ------------------------------------------------------------------------------

fn header_mac_key(file_key: &[u8]) -> [u8; 32] {
    let mut key = [0u8; 32];
    // 32 bytes is far below the HKDF-SHA256 limit, so expand cannot fail.
    let _ = Hkdf::<Sha256>::new(None, file_key).expand(b"header", &mut key);
    key
}

fn header_mac(file_key: &[u8], header_without_mac: &[u8]) -> Result<Hmac<Sha256>, CryptoError> {
    let key = header_mac_key(file_key);
    // HMAC accepts any key length, so this cannot fail in practice.
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(&key)
        .map_err(|_| decrypt_error("cannot compute the age header MAC"))?;
    mac.update(header_without_mac);
    Ok(mac)
}

fn marshal_stanza(stanza: &Stanza, out: &mut String) {
    out.push_str("->");
    out.push(' ');
    out.push_str(&stanza.kind);
    for arg in &stanza.args {
        out.push(' ');
        out.push_str(arg);
    }
    out.push('\n');
    out.push_str(&wrap64(&base64_encode(&stanza.body, false)));
    // The body ends with a short (possibly empty) line, then a newline.
    out.push('\n');
}

/// Builds the complete binary age file for one stanza: header, nonce and encrypted payload.
pub(crate) fn seal(
    file_key: &[u8; FILE_KEY_LEN],
    stanza: &Stanza,
    nonce: &[u8; STREAM_NONCE_LEN],
    plaintext: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    let mut header = String::from(INTRO);
    marshal_stanza(stanza, &mut header);
    header.push_str("---");
    let mac = header_mac(file_key, header.as_bytes())?
        .finalize()
        .into_bytes();
    let mut out = header.into_bytes();
    out.push(b' ');
    out.extend_from_slice(base64_encode(&mac, false).as_bytes());
    out.push(b'\n');
    out.extend_from_slice(nonce);
    out.extend_from_slice(&stream_seal(file_key, nonce, plaintext)?);
    Ok(out)
}

/// The parsed header of a binary age file.
pub(crate) struct Header {
    pub stanzas: Vec<Stanza>,
    pub mac: Vec<u8>,
    /// Bytes covered by the MAC: the intro through the `---` of the footer line.
    pub mac_input_len: usize,
    /// Offset of the first payload byte (the stream nonce).
    pub payload_offset: usize,
}

fn read_line(data: &[u8], pos: &mut usize) -> Option<(usize, usize)> {
    let start = *pos;
    let rel = data.get(start..)?.iter().position(|&b| b == b'\n')?;
    *pos = start + rel + 1;
    Some((start, start + rel))
}

fn valid_arg(arg: &[u8]) -> bool {
    !arg.is_empty() && arg.iter().all(|&c| (33..=126).contains(&c))
}

pub(crate) fn parse_header(data: &[u8]) -> Result<Header, CryptoError> {
    if !data.starts_with(INTRO.as_bytes()) {
        return Err(format_error("not an age v1 file"));
    }
    let mut pos = INTRO.len();
    let mut stanzas = Vec::new();
    loop {
        let rest = data.get(pos..).unwrap_or(&[]);
        if rest.len() < 3 {
            return Err(format_error("truncated age header"));
        }
        if rest.starts_with(b"---") {
            let footer_start = pos;
            let (s, e) =
                read_line(data, &mut pos).ok_or_else(|| format_error("truncated age header"))?;
            let line = &data[s..e];
            let mut parts = line.split(|&b| b == b' ');
            let prefix = parts.next().unwrap_or(&[]);
            let args: Vec<&[u8]> = parts.collect();
            if prefix != b"---" || args.len() != 1 {
                return Err(format_error("malformed age header closing line"));
            }
            let mac = base64_decode(args[0], false)
                .filter(|m| m.len() == 32)
                .ok_or_else(|| format_error("malformed age header MAC"))?;
            return Ok(Header {
                stanzas,
                mac,
                mac_input_len: footer_start + 3,
                payload_offset: pos,
            });
        }
        // A stanza.
        let (s, e) =
            read_line(data, &mut pos).ok_or_else(|| format_error("truncated age header"))?;
        let line = &data[s..e];
        if !line.starts_with(b"->") {
            return Err(format_error("malformed age stanza opening line"));
        }
        let mut parts = line.split(|&b| b == b' ');
        if parts.next() != Some(b"->".as_slice()) {
            return Err(format_error("malformed age stanza opening line"));
        }
        let args: Vec<&[u8]> = parts.collect();
        if args.is_empty() || !args.iter().all(|a| valid_arg(a)) {
            return Err(format_error("malformed age stanza arguments"));
        }
        let text = |a: &[u8]| String::from_utf8_lossy(a).into_owned();
        let mut stanza = Stanza {
            kind: text(args[0]),
            args: args[1..].iter().map(|a| text(a)).collect(),
            body: Vec::new(),
        };
        loop {
            let (s, e) = read_line(data, &mut pos)
                .ok_or_else(|| format_error("truncated age stanza body"))?;
            let decoded = base64_decode(&data[s..e], false)
                .ok_or_else(|| format_error("malformed age stanza body line"))?;
            if decoded.len() > BYTES_PER_LINE {
                return Err(format_error("age stanza body line too long"));
            }
            stanza.body.extend_from_slice(&decoded);
            if decoded.len() < BYTES_PER_LINE {
                break;
            }
        }
        stanzas.push(stanza);
    }
}

/// Checks the header MAC (constant time) against the recovered file key.
pub(crate) fn verify_header_mac(
    file_key: &[u8],
    data: &[u8],
    header: &Header,
) -> Result<(), CryptoError> {
    let covered = data
        .get(..header.mac_input_len)
        .ok_or_else(|| format_error("truncated age header"))?;
    header_mac(file_key, covered)?
        .verify_slice(&header.mac)
        .map_err(|_| decrypt_error("the age header MAC does not verify"))
}

// ---- payload (STREAM) --------------------------------------------------------------------

fn stream_key(file_key: &[u8], nonce: &[u8]) -> [u8; 32] {
    let mut key = [0u8; 32];
    let _ = Hkdf::<Sha256>::new(Some(nonce), file_key).expand(b"payload", &mut key);
    key
}

fn counter_nonce(counter: u128, last: bool) -> [u8; 12] {
    let mut nonce = [0u8; 12];
    nonce[..11].copy_from_slice(&counter.to_be_bytes()[5..]);
    nonce[11] = u8::from(last);
    nonce
}

fn stream_seal(file_key: &[u8], nonce: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, CryptoError> {
    let key = stream_key(file_key, nonce);
    let cipher = ChaCha20Poly1305::new((&key).into());
    let chunks: Vec<&[u8]> = if plaintext.is_empty() {
        vec![&[][..]]
    } else {
        plaintext.chunks(CHUNK).collect()
    };
    let mut out = Vec::with_capacity(plaintext.len() + chunks.len() * TAG);
    let total = chunks.len();
    for (index, chunk) in chunks.iter().enumerate() {
        let n = counter_nonce(index as u128, index + 1 == total);
        let ct = cipher
            .encrypt(
                Nonce::from_slice(&n),
                Payload {
                    msg: chunk,
                    aad: &[],
                },
            )
            .map_err(|_| decrypt_error("cannot encrypt the payload"))?;
        out.extend_from_slice(&ct);
    }
    Ok(out)
}

/// Decrypts and authenticates the payload (`data` starts at the stream nonce).
pub(crate) fn stream_open(file_key: &[u8], data: &[u8]) -> Result<Vec<u8>, CryptoError> {
    if data.len() < STREAM_NONCE_LEN {
        return Err(format_error("the age payload is missing its nonce"));
    }
    let (nonce, body) = data.split_at(STREAM_NONCE_LEN);
    let key = stream_key(file_key, nonce);
    let cipher = ChaCha20Poly1305::new((&key).into());
    let open = |counter: u128, last: bool, chunk: &[u8]| {
        let n = counter_nonce(counter, last);
        cipher
            .decrypt(
                Nonce::from_slice(&n),
                Payload {
                    msg: chunk,
                    aad: &[],
                },
            )
            .ok()
    };
    let auth = || decrypt_error("the payload does not authenticate");
    let mut out = Vec::with_capacity(body.len());
    let mut pos = 0usize;
    let mut counter = 0u128;
    loop {
        let rest = &body[pos..];
        if rest.is_empty() {
            return Err(decrypt_error("the payload is truncated"));
        }
        if rest.len() >= CHUNK + TAG {
            let chunk = &rest[..CHUNK + TAG];
            if let Some(plain) = open(counter, false, chunk) {
                out.extend_from_slice(&plain);
                pos += CHUNK + TAG;
                counter += 1;
                continue;
            }
            // A full-size final chunk.
            let plain = open(counter, true, chunk).ok_or_else(auth)?;
            if rest.len() != CHUNK + TAG {
                return Err(decrypt_error("trailing data after the end of the payload"));
            }
            out.extend_from_slice(&plain);
            return Ok(out);
        }
        if counter != 0 && rest.len() == TAG {
            return Err(decrypt_error("the last payload chunk is empty"));
        }
        let plain = open(counter, true, rest).ok_or_else(auth)?;
        out.extend_from_slice(&plain);
        return Ok(out);
    }
}
