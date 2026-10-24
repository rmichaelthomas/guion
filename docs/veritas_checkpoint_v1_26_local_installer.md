# Veritas checkpoint v1.26 — current-host local installer

## Change

The release archive now has `install.sh`. It copies a distribution into a versioned directory under a caller-selected prefix (default `~/.local`), creates a `bin/veritas` launcher, preserves prior version directories when a new version is installed, and refuses to replace an unrelated command. It does not edit shell startup files. The launcher resolves the runtime relative to its own `bin` directory, so spaces and Unicode in the prefix are supported.

Release assembly exercises the installer using a temporary prefix with spaces and Unicode, verifies a bundled Python app through the installed CLI, and runs that app twice offline. This is a local current-host installation path; it does not add signing, notarization or cross-platform installers.

## Evidence

- `cargo fmt --all --check` and strict workspace Clippy passed.
- `cargo test --workspace --offline`: 110 passed, 0 failed. Python acceptance: 20 passed, 0 failed.
- The release installer fixture passed with a Unicode/space prefix, retained distribution notices, preserved arguments, and refused same-version replacement and an unrelated existing command.
- RC46 release assembly installed the app into a temporary prefix, verified the source-free Python interop bundle, and launched it twice offline with identical output.
- The final archive checksum passed. The extracted archive installed from a path containing spaces and Unicode; its CLI verified the bundled app; the installed app launched twice offline with identical output. The installed CLI ran the bundled `sum.vr` example, printing `15` and returning the declared process status `15`.
- The detached SHA-256 sidecar is shipped beside the archive.

## Limits

The installer copies the complete distribution and therefore requires space for a second copy. It retains old version directories and does not currently offer uninstall or garbage collection. The release remains macOS ARM64-only and unsigned.
