//! Filesystem operations behind `Op::Fs`. Every path goes through the same grant checks as
//! `read_text` and `write_text`: relative to a granted root, no parent traversal, confined after
//! symlink resolution. Mutating operations need the file-write grant. `remove` and `rename` act
//! on the named entry itself, so a symlink is removed or moved rather than its target.
use super::{
    file_error_result, file_io_error_code, host_file_entry_path, host_file_path, HostPolicy,
    RuntimeError, Shared, Span, Value,
};
use guion_model::{BigInt, FsFunction};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const TEMP_DIRECTORY: &str = ".guion-tmp";
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

type Outcome = Result<Value, RuntimeError>;

fn file_error(
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
fn make_dir_all(policy: &HostPolicy, requested: &str, span: Span) -> Result<PathBuf, RuntimeError> {
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
