//! gzip (RFC 1952), zlib (RFC 1950) and tar for `std:compress` and `std:tar`.
//!
//! Every inflate path in this module counts the bytes it produces and stops at a ceiling, so a
//! small input cannot grow without bound: the in-memory functions take the caller's `max_output`
//! (capped by `[runtime] decompress_limit_bytes`), and the file and tar functions use that
//! runtime ceiling. The file functions stream through 64 KiB buffers and never hold a whole file.
//!
//! gzip input may hold several members; their output is concatenated, as `gzip -d` and Python's
//! `gzip.decompress` do. Bytes after the last member that do not begin another member, and bytes
//! after a zlib stream, are refused with `E_COMPRESS_TRAILING_DATA` rather than ignored.
use super::{host_file_path, HostPolicy, RuntimeError, Span, Value};
use crate::fs_ops::{self, file_error};
use flate2::bufread::{GzDecoder, ZlibDecoder};
use flate2::write::{GzEncoder, ZlibEncoder};
use flate2::Compression;
use guion_model::{BigInt, FsFunction};
use std::fmt;
use std::fs;
use std::io::{self, BufRead, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const CHUNK: usize = 64 * 1024;
static PART_COUNTER: AtomicU64 = AtomicU64::new(0);

/// A typed failure of a compression or tar operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CompressFailure {
    pub code: &'static str,
    pub message: String,
}

impl fmt::Display for CompressFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for CompressFailure {}

fn failure(code: &'static str, message: impl Into<String>) -> CompressFailure {
    CompressFailure {
        code,
        message: message.into(),
    }
}

fn limit_failure(limit: u64) -> CompressFailure {
    failure(
        "E_COMPRESS_LIMIT",
        format!("decompressed output would exceed the ceiling of {limit} bytes"),
    )
}

