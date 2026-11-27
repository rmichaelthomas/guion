//! Filesystem operations behind `Op::Fs`. Every path goes through the same grant checks as
//! `read_text` and `write_text`: relative to a granted root, no parent traversal, confined after
//! symlink resolution. Mutating operations need the file-write grant. `remove` and `rename` act
//! on the named entry itself, so a symlink is removed or moved rather than its target.
use super::{
    file_error_result, file_io_error_code, host_file_entry_path, host_file_path,
    host_file_path_no_follow, HostPolicy, RuntimeError, Shared, Span, Value,
};
use guion_model::{BigInt, FsFunction};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// The most bytes one ranged read returns, so a caller holds a bounded block.
const MAX_RANGE_BYTES: u64 = 16 * 1024 * 1024;
const TEMP_DIRECTORY: &str = ".guion-tmp";
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

type Outcome = Result<Value, RuntimeError>;

pub(crate) fn file_error(
    error: &std::io::Error,
    fallback: &'static str,
    what: &str,
    path: &str,
    span: Span,
) -> RuntimeError {
    RuntimeError::new(
        file_io_error_code(error, fallback),
        format!("cannot {what} `{path}`: {error}"),
        span,
    )
}

fn ok(value: Value) -> Value {
    Value::ResultOk(Box::new(value))
}

fn int(value: i64) -> Value {
    Value::Int(BigInt::from_i64(value))
}

fn granted_roots(policy: &HostPolicy) -> Vec<PathBuf> {
    policy
        .file_root
        .iter()
        .chain(policy.additional_file_roots.iter())
        .filter_map(|root| root.canonicalize().ok())
        .collect()
}

/// Creates `requested` and any missing parents, one directory at a time, so every step is
/// confined by the same resolver as an ordinary write.
pub(crate) fn make_dir_all(
    policy: &HostPolicy,
    requested: &str,
    span: Span,
) -> Result<PathBuf, RuntimeError> {
    if let Some(parent) = Path::new(requested).parent() {
        let parent = parent.to_string_lossy();
        if !parent.is_empty() && host_file_path(policy, &parent, false, span).is_err() {
            make_dir_all(policy, &parent, span)?;
        }
    }
    let resolved = host_file_path(policy, requested, true, span)?;
    match fs::create_dir(&resolved) {
        Ok(()) => Ok(resolved),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists && resolved.is_dir() => {
            Ok(resolved)
        }
        Err(error) => Err(file_error(
            &error,
            "R_FILE_WRITE",
            "create directory",
            requested,
            span,
        )),
    }
}

fn temp_name(prefix: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or(0);
    let count = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}-{}-{nanos:x}-{count}", std::process::id())
}

/// Permission bits from an `Int` mode: 0 to 0o7777, else `R_FILE_MODE_INVALID`.
fn mode_bits(mode: &BigInt, span: Span) -> Result<u32, RuntimeError> {
    match mode.to_i64() {
        Some(bits @ 0..=0o7777) => Ok(bits as u32),
        _ => Err(RuntimeError::new(
            "R_FILE_MODE_INVALID",
            format!(
                "`{mode}` is not a file mode: write an Int from 0 to 0o7777 such as 0o600 or 0o755"
            ),
            span,
        )),
    }
}

#[cfg(unix)]
fn set_mode(resolved: &Path, bits: u32, path: &str, span: Span) -> Result<(), RuntimeError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(resolved, fs::Permissions::from_mode(bits))
        .map_err(|error| file_error(&error, "R_FILE_WRITE", "change the mode of", path, span))
}

#[cfg(not(unix))]
fn set_mode(_resolved: &Path, _bits: u32, path: &str, span: Span) -> Result<(), RuntimeError> {
    Err(RuntimeError::new(
        "R_FILE_UNSUPPORTED",
        format!(
            "cannot change the mode of `{path}`: file modes are not supported on this platform"
        ),
        span,
    ))
}

/// Creates the symbolic link `link` holding `target` as given, never resolving or checking it.
#[cfg(unix)]
fn make_link(target: &str, link: &Path, link_name: &str, span: Span) -> Result<(), RuntimeError> {
    std::os::unix::fs::symlink(target, link).map_err(|error| {
        let code = if error.kind() == std::io::ErrorKind::AlreadyExists {
            "R_FILE_EXISTS"
        } else {
            "R_FILE_WRITE"
        };
        file_error(&error, code, "create the link", link_name, span)
    })
}

