//! gzip (RFC 1952) and zlib (RFC 1950) over bytes in memory, shared by the native runtime and the
//! browser VM. Every inflate counts the bytes it produces and stops at a ceiling.

use flate2::bufread::{GzDecoder, ZlibDecoder};
use flate2::write::{GzEncoder, ZlibEncoder};
use flate2::Compression;
use guion_model::BigInt;
use std::fmt;
use std::io::{self, BufRead, Read, Write};

pub const CHUNK: usize = 64 * 1024;

/// A typed failure of a compression or tar operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompressFailure {
    pub code: &'static str,
    pub message: String,
}

impl fmt::Display for CompressFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for CompressFailure {}

pub fn failure(code: &'static str, message: impl Into<String>) -> CompressFailure {
    CompressFailure {
        code,
        message: message.into(),
    }
}

pub fn limit_failure(limit: u64) -> CompressFailure {
    failure(
        "E_COMPRESS_LIMIT",
        format!("decompressed output would exceed the ceiling of {limit} bytes"),
    )
}

/// Finds a [`CompressFailure`] in an I/O error or anything it wraps.
pub fn failure_in(error: &io::Error) -> Option<CompressFailure> {
    let mut current: Option<&(dyn std::error::Error + 'static)> =
        error.get_ref().map(|inner| inner as _);
    while let Some(candidate) = current {
        if let Some(found) = candidate.downcast_ref::<CompressFailure>() {
            return Some(found.clone());
        }
        // `io::Error::source` skips its own payload, so look inside nested I/O errors directly.
        current = match candidate.downcast_ref::<io::Error>() {
            Some(inner) => inner.get_ref().map(|payload| payload as _),
            None => candidate.source(),
        };
    }
    None
}

/// Classifies an error from a flate2 decoder.
pub fn inflate_failure(error: &io::Error, what: &str) -> CompressFailure {
    if let Some(found) = failure_in(error) {
        return found;
    }
    match error.kind() {
        io::ErrorKind::UnexpectedEof => failure(
            "E_COMPRESS_TRUNCATED",
            format!("the {what} data ends before the stream is complete"),
        ),
        io::ErrorKind::InvalidInput | io::ErrorKind::InvalidData => failure(
            "E_COMPRESS_CORRUPT",
            format!("the {what} data is not valid: {error}"),
        ),
        _ => failure(
            "E_COMPRESS_CORRUPT",
            format!("the {what} data could not be read: {error}"),
        ),
    }
}

/// Copies `reader` into `sink` in chunks, counting the bytes and failing at `limit`.
pub fn copy_bounded(
    reader: &mut dyn Read,
    sink: &mut dyn FnMut(&[u8]) -> Result<(), CompressFailure>,
    total: &mut u64,
    limit: u64,
    what: &str,
) -> Result<(), CompressFailure> {
    let mut chunk = vec![0u8; CHUNK];
    loop {
        let count = match reader.read(&mut chunk) {
            Ok(0) => return Ok(()),
            Ok(count) => count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(inflate_failure(&error, what)),
        };
        if total.saturating_add(count as u64) > limit {
            return Err(limit_failure(limit));
        }
        *total += count as u64;
        sink(&chunk[..count])?;
    }
}

/// Inflates every gzip member of `input` into `sink`. Returns the bytes produced.
pub fn gunzip_into(
    input: &mut dyn BufRead,
    sink: &mut dyn FnMut(&[u8]) -> Result<(), CompressFailure>,
    limit: u64,
) -> Result<u64, CompressFailure> {
    let mut total = 0u64;
    let mut members = 0u64;
    loop {
        let buffered = input
            .fill_buf()
            .map_err(|error| inflate_failure(&error, "gzip"))?;
        if buffered.is_empty() {
            if members == 0 {
                return Err(failure(
                    "E_COMPRESS_TRUNCATED",
                    "the gzip data is empty: a gzip stream has at least a header",
                ));
            }
            return Ok(total);
        }
        if members > 0 && (buffered[0] != 0x1f || (buffered.len() > 1 && buffered[1] != 0x8b)) {
            return Err(failure(
                "E_COMPRESS_TRAILING_DATA",
                format!(
                    "data follows the last gzip member and is not another member (after {members} member{})",
                    if members == 1 { "" } else { "s" }
                ),
            ));
        }
        let mut decoder = GzDecoder::new(&mut *input);
        copy_bounded(&mut decoder, sink, &mut total, limit, "gzip")?;
        members += 1;
    }
}

/// Inflates one zlib stream of `input` into `sink`. Returns the bytes produced.
pub fn zlib_inflate_into(
    input: &mut dyn BufRead,
    sink: &mut dyn FnMut(&[u8]) -> Result<(), CompressFailure>,
    limit: u64,
) -> Result<u64, CompressFailure> {
    let mut total = 0u64;
    if input
        .fill_buf()
        .map_err(|error| inflate_failure(&error, "zlib"))?
        .is_empty()
    {
        return Err(failure(
            "E_COMPRESS_TRUNCATED",
            "the zlib data is empty: a zlib stream has at least a header",
        ));
    }
    {
        let mut decoder = ZlibDecoder::new(&mut *input);
        copy_bounded(&mut decoder, sink, &mut total, limit, "zlib")?;
    }
    if !input
        .fill_buf()
        .map_err(|error| inflate_failure(&error, "zlib"))?
        .is_empty()
    {
        return Err(failure(
            "E_COMPRESS_TRAILING_DATA",
            "data follows the end of the zlib stream",
        ));
    }
    Ok(total)
}

pub fn gzip_compress(data: &[u8]) -> Vec<u8> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(data)
        .expect("writing to a Vec cannot fail");
    encoder.finish().expect("writing to a Vec cannot fail")
}

pub fn zlib_compress(data: &[u8]) -> Vec<u8> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(data)
        .expect("writing to a Vec cannot fail");
    encoder.finish().expect("writing to a Vec cannot fail")
}

/// The ceiling for an in-memory inflate: the caller's `max_output`, never above `ceiling`.
pub fn effective_limit(max_output: &BigInt, ceiling: u64) -> Result<u64, CompressFailure> {
    if max_output.is_negative() {
        return Err(failure(
            "E_COMPRESS_ARGUMENT",
            "max_output must not be negative",
        ));
    }
    let asked = max_output.to_usize().map_or(u64::MAX, |value| value as u64);
    Ok(asked.min(ceiling))
}

pub fn inflate_bytes(
    gzip: bool,
    data: &[u8],
    max_output: &BigInt,
    ceiling: u64,
) -> Result<Vec<u8>, CompressFailure> {
    let limit = effective_limit(max_output, ceiling)?;
    let mut output = Vec::new();
    let mut sink = |chunk: &[u8]| {
        output.extend_from_slice(chunk);
        Ok(())
    };
    let mut input = data;
    if gzip {
        gunzip_into(&mut input, &mut sink, limit)?;
    } else {
        zlib_inflate_into(&mut input, &mut sink, limit)?;
    }
    Ok(output)
}
