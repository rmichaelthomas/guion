//! Portable, deterministic pieces of the data standard library.
#[path = "csv_columns.rs"]
pub mod csv_columns;
#[path = "resources.rs"]
pub mod resources;

use std::path::Path;

pub use resources::{
    CsvError, CsvRow, FileReaderHandle, FileReaderRegistry, ReaderError, ReaderErrorKind,
    SourceLine,
};

pub use guion_vm_core::data::*;
pub use guion_vm_core::json_try::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectoryEntry {
    pub relative_path: String,
    pub kind: String,
}

pub fn fs_walk(root: &Path, max_entries: usize) -> Result<Vec<DirectoryEntry>, DataError> {
    let root_metadata = std::fs::symlink_metadata(root).map_err(|error| {
        DataError::invalid("E_FS_WALK", format!("cannot inspect walk root: {error}"))
    })?;
    if root_metadata.file_type().is_symlink() || !root_metadata.is_dir() {
        return Err(DataError::invalid(
            "E_FS_WALK_ROOT",
            "walk root must be a directory and cannot be a symlink",
        ));
    }
    let mut directories = vec![(root.to_path_buf(), String::new())];
    let mut entries = Vec::new();
    while let Some((directory, prefix)) = directories.pop() {
        let children = std::fs::read_dir(&directory).map_err(|error| {
            DataError::invalid(
                "E_FS_WALK",
                format!("cannot read directory `{prefix}`: {error}"),
            )
        })?;
        let mut children = children.collect::<Result<Vec<_>, _>>().map_err(|error| {
            DataError::invalid(
                "E_FS_WALK",
                format!("cannot read a directory entry: {error}"),
            )
        })?;
        children.sort_by_key(|child| child.file_name());
        for child in children {
            let name = child.file_name().into_string().map_err(|_| {
                DataError::invalid("E_FS_WALK_UTF8", "walk path is not valid UTF-8")
            })?;
            let relative_path = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };
            let metadata = std::fs::symlink_metadata(child.path()).map_err(|error| {
                DataError::invalid(
                    "E_FS_WALK",
                    format!("cannot inspect `{relative_path}`: {error}"),
                )
            })?;
            let kind = if metadata.file_type().is_symlink() {
                "symlink"
            } else if metadata.is_dir() {
                "directory"
            } else if metadata.is_file() {
                "file"
            } else {
                "other"
            };
            if entries.len() >= max_entries {
                return Err(DataError::invalid(
                    "E_FS_WALK_LIMIT",
                    "directory traversal exceeds the configured entry limit",
                ));
            }
            entries.push(DirectoryEntry {
                relative_path: relative_path.clone(),
                kind: kind.into(),
            });
            if kind == "directory" {
                directories.push((child.path(), relative_path));
            }
        }
    }
    entries.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    Ok(entries)
}