#[cfg(not(unix))]
fn make_link(_target: &str, _link: &Path, link_name: &str, span: Span) -> Result<(), RuntimeError> {
    Err(RuntimeError::new(
        "R_FILE_UNSUPPORTED",
        format!("cannot create the link `{link_name}`: symbolic links are not supported on this platform"),
        span,
    ))
}

#[cfg(unix)]
fn read_mode(resolved: &Path, path: &str, span: Span) -> Result<u32, RuntimeError> {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(resolved)
        .map(|metadata| metadata.permissions().mode() & 0o7777)
        .map_err(|error| file_error(&error, "R_FILE_READ", "read the mode of", path, span))
}

#[cfg(not(unix))]
fn read_mode(_resolved: &Path, path: &str, span: Span) -> Result<u32, RuntimeError> {
    Err(RuntimeError::new(
        "R_FILE_UNSUPPORTED",
        format!("cannot read the mode of `{path}`: file modes are not supported on this platform"),
        span,
    ))
}

/// Resolves the entry `path` names without following a final link, using only the read grant.
/// Every read root is searched in order and the first that holds the entry wins (a dangling link
/// counts as an entry); a path with no final name (`.`, a root alias) is the root itself. A
/// symlink among the leading components is refused, as for `fs_walk`, so the walk cannot leave
/// the root through one.
fn read_entry_path(policy: &HostPolicy, path: &str, span: Span) -> Result<PathBuf, RuntimeError> {
    host_file_path_no_follow(policy, path, span)
}

fn u64_value(value: u64) -> Value {
    Value::Int(BigInt::parse(&value.to_string()).expect("a u64 is a valid integer"))
}

/// Bytes the entry occupies on disk (`st_blocks * 512`, as `du -k` times 1024): never follows
/// links, skips other devices (`du -x`) and counts each inode once, so hard links add up once.
#[cfg(unix)]
fn disk_usage(resolved: &Path, path: &str, span: Span) -> Result<u64, RuntimeError> {
    use std::collections::HashSet;
    use std::os::unix::fs::MetadataExt;
    let start = fs::symlink_metadata(resolved)
        .map_err(|error| file_error(&error, "R_FILE_READ", "measure", path, span))?;
    let device = start.dev();
    let mut seen: HashSet<(u64, u64)> = HashSet::new();
    let mut total: u64 = 0;
    let mut stack: Vec<(PathBuf, fs::Metadata)> = vec![(resolved.to_path_buf(), start)];
    while let Some((entry, metadata)) = stack.pop() {
        if metadata.dev() != device {
            continue;
        }
        // Only a non-directory with several names can be reached twice, as `du` tracks it.
        if !metadata.file_type().is_dir()
            && metadata.nlink() > 1
            && !seen.insert((metadata.dev(), metadata.ino()))
        {
            continue;
        }
        total = total.saturating_add(metadata.blocks().saturating_mul(512));
        if !metadata.file_type().is_dir() {
            continue;
        }
        let relative = |entry: &Path| -> String {
            match entry.strip_prefix(resolved) {
                Ok(rest) if rest.as_os_str().is_empty() => path.to_owned(),
                Ok(rest) => format!("{}/{}", path.trim_end_matches('/'), rest.display()),
                Err(_) => path.to_owned(),
            }
        };
        let children = fs::read_dir(&entry).map_err(|error| {
            file_error(
                &error,
                "R_FILE_READ",
                "read the directory",
                &relative(&entry),
                span,
            )
        })?;
        for child in children {
            let child = child.map_err(|error| {
                file_error(
                    &error,
                    "R_FILE_READ",
                    "read the directory",
                    &relative(&entry),
                    span,
                )
            })?;
            let child_path = child.path();
            let child_metadata = match fs::symlink_metadata(&child_path) {
                Ok(metadata) => metadata,
                // Removed since the directory was listed: skipped, as `du` does.
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => {
                    return Err(file_error(
                        &error,
                        "R_FILE_READ",
                        "measure",
                        &relative(&child_path),
                        span,
                    ));
                }
            };
            stack.push((child_path, child_metadata));
        }
    }
    Ok(total)
}

#[cfg(not(unix))]
fn disk_usage(_resolved: &Path, path: &str, span: Span) -> Result<u64, RuntimeError> {
    Err(RuntimeError::new(
        "R_FILE_UNSUPPORTED",
        format!("cannot measure `{path}`: disk usage is not supported on this platform"),
        span,
    ))
}