/// Finds a [`CompressFailure`] in an I/O error or anything it wraps.
fn failure_in(error: &io::Error) -> Option<CompressFailure> {
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
fn inflate_failure(error: &io::Error, what: &str) -> CompressFailure {
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
fn copy_bounded(
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
fn gunzip_into(
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
fn zlib_inflate_into(
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

pub(crate) fn gzip_compress(data: &[u8]) -> Vec<u8> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(data)
        .expect("writing to a Vec cannot fail");
    encoder.finish().expect("writing to a Vec cannot fail")
}

pub(crate) fn zlib_compress(data: &[u8]) -> Vec<u8> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(data)
        .expect("writing to a Vec cannot fail");
    encoder.finish().expect("writing to a Vec cannot fail")
}

/// The ceiling for an in-memory inflate: the caller's `max_output`, never above the runtime's.
fn effective_limit(max_output: &BigInt, policy: &HostPolicy) -> Result<u64, CompressFailure> {
    if max_output.is_negative() {
        return Err(failure(
            "E_COMPRESS_ARGUMENT",
            "max_output must not be negative",
        ));
    }
    let asked = max_output.to_usize().map_or(u64::MAX, |value| value as u64);
    Ok(asked.min(policy.decompress_limit_bytes as u64))
}

pub(crate) fn inflate_bytes(
    gzip: bool,
    data: &[u8],
    max_output: &BigInt,
    policy: &HostPolicy,
) -> Result<Vec<u8>, CompressFailure> {
    let limit = effective_limit(max_output, policy)?;
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

fn failure_error(failure: CompressFailure, span: Span) -> RuntimeError {
    RuntimeError::new(failure.code, failure.message, span)
}

fn int(value: u64) -> Value {
    Value::Int(BigInt::from_i64(i64::try_from(value).unwrap_or(i64::MAX)))
}

fn ok(value: Value) -> Value {
    Value::ResultOk(Box::new(value))
}

/// A sibling path for a file being written, renamed over the destination on success so a refused
/// or failed run leaves nothing partial behind.
struct PartFile {
    path: PathBuf,
    destination: PathBuf,
    done: bool,
}

impl PartFile {
    fn new(destination: &Path) -> Self {
        let name = destination
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let count = PART_COUNTER.fetch_add(1, Ordering::Relaxed);
        let part = format!(".{name}.guion-part-{}-{count}", std::process::id());
        PartFile {
            path: destination.with_file_name(part),
            destination: destination.to_path_buf(),
            done: false,
        }
    }

    fn commit(mut self) -> io::Result<()> {
        fs::rename(&self.path, &self.destination)?;
        self.done = true;
        Ok(())
    }
}

impl Drop for PartFile {
    fn drop(&mut self) {
        if !self.done {
            let _ = fs::remove_file(&self.path);
        }
    }
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

fn open_input(
    policy: &HostPolicy,
    path: &str,
    span: Span,
) -> Result<(PathBuf, BufReader<fs::File>), RuntimeError> {
    let resolved = host_file_path(policy, path, false, span)?;
    let file = fs::File::open(&resolved)
        .map_err(|error| file_error(&error, "R_FILE_READ", "open", path, span))?;
    if !file
        .metadata()
        .map_err(|error| file_error(&error, "R_FILE_READ", "inspect", path, span))?
        .is_file()
    {
        return Err(RuntimeError::new(
            "R_FILE_READ",
            format!("cannot read `{path}`: it is not a regular file"),
            span,
        ));
    }
    Ok((resolved, BufReader::with_capacity(CHUNK, file)))
}

fn create_part(
    destination: &Path,
    shown: &str,
    span: Span,
) -> Result<(PartFile, BufWriter<fs::File>), RuntimeError> {
    let part = PartFile::new(destination);
    let file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&part.path)
        .map_err(|error| {
            file_error(&error, "R_FILE_WRITE", "create a file next to", shown, span)
        })?;
    Ok((part, BufWriter::with_capacity(CHUNK, file)))
}

pub(crate) fn run_file_function(
    policy: &HostPolicy,
    function: FsFunction,
    operands: &[String],
    span: Span,
) -> Result<Value, RuntimeError> {
    match function {
        FsFunction::GzipCompressFile => gzip_compress_file(policy, operands, span),
        FsFunction::GzipDecompressFile => gzip_decompress_file(policy, operands, span),
        FsFunction::TarWrite => tar_write(policy, operands, span),
        FsFunction::TarRead => tar_read(policy, operands, span),
        FsFunction::TarList => tar_list(policy, operands, span),
        _ => unreachable!("not a compression function"),
    }
}

fn gzip_compress_file(
    policy: &HostPolicy,
    operands: &[String],
    span: Span,
) -> Result<Value, RuntimeError> {
    let (source, destination) = (operands[0].as_str(), operands[1].as_str());
    let (source_path, mut input) = open_input(policy, source, span)?;
    let destination_path = host_file_path(policy, destination, true, span)?;
    if same_file(&source_path, &destination_path) {
        return Err(RuntimeError::new(
            "R_FILE_WRITE",
            "the source and destination are the same file",
            span,
        ));
    }
    let (part, writer) = create_part(&destination_path, destination, span)?;
    let mut encoder = GzEncoder::new(writer, Compression::default());
    io::copy(&mut input, &mut encoder)
        .map_err(|error| file_error(&error, "R_FILE_WRITE", "compress", source, span))?;
    let mut writer = encoder
        .finish()
        .map_err(|error| file_error(&error, "R_FILE_WRITE", "write", destination, span))?;
    writer
        .flush()
        .map_err(|error| file_error(&error, "R_FILE_WRITE", "write", destination, span))?;
    let written = writer
        .get_ref()
        .metadata()
        .map_err(|error| file_error(&error, "R_FILE_WRITE", "inspect", destination, span))?
        .len();
    drop(writer);
    part.commit()
        .map_err(|error| file_error(&error, "R_FILE_WRITE", "write", destination, span))?;
    Ok(ok(int(written)))
}

fn gzip_decompress_file(
    policy: &HostPolicy,
    operands: &[String],
    span: Span,
) -> Result<Value, RuntimeError> {
    let (source, destination) = (operands[0].as_str(), operands[1].as_str());
    let (source_path, mut input) = open_input(policy, source, span)?;
    let destination_path = host_file_path(policy, destination, true, span)?;
    if same_file(&source_path, &destination_path) {
        return Err(RuntimeError::new(
            "R_FILE_WRITE",
            "the source and destination are the same file",
            span,
        ));
    }
    let (part, mut writer) = create_part(&destination_path, destination, span)?;
    let mut write_failure: Option<io::Error> = None;
    let mut sink = |chunk: &[u8]| match writer.write_all(chunk) {
        Ok(()) => Ok(()),
        Err(error) => {
            let message = format!("cannot write `{destination}`: {error}");
            write_failure = Some(error);
            Err(failure("R_FILE_WRITE", message))
        }
    };
    let produced = gunzip_into(&mut input, &mut sink, policy.decompress_limit_bytes as u64);
    let produced = match produced {
        Ok(produced) => produced,
        Err(found) => {
            if let Some(error) = write_failure {
                return Err(file_error(
                    &error,
                    "R_FILE_WRITE",
                    "write",
                    destination,
                    span,
                ));
            }
            return Err(failure_error(found, span));
        }
    };
    writer
        .flush()
        .map_err(|error| file_error(&error, "R_FILE_WRITE", "write", destination, span))?;
    drop(writer);
    part.commit()
        .map_err(|error| file_error(&error, "R_FILE_WRITE", "write", destination, span))?;
    Ok(ok(int(produced)))
}

// ---------------------------------------------------------------------------------------------
// tar

fn is_compressed_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".gz") || lower.ends_with(".tgz")
}

fn unsafe_path(message: impl Into<String>) -> CompressFailure {
    failure("E_TAR_UNSAFE_PATH", message)
}

fn tar_failure(error: &io::Error) -> CompressFailure {
    if let Some(found) = failure_in(error) {
        return found;
    }
    failure(
        "E_TAR_CORRUPT",
        format!("the tar archive is not valid: {error}"),
    )
}

/// Walks `directory` in sorted order, appending entries named relative to it.
fn append_tree<W: Write>(
    builder: &mut tar::Builder<W>,
    root: &Path,
    relative: &Path,
    count: &mut usize,
    max_entries: usize,
) -> Result<(), CompressFailure> {
    let directory = root.join(relative);
    let mut names = Vec::new();
    for entry in fs::read_dir(&directory).map_err(|error| {
        failure(
            "R_FILE_READ",
            format!("cannot list `{}`: {error}", directory.display()),
        )
    })? {
        let entry = entry.map_err(|error| failure("R_FILE_READ", error.to_string()))?;
        names.push(entry.file_name());
    }
    names.sort();
    for name in names {
        let child = relative.join(&name);
        let full = root.join(&child);
        let shown = child.to_string_lossy().into_owned();
        let Some(text) = child.to_str() else {
            return Err(unsafe_path(format!(
                "`{shown}` is not a valid UTF-8 name and cannot be archived"
            )));
        };
        let metadata = fs::symlink_metadata(&full).map_err(|error| {
            failure("R_FILE_READ", format!("cannot inspect `{shown}`: {error}"))
        })?;
        if metadata.file_type().is_symlink() {
            return Err(unsafe_path(format!(
                "`{shown}` is a symbolic link; tar_write does not archive links"
            )));
        }
        *count += 1;
        if *count > max_entries {
            return Err(failure(
                "E_TAR_LIMIT",
                format!("the directory holds more than {max_entries} entries"),
            ));
        }
        let mut header = tar::Header::new_gnu();
        header.set_mtime(0);
        header.set_uid(0);
        header.set_gid(0);
        if metadata.is_dir() {
            header.set_entry_type(tar::EntryType::Directory);
            header.set_mode(0o755);
            header.set_size(0);
            let name = format!("{text}/");
            builder
                .append_data(&mut header, &name, io::empty())
                .map_err(|error| {
                    failure("R_FILE_WRITE", format!("cannot write `{shown}`: {error}"))
                })?;
            append_tree(builder, root, &child, count, max_entries)?;
        } else if metadata.is_file() {
            header.set_entry_type(tar::EntryType::Regular);
            #[cfg(unix)]
            let mode = {
                use std::os::unix::fs::PermissionsExt;
                if metadata.permissions().mode() & 0o111 != 0 {
                    0o755
                } else {
                    0o644
                }
            };
            #[cfg(not(unix))]
            let mode = 0o644;
            header.set_mode(mode);
            header.set_size(metadata.len());
            let file = fs::File::open(&full).map_err(|error| {
                failure("R_FILE_READ", format!("cannot open `{shown}`: {error}"))
            })?;
            builder
                .append_data(&mut header, text, BufReader::with_capacity(CHUNK, file))
                .map_err(|error| {
                    failure("R_FILE_WRITE", format!("cannot write `{shown}`: {error}"))
                })?;
        } else {
            return Err(failure(
                "E_TAR_UNSUPPORTED_ENTRY",
                format!("`{shown}` is neither a regular file nor a directory"),
            ));
        }
    }
    Ok(())
}

fn tar_write(policy: &HostPolicy, operands: &[String], span: Span) -> Result<Value, RuntimeError> {
    let (directory, archive) = (operands[0].as_str(), operands[1].as_str());
    let root = host_file_path(policy, directory, false, span)?;
    if !root.is_dir() {
        return Err(RuntimeError::new(
            "R_FILE_READ",
            format!("cannot archive `{directory}`: it is not a directory"),
            span,
        ));
    }
    let archive_path = host_file_path(policy, archive, true, span)?;
    if archive_path.starts_with(&root) {
        return Err(failure_error(
            unsafe_path(format!(
                "`{archive}` would be written inside the directory `{directory}` it archives"
            )),
            span,
        ));
    }
    let (part, writer) = create_part(&archive_path, archive, span)?;
    let mut count = 0usize;
    let max_entries = policy.max_collection_items;
    let walked: Result<BufWriter<fs::File>, CompressFailure> = if is_compressed_name(archive) {
        let mut builder = tar::Builder::new(GzEncoder::new(writer, Compression::default()));
        append_tree(&mut builder, &root, Path::new(""), &mut count, max_entries).and_then(|()| {
            builder
                .into_inner()
                .and_then(GzEncoder::finish)
                .map_err(|error| {
                    failure("R_FILE_WRITE", format!("cannot write `{archive}`: {error}"))
                })
        })
    } else {
        let mut builder = tar::Builder::new(writer);
        append_tree(&mut builder, &root, Path::new(""), &mut count, max_entries).and_then(|()| {
            builder.into_inner().map_err(|error| {
                failure("R_FILE_WRITE", format!("cannot write `{archive}`: {error}"))
            })
        })
    };
    let mut writer = walked.map_err(|found| failure_error(found, span))?;
    writer
        .flush()
        .map_err(|error| file_error(&error, "R_FILE_WRITE", "write", archive, span))?;
    drop(writer);
    part.commit()
        .map_err(|error| file_error(&error, "R_FILE_WRITE", "write", archive, span))?;
    Ok(ok(int(count as u64)))
}

/// A reader that stops at the decompression ceiling and types decoder errors.
struct GuardedReader<R> {
    inner: R,
    produced: u64,
    limit: u64,
    what: &'static str,
}

impl<R: Read> Read for GuardedReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        match self.inner.read(buffer) {
            Ok(count) => {
                if self.produced.saturating_add(count as u64) > self.limit {
                    return Err(io::Error::other(limit_failure(self.limit)));
                }
                self.produced += count as u64;
                Ok(count)
            }
            Err(error) => Err(io::Error::other(inflate_failure(&error, self.what))),
        }
    }
}

