//! The entropy behind `crypto_random_bytes`, `crypto_random_hex` and `crypto_random_base64url`.
//!
//! The default source is the operating system's cryptographic generator: `getentropy(2)` on
//! macOS and iOS, `getrandom(2)` on Linux and Android, and `/dev/urandom` elsewhere on Unix.
//! No other source is used, and nothing here is derived from a clock or from `std:random`.
//!
//! An embedding can pin the source with `HostPolicy::with_entropy_source` so a test sees known
//! bytes. The pin is a field of the Rust `HostPolicy` only: no guest function, `guion.toml` key
//! or command-line flag reaches it, and a policy that leaves it unset always draws from the OS.
use std::fmt;
use std::sync::{Arc, Mutex};

/// The most bytes one call may draw: 1 MiB. A larger or negative count is the typed error
/// `E_RANDOM_COUNT`, never a clamp.
pub const ENTROPY_MAX_BYTES: usize = 1024 * 1024;

type Fill = dyn Fn(&mut [u8]) -> Result<(), String> + Send + Sync;

/// A host-supplied replacement for the OS entropy source. Embedding-only.
#[derive(Clone)]
pub struct EntropySource(Arc<Fill>);

impl EntropySource {
    /// A source that fills each request with `fill`; an `Err` becomes `E_ENTROPY_UNAVAILABLE`.
    pub fn new(fill: impl Fn(&mut [u8]) -> Result<(), String> + Send + Sync + 'static) -> Self {
        EntropySource(Arc::new(fill))
    }

    /// A source that repeats `bytes` forever, continuing where the previous request stopped, so a
    /// test can assert exact values. An empty `bytes` is a source that always fails.
    pub fn fixed(bytes: Vec<u8>) -> Self {
        let position = Mutex::new(0usize);
        EntropySource::new(move |buffer| {
            if bytes.is_empty() {
                return Err("the pinned entropy source holds no bytes".into());
            }
            let mut position = position.lock().map_err(|_| "pinned source poisoned")?;
            for slot in buffer.iter_mut() {
                *slot = bytes[*position % bytes.len()];
                *position += 1;
            }
            *position %= bytes.len();
            Ok(())
        })
    }

    pub(crate) fn fill(&self, buffer: &mut [u8]) -> Result<(), String> {
        (self.0)(buffer)
    }
}

impl fmt::Debug for EntropySource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("EntropySource(..)")
    }
}

/// Fill `buffer` from the operating system's cryptographic generator.
pub(crate) fn fill_from_os(buffer: &mut [u8]) -> Result<(), String> {
    os::fill(buffer)
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
mod os {
    pub(super) fn fill(buffer: &mut [u8]) -> Result<(), String> {
        // getentropy accepts at most 256 bytes per call.
        for chunk in buffer.chunks_mut(256) {
            // SAFETY: the pointer and length describe a live, writable slice.
            let status = unsafe { libc::getentropy(chunk.as_mut_ptr().cast(), chunk.len()) };
            if status != 0 {
                return Err(format!(
                    "getentropy failed: {}",
                    std::io::Error::last_os_error()
                ));
            }
        }
        Ok(())
    }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
mod os {
    pub(super) fn fill(buffer: &mut [u8]) -> Result<(), String> {
        let mut filled = 0;
        while filled < buffer.len() {
            let rest = &mut buffer[filled..];
            // SAFETY: the pointer and length describe a live, writable slice.
            let count = unsafe { libc::getrandom(rest.as_mut_ptr().cast(), rest.len(), 0) };
            if count < 0 {
                let error = std::io::Error::last_os_error();
                if error.kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(format!("getrandom failed: {error}"));
            }
            filled += count as usize;
        }
        Ok(())
    }
}

#[cfg(not(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "linux",
    target_os = "android"
)))]
mod os {
    pub(super) fn fill(buffer: &mut [u8]) -> Result<(), String> {
        use std::io::Read;
        std::fs::File::open("/dev/urandom")
            .and_then(|mut file| file.read_exact(buffer))
            .map_err(|error| format!("cannot read /dev/urandom: {error}"))
    }
}