/// Bytes available to an unprivileged user on the volume holding `resolved` (`f_bavail * f_frsize`).
#[cfg(unix)]
fn free_space(resolved: &Path, path: &str, span: Span) -> Result<u64, RuntimeError> {
    use std::os::unix::ffi::OsStrExt;
    let c_path = std::ffi::CString::new(resolved.as_os_str().as_bytes()).map_err(|_| {
        RuntimeError::new(
            "R_FILE_READ",
            format!("cannot read the free space of `{path}`: the path contains a NUL byte"),
            span,
        )
    })?;
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: `c_path` is a valid NUL-terminated string and `stat` a valid out-pointer.
    if unsafe { libc::statvfs(c_path.as_ptr(), &mut stat) } != 0 {
        let error = std::io::Error::last_os_error();
        return Err(file_error(
            &error,
            "R_FILE_READ",
            "read the free space of",
            path,
            span,
        ));
    }
    Ok((stat.f_bavail as u64).saturating_mul(stat.f_frsize as u64))
}

#[cfg(not(unix))]
fn free_space(_resolved: &Path, path: &str, span: Span) -> Result<u64, RuntimeError> {
    Err(RuntimeError::new(
        "R_FILE_UNSUPPORTED",
        format!("cannot read the free space of `{path}`: not supported on this platform"),
        span,
    ))
}

pub(crate) fn run(
    policy: &HostPolicy,
    function: FsFunction,
    operands: &[String],
    span: Span,
) -> Outcome {
    match run_inner(policy, function, operands, span) {
        Ok(value) => Ok(value),
        Err(error) if error.code == "R_COLLECTION_LIMIT" || error.code == "R_FILE_LIMIT" => {
            Err(error)
        }
        Err(error) => Ok(file_error_result(error)),
    }
}