/// Opens an archive for reading, undoing gzip when the file starts with its magic number.
fn open_archive(
    policy: &HostPolicy,
    path: &str,
    span: Span,
) -> Result<Box<dyn Read>, RuntimeError> {
    let (_, mut file) = open_input(policy, path, span)?;
    let magic = file
        .fill_buf()
        .map_err(|error| file_error(&error, "R_FILE_READ", "read", path, span))?;
    if magic.len() >= 2 && magic[0] == 0x1f && magic[1] == 0x8b {
        Ok(Box::new(GuardedReader {
            inner: GzDecoder::new(file),
            produced: 0,
            limit: policy.decompress_limit_bytes as u64,
            what: "gzip",
        }))
    } else {
        Ok(Box::new(file))
    }
}

/// An entry name made relative to the extraction directory, or an error naming the problem.
fn safe_relative(name: &str, what: &str) -> Result<Vec<String>, CompressFailure> {
    if name.starts_with('/') || name.starts_with('\\') {
        return Err(unsafe_path(format!("{what} `{name}` is an absolute path")));
    }
    let mut parts = Vec::new();
    for part in name.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                return Err(unsafe_path(format!(
                    "{what} `{name}` climbs out of the destination with `..`"
                )));
            }
            other if other.contains('\\') || other.contains('\0') || other.ends_with(':') => {
                return Err(unsafe_path(format!(
                    "{what} `{name}` has a component that is not a plain name"
                )));
            }
            other => parts.push(other.to_owned()),
        }
    }
    Ok(parts)
}

