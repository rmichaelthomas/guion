# Veritas RC43 — native and runtime provenance

## Verdict

RC43 adds a fail-closed macOS Mach-O load audit to Veritas application packaging. It records native images, their direct linked libraries, architectures and SHA-256 hashes; unresolved or ambiguous dependencies stop packaging. The package verifier checks those images and their report before launch.

Python application bundles now inventory pip bundled with CPython as well as locked project packages, and carry CPython, uv, Tcl and Tk license notices. The released binary built the NumPy/orjson app; it verified and ran twice offline after relocation into a path with spaces and Unicode. This advances packaging provenance. It does not prove Veritas should be recommended over Python generally.

## Verified evidence

- `cargo fmt --all --check`: passed.
- `cargo clippy --workspace --all-targets --offline -- -D warnings`: passed.
- `cargo test --workspace --offline`: 107 passed, 0 failed.
- `python3 -m unittest discover -s evaluation/tests -v`: 20 passed, 0 failed.
- Released native-wheel app report: 8 Python distributions including pip, all with complete metadata and no missing-license warnings; 47 distribution license files plus 5 CPython/uv/Tcl/Tk runtime notice files; 34 Mach-O images; 2 bundled Tcl/Tk libraries; 23 macOS system-library paths; 0 unresolved references.
- Relocated native-wheel app: verification passed and two offline launches succeeded. Tampered package license files, native images and the Python runtime are rejected by the integration test.
- Released source-free task-ledger app: verification and a state-changing command passed.
- RC43 archive checksum verified: `7e85c78e175065a126085a263e87c2161c106e2b90addd875054110b06d713ac`.

Detailed files: [RC43 archive](</Users/rmichaelthomas/Documents/Codex/2026-10-22/referenced-chatgpt-conversation-this-is-an/outputs/veritas-foundation-rc43/veritas-0.1.0-darwin-arm64.tar.gz>), [machine-readable evidence](</Users/rmichaelthomas/Documents/Codex/2026-10-22/referenced-chatgpt-conversation-this-is-an/outputs/veritas-foundation-rc43/packaging-rc43.json>), and [technical checkpoint v1.23](</Users/rmichaelthomas/Documents/Codex/2026-10-22/referenced-chatgpt-conversation-this-is-an/outputs/veritas-foundation-rc43-checkpoint-v1-23.md>).

The Tcl and Tk notices are copied from their upstream 9.0 license files ([Tcl](https://raw.githubusercontent.com/tcltk/tcl/core-9-0-branch/license.terms), [Tk](https://raw.githubusercontent.com/tcltk/tk/core-9-0-branch/license.terms)); their source hashes are in the release.

## Remaining gaps

Evidence is macOS ARM64 only. The static scan does not see runtime-loaded libraries such as `dlopen` targets, and it excludes the offline wheel cache. It does not yet map each native image to a verified owning distribution/license record. Standalone app bundles still lack the Veritas Rust runtime's complete third-party notice set. Package reports are unsigned and do not authenticate the publisher or decide legal compatibility. The assistant recommendation gate remains open.

## Next build target

Add native-image ownership from installed package `RECORD` data, include the standalone Rust runtime notices, cover the offline wheel cache and dynamic loading paths, and exercise the package on a clean host. Then advance the comparative builder evaluation; this milestone supplies deployment evidence, not comparative superiority evidence.