fn run_inner(
    policy: &HostPolicy,
    function: FsFunction,
    operands: &[String],
    span: Span,
) -> Outcome {
    let path = operands[0].as_str();
    match function {
        FsFunction::Exists | FsFunction::Kind => {
            let resolved = match host_file_path(policy, path, false, span) {
                Ok(resolved) => resolved,
                Err(error) if error.code == "R_FILE_NOT_FOUND" => {
                    return Ok(ok(if function == FsFunction::Exists {
                        Value::Bool(false)
                    } else {
                        Value::Text("missing".into())
                    }));
                }
                Err(error) => return Err(error),
            };
            if function == FsFunction::Exists {
                return Ok(ok(Value::Bool(true)));
            }
            let kind = match fs::metadata(&resolved) {
                Ok(metadata) if metadata.is_dir() => "directory",
                Ok(metadata) if metadata.is_file() => "file",
                Ok(_) => "other",
                Err(error) => {
                    return Err(file_error(&error, "R_FILE_READ", "inspect", path, span));
                }
            };
            Ok(ok(Value::Text(kind.into())))
        }
        FsFunction::Size | FsFunction::ModifiedMs => {
            let resolved = host_file_path(policy, path, false, span)?;
            let metadata = fs::metadata(&resolved)
                .map_err(|error| file_error(&error, "R_FILE_READ", "inspect", path, span))?;
            if function == FsFunction::Size {
                return Ok(ok(int(i64::try_from(metadata.len()).unwrap_or(i64::MAX))));
            }
            let modified = metadata.modified().map_err(|error| {
                file_error(
                    &error,
                    "R_FILE_READ",
                    "read the modified time of",
                    path,
                    span,
                )
            })?;
            let millis = match modified.duration_since(UNIX_EPOCH) {
                Ok(elapsed) => i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX),
                Err(before) => -i64::try_from(before.duration().as_millis()).unwrap_or(i64::MAX),
            };
            Ok(ok(int(millis)))
        }
        FsFunction::Sha256 => {
            use sha2::{Digest, Sha256};
            use std::io::Read;
            let resolved = host_file_path(policy, path, false, span)?;
            let mut file = fs::File::open(&resolved)
                .map_err(|error| file_error(&error, "R_FILE_READ", "open", path, span))?;
            if !file
                .metadata()
                .map_err(|error| file_error(&error, "R_FILE_READ", "inspect", path, span))?
                .is_file()
            {
                return Err(RuntimeError::new(
                    "R_FILE_READ",
                    format!("cannot digest `{path}`: it is not a regular file"),
                    span,
                ));
            }
            // Streamed in 64 KiB blocks: memory stays constant however large the file is, and the
            // read ceiling for `read_text` does not apply because nothing is held.
            let mut hasher = Sha256::new();
            let mut block = vec![0u8; 64 * 1024];
            loop {
                match file.read(&mut block) {
                    Ok(0) => break,
                    Ok(count) => hasher.update(&block[..count]),
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(error) => {
                        return Err(file_error(&error, "R_FILE_READ", "read", path, span));
                    }
                }
            }
            Ok(ok(Value::Bytes(Shared::new(hasher.finalize().to_vec()))))
        }
        FsFunction::ListDir => {
            let resolved = host_file_path(policy, path, false, span)?;
            let entries = fs::read_dir(&resolved)
                .map_err(|error| file_error(&error, "R_FILE_READ", "list", path, span))?;
            let mut names = Vec::new();
            for entry in entries {
                let entry =
                    entry.map_err(|error| file_error(&error, "R_FILE_READ", "list", path, span))?;
                if let Some(name) = entry.file_name().to_str() {
                    names.push(name.to_owned());
                }
                if names.len() > policy.max_collection_items {
                    return Err(RuntimeError::new(
                        "R_COLLECTION_LIMIT",
                        "directory listing exceeds the configured collection limit",
                        span,
                    ));
                }
            }
            names.sort();
            let items = names
                .into_iter()
                .map(|name| Value::Text(name.as_str().into()))
                .collect();
            Ok(ok(Value::from_list_items(items)))
        }
        FsFunction::MakeDir => {
            make_dir_all(policy, path, span)?;
            Ok(ok(Value::Unit))
        }
        FsFunction::Remove | FsFunction::RemoveAll => {
            let resolved = host_file_entry_path(policy, path, span)?;
            if granted_roots(policy).contains(&resolved) {
                return Err(RuntimeError::new(
                    "R_HOST_DENIED",
                    "the granted file root itself cannot be removed",
                    span,
                ));
            }
            let metadata = fs::symlink_metadata(&resolved)
                .map_err(|error| file_error(&error, "R_FILE_WRITE", "inspect", path, span))?;
            let result = if metadata.is_dir() {
                if function == FsFunction::RemoveAll {
                    fs::remove_dir_all(&resolved)
                } else {
                    fs::remove_dir(&resolved)
                }
            } else {
                fs::remove_file(&resolved)
            };
            result.map_err(|error| file_error(&error, "R_FILE_WRITE", "remove", path, span))?;
            Ok(ok(Value::Unit))
        }
        FsFunction::Rename | FsFunction::Copy => {
            let destination = operands[1].as_str();
            if function == FsFunction::Rename {
                let source_path = host_file_entry_path(policy, path, span)?;
                let destination_path = host_file_entry_path(policy, destination, span)?;
                let roots = granted_roots(policy);
                if roots.contains(&source_path) || roots.contains(&destination_path) {
                    return Err(RuntimeError::new(
                        "R_HOST_DENIED",
                        "the granted file root itself cannot be renamed or replaced",
                        span,
                    ));
                }
                fs::symlink_metadata(&source_path)
                    .map_err(|error| file_error(&error, "R_FILE_WRITE", "rename", path, span))?;
                fs::rename(&source_path, &destination_path)
                    .map_err(|error| file_error(&error, "R_FILE_WRITE", "rename", path, span))?;
                return Ok(ok(Value::Unit));
            }
            let source_path = host_file_path(policy, path, false, span)?;
            let destination_path = host_file_path(policy, destination, true, span)?;
            let length = fs::metadata(&source_path)
                .map_err(|error| file_error(&error, "R_FILE_READ", "inspect", path, span))?
                .len();
            if length as usize > policy.max_file_bytes {
                return Err(RuntimeError::new(
                    "R_FILE_LIMIT",
                    format!("file `{path}` exceeds the configured byte limit"),
                    span,
                ));
            }
            let copied = fs::copy(&source_path, &destination_path)
                .map_err(|error| file_error(&error, "R_FILE_WRITE", "copy", path, span))?;
            Ok(ok(int(i64::try_from(copied).unwrap_or(i64::MAX))))
        }
        FsFunction::ReadRange => {
            use std::io::{Read, Seek, SeekFrom};
            let parse = |text: &str, what: &str| -> Result<u64, RuntimeError> {
                text.parse::<u64>().map_err(|_| {
                    RuntimeError::new(
                        "R_FILE_READ",
                        format!("the {what} of a ranged read is not a non-negative integer"),
                        span,
                    )
                })
            };
            let start = parse(operands[1].as_str(), "start")?;
            let length = parse(operands[2].as_str(), "length")?;
            if length > MAX_RANGE_BYTES {
                return Err(RuntimeError::new(
                    "R_FILE_READ",
                    format!("a ranged read is at most {MAX_RANGE_BYTES} bytes"),
                    span,
                ));
            }
            let resolved = host_file_path(policy, path, false, span)?;
            let mut file = fs::File::open(&resolved)
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
            file.seek(SeekFrom::Start(start))
                .map_err(|error| file_error(&error, "R_FILE_READ", "seek in", path, span))?;
            // Reads at most `length` bytes; a range that runs past the end is short.
            let mut data = Vec::new();
            file.take(length)
                .read_to_end(&mut data)
                .map_err(|error| file_error(&error, "R_FILE_READ", "read", path, span))?;
            Ok(ok(Value::Bytes(Shared::new(data))))
        }
        FsFunction::ConfinedPath => {
            let relative = operands[1].as_str();
            let joined = format!("{}/{}", path.trim_end_matches('/'), relative);
            let root = host_file_path(policy, path, false, span)?;
            let target = host_file_path(policy, &joined, false, span)?;
            // Both are resolved through symlinks, so a link inside the root that points outside it
            // no longer starts with the root.
            let root = root.canonicalize().unwrap_or(root);
            let target = target.canonicalize().unwrap_or(target);
            if !target.starts_with(&root) {
                return Err(RuntimeError::new(
                    "R_HOST_DENIED",
                    format!("`{relative}` resolves outside `{path}`"),
                    span,
                ));
            }
            Ok(ok(Value::Text(joined.as_str().into())))
        }
        FsFunction::SetMode => {
            // The wrapper passes the Int mode as its decimal text, as `fs_read_range` does.
            let mode = BigInt::parse(operands[1].as_str())
                .map_err(|_| RuntimeError::new("R_TYPE_INVALID", "file mode must be an Int", span))
                .and_then(|mode| mode_bits(&mode, span))?;
            let resolved = host_file_path(policy, path, false, span)?;
            set_mode(&resolved, mode, path, span)?;
            Ok(ok(Value::Unit))
        }
        FsFunction::Mode => {
            let resolved = host_file_path(policy, path, false, span)?;
            let bits = read_mode(&resolved, path, span)?;
            Ok(ok(Value::Int(BigInt::from_i64(i64::from(bits)))))
        }
        FsFunction::GzipCompressFile
        | FsFunction::GzipDecompressFile
        | FsFunction::TarWrite
        | FsFunction::TarRead
        | FsFunction::TarList => {
            super::compress::run_file_function(policy, function, operands, span)
        }
        FsFunction::MakeLink => {
            // `path` is the target text, stored as is; the second operand names the new entry,
            // whose parent is resolved and confined like a write and whose last component is
            // not followed.
            let link = operands[1].as_str();
            let resolved = host_file_entry_path(policy, link, span)?;
            make_link(path, &resolved, link, span)?;
            Ok(ok(Value::Unit))
        }
        FsFunction::DiskUsage => {
            // The named entry itself: a link at `path` is measured, not followed.
            let resolved = read_entry_path(policy, path, span)?;
            Ok(ok(u64_value(disk_usage(&resolved, path, span)?)))
        }
        FsFunction::FreeSpace => {
            let resolved = host_file_path(policy, path, false, span)?;
            Ok(ok(u64_value(free_space(&resolved, path, span)?)))
        }
        FsFunction::IsLink => {
            // A missing entry is not a link, as `fs_exists` gives false for it.
            let resolved = match read_entry_path(policy, path, span) {
                Ok(resolved) => resolved,
                Err(error) if error.code == "R_FILE_NOT_FOUND" => {
                    return Ok(ok(Value::Bool(false)));
                }
                // A leading component that is a regular file (`f.txt/x`) holds no link either;
                // the resolver reports ENOTDIR with the OS text, which is "Not a directory".
                Err(error)
                    if error.code == "R_FILE_READ" && error.message.contains("Not a directory") =>
                {
                    return Ok(ok(Value::Bool(false)));
                }
                Err(error) => return Err(error),
            };
            match fs::symlink_metadata(&resolved) {
                Ok(metadata) => Ok(ok(Value::Bool(metadata.file_type().is_symlink()))),
                Err(error)
                    if error.kind() == std::io::ErrorKind::NotFound
                        || error.kind() == std::io::ErrorKind::NotADirectory =>
                {
                    Ok(ok(Value::Bool(false)))
                }
                Err(error) => Err(file_error(&error, "R_FILE_READ", "inspect", path, span)),
            }
        }
        FsFunction::TempDir | FsFunction::TempFile => {
            let valid = !path.is_empty()
                && path.len() <= 64
                && path
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_');
            if !valid {
                return Err(RuntimeError::new(
                    "R_FILE_WRITE",
                    "temporary name prefix must be 1 to 64 ASCII letters, digits, `-` or `_`",
                    span,
                ));
            }
            make_dir_all(policy, TEMP_DIRECTORY, span)?;
            let relative = format!("{TEMP_DIRECTORY}/{}", temp_name(path));
            let resolved = host_file_path(policy, &relative, true, span)?;
            let created = if function == FsFunction::TempDir {
                fs::create_dir(&resolved)
            } else {
                fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&resolved)
                    .map(|_| ())
            };
            created.map_err(|error| {
                file_error(
                    &error,
                    "R_FILE_WRITE",
                    "create temporary entry under",
                    TEMP_DIRECTORY,
                    span,
                )
            })?;
            Ok(ok(Value::Text(relative.as_str().into())))
        }
    }
}