/// A link target that stays below the destination whatever else is extracted: relative, and
/// with no `..` component at all (so a chain of links cannot climb out).
fn safe_link_target(target: &str, name: &str) -> Result<Vec<String>, CompressFailure> {
    if target.is_empty() {
        return Err(unsafe_path(format!("link `{name}` has an empty target")));
    }
    if target.starts_with('/') {
        return Err(unsafe_path(format!(
            "link `{name}` points at the absolute path `{target}`"
        )));
    }
    if target.split('/').any(|part| part == "..") {
        return Err(unsafe_path(format!(
            "link `{name}` points at `{target}`, which climbs with `..`"
        )));
    }
    safe_relative(target, "link target")
}

fn join_destination(destination: &str, parts: &[String]) -> String {
    let mut path = destination.trim_end_matches('/').to_owned();
    if path.is_empty() {
        path.push('.');
    }
    for part in parts {
        path.push('/');
        path.push_str(part);
    }
    path
}

fn tar_read(policy: &HostPolicy, operands: &[String], span: Span) -> Result<Value, RuntimeError> {
    let (archive, destination) = (operands[0].as_str(), operands[1].as_str());
    let reader = open_archive(policy, archive, span)?;
    host_file_path(policy, destination, true, span)?;
    fs_ops::make_dir_all(policy, destination.trim_end_matches('/'), span)?;
    let mut entries_read = tar::Archive::new(reader);
    let entries = entries_read
        .entries()
        .map_err(|error| failure_error(tar_failure(&error), span))?;
    let limit = policy.decompress_limit_bytes as u64;
    let mut written_total = 0u64;
    let mut count = 0usize;
    for entry in entries {
        let mut entry = entry.map_err(|error| failure_error(tar_failure(&error), span))?;
        let entry_type = entry.header().entry_type();
        if matches!(
            entry_type,
            tar::EntryType::XGlobalHeader
                | tar::EntryType::XHeader
                | tar::EntryType::GNULongName
                | tar::EntryType::GNULongLink
        ) {
            continue;
        }
        let raw = entry.path_bytes().into_owned();
        let name = String::from_utf8(raw).map_err(|_| {
            failure_error(
                unsafe_path("an entry name is not valid UTF-8".to_owned()),
                span,
            )
        })?;
        let parts = safe_relative(&name, "entry").map_err(|found| failure_error(found, span))?;
        if parts.is_empty() {
            // The archive's own root ("./"): nothing to create.
            continue;
        }
        count += 1;
        if count > policy.max_collection_items {
            return Err(RuntimeError::new(
                "R_COLLECTION_LIMIT",
                "tar archive exceeds the configured collection limit",
                span,
            ));
        }
        let target_string = join_destination(destination, &parts);
        match entry_type {
            tar::EntryType::Directory => {
                fs_ops::make_dir_all(policy, &target_string, span)?;
            }
            tar::EntryType::Regular | tar::EntryType::Continuous | tar::EntryType::GNUSparse => {
                let size = entry.header().size().unwrap_or(0);
                if written_total.saturating_add(size) > limit {
                    return Err(failure_error(limit_failure(limit), span));
                }
                if parts.len() > 1 {
                    let parent = join_destination(destination, &parts[..parts.len() - 1]);
                    fs_ops::make_dir_all(policy, &parent, span)?;
                }
                let resolved = host_file_path(policy, &target_string, true, span)?;
                #[cfg(unix)]
                let mode = entry.header().mode().unwrap_or(0o644);
                let mut file = fs::File::create(&resolved).map_err(|error| {
                    file_error(&error, "R_FILE_WRITE", "create", &target_string, span)
                })?;
                let copied =
                    io::copy(&mut entry.by_ref().take(size), &mut file).map_err(|error| {
                        match failure_in(&error) {
                            Some(found) => failure_error(found, span),
                            None => {
                                file_error(&error, "R_FILE_WRITE", "extract", &target_string, span)
                            }
                        }
                    })?;
                if copied < size {
                    return Err(failure_error(
                        failure(
                            "E_TAR_CORRUPT",
                            format!("the entry `{name}` ends before its recorded size"),
                        ),
                        span,
                    ));
                }
                written_total += copied;
                #[cfg(unix)]
                if mode & 0o111 != 0 {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = fs::set_permissions(&resolved, fs::Permissions::from_mode(0o755));
                }
            }
            tar::EntryType::Symlink | tar::EntryType::Link => {
                let link = entry
                    .link_name_bytes()
                    .map(|bytes| bytes.into_owned())
                    .ok_or_else(|| {
                        failure_error(unsafe_path(format!("link `{name}` has no target")), span)
                    })?;
                let link = String::from_utf8(link).map_err(|_| {
                    failure_error(
                        unsafe_path(format!("the target of link `{name}` is not valid UTF-8")),
                        span,
                    )
                })?;
                let target_parts =
                    safe_link_target(&link, &name).map_err(|found| failure_error(found, span))?;
                if parts.len() > 1 {
                    let parent = join_destination(destination, &parts[..parts.len() - 1]);
                    fs_ops::make_dir_all(policy, &parent, span)?;
                }
                let resolved = host_file_path(policy, &target_string, true, span)?;
                if let Ok(existing) = fs::symlink_metadata(&resolved) {
                    if existing.is_dir() {
                        return Err(RuntimeError::new(
                            "R_FILE_WRITE",
                            format!("cannot create link `{name}`: a directory is in the way"),
                            span,
                        ));
                    }
                    let _ = fs::remove_file(&resolved);
                }
                if entry_type == tar::EntryType::Link {
                    // A hard link names another archive entry, relative to the archive root.
                    let source_string = join_destination(destination, &target_parts);
                    let source = host_file_path(policy, &source_string, false, span)?;
                    fs::hard_link(&source, &resolved).map_err(|error| {
                        file_error(&error, "R_FILE_WRITE", "link", &target_string, span)
                    })?;
                } else {
                    #[cfg(unix)]
                    std::os::unix::fs::symlink(&link, &resolved).map_err(|error| {
                        file_error(&error, "R_FILE_WRITE", "link", &target_string, span)
                    })?;
                    #[cfg(not(unix))]
                    return Err(failure_error(
                        failure(
                            "E_TAR_UNSUPPORTED_ENTRY",
                            "symbolic links in archives are only extracted on Unix hosts",
                        ),
                        span,
                    ));
                }
            }
            other => {
                return Err(failure_error(
                    failure(
                        "E_TAR_UNSUPPORTED_ENTRY",
                        format!("the entry `{name}` has the unsupported type {other:?}"),
                    ),
                    span,
                ));
            }
        }
    }
    Ok(ok(int(count as u64)))
}

fn tar_list(policy: &HostPolicy, operands: &[String], span: Span) -> Result<Value, RuntimeError> {
    let reader = open_archive(policy, operands[0].as_str(), span)?;
    let mut archive = tar::Archive::new(reader);
    let entries = archive
        .entries()
        .map_err(|error| failure_error(tar_failure(&error), span))?;
    let mut names = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| failure_error(tar_failure(&error), span))?;
        let entry_type = entry.header().entry_type();
        if matches!(
            entry_type,
            tar::EntryType::XGlobalHeader
                | tar::EntryType::XHeader
                | tar::EntryType::GNULongName
                | tar::EntryType::GNULongLink
        ) {
            continue;
        }
        let mut name = String::from_utf8_lossy(&entry.path_bytes()).into_owned();
        if entry_type == tar::EntryType::Directory && !name.ends_with('/') {
            name.push('/');
        }
        names.push(Value::Text(name.as_str().into()));
        if names.len() > policy.max_collection_items {
            return Err(RuntimeError::new(
                "R_COLLECTION_LIMIT",
                "tar listing exceeds the configured collection limit",
                span,
            ));
        }
    }
    Ok(ok(Value::from_list_items(names)))
}
