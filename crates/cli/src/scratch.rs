//! The private `temp:` folder a run gets: fresh, owner-only, under the operating system's
//! temporary directory (which honors `TMPDIR`), and removed when the run ends, including after
//! a failure.

use std::path::{Path, PathBuf};

pub struct ScratchDir {
    path: PathBuf,
    keep: bool,
}

impl ScratchDir {
    pub fn create() -> Result<Self, String> {
        let base = std::env::temp_dir();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or(0);
        for attempt in 0..32u32 {
            let candidate = base.join(format!(
                "guion-temp-{}-{nanos:x}-{attempt}",
                std::process::id()
            ));
            let mut builder = std::fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            match builder.create(&candidate) {
                Ok(()) => {
                    // Resolved once so a symlinked temporary directory (macOS `/var`) is not
                    // mistaken for an escape later.
                    let path = candidate.canonicalize().unwrap_or(candidate);
                    return Ok(Self { path, keep: false });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return Err(format!(
                        "cannot create a temp folder under {}: {error}",
                        base.display()
                    ))
                }
            }
        }
        Err(format!(
            "cannot create a unique temp folder under {}",
            base.display()
        ))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Leave the folder in place when the run ends (`--keep-temp`).
    pub fn keep(&mut self) {
        self.keep = true;
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        if !self.keep {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}