#[allow(dead_code)]
fn _shared_is_used(_: &Shared<Vec<Value>>) {}

#[cfg(all(test, unix))]
mod disk_usage_tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("guion-fsops-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn ok_value(outcome: Outcome) -> Value {
        match outcome.unwrap() {
            Value::ResultOk(inner) => *inner,
            other => panic!("expected Ok, got {other:?}"),
        }
    }

    fn call(policy: &HostPolicy, function: FsFunction, path: &str) -> Value {
        ok_value(run_inner(
            policy,
            function,
            &[path.to_owned()],
            Span::default(),
        ))
    }

    #[test]
    fn read_only_policy_does_not_follow_a_final_link() {
        let dir = scratch("readonly");
        let root = dir.join("root");
        fs::create_dir_all(root.join("d")).unwrap();
        fs::write(root.join("d/big.bin"), vec![7u8; 1 << 20]).unwrap();
        std::os::unix::fs::symlink("d", root.join("dlink")).unwrap();
        std::os::unix::fs::symlink("missing-target", root.join("dangling")).unwrap();
        std::os::unix::fs::symlink(&dir, root.join("outward")).unwrap();
        let policy = HostPolicy::default().with_file_access(&root, true, false);
        for link in ["dlink", "dangling", "outward"] {
            assert_eq!(
                call(&policy, FsFunction::IsLink, link),
                Value::Bool(true),
                "{link}"
            );
            let Value::Int(bytes) = call(&policy, FsFunction::DiskUsage, link) else {
                panic!("not an Int");
            };
            assert!(bytes.to_string().parse::<u64>().unwrap() < 65_536, "{link}");
        }
        assert_eq!(
            call(&policy, FsFunction::IsLink, "nope"),
            Value::Bool(false)
        );
        assert_eq!(call(&policy, FsFunction::IsLink, "d"), Value::Bool(false));
        assert_eq!(
            call(&policy, FsFunction::IsLink, "d/big.bin"),
            Value::Bool(false)
        );
        assert_eq!(call(&policy, FsFunction::IsLink, "."), Value::Bool(false));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_child_removed_after_listing_is_skipped() {
        // `disk_usage` lists a directory and then stats each child; a child that vanishes in
        // between is NotFound. The race cannot be forced from outside, so a dangling entry
        // that `read_dir` reports and `symlink_metadata` still finds is the closest real case;
        // the NotFound arm itself is a one-line `continue`.
        let dir = scratch("vanish");
        fs::create_dir_all(dir.join("d")).unwrap();
        std::os::unix::fs::symlink("nowhere", dir.join("d/dangling")).unwrap();
        let bytes = disk_usage(&dir.join("d"), "d", Span::default()).unwrap();
        assert!(bytes < 65_536);
        let _ = fs::remove_dir_all(&dir);
    }
}
