# Veritas RC42 — dependency provenance

## Verdict

Veritas now produces a source-free, relocatable macOS ARM64 application bundle with a managed Python runtime, locked wheels, an offline cache, and a hash-checked inventory of license metadata and documents for the project's locked Python environment. The released binary built a NumPy/orjson app bundle, which verified and ran twice offline after relocation into a path containing spaces and Unicode. This is a real packaging capability, not evidence that Veritas should yet be recommended over Python generally.

The feature is deliberately an inventory: it preserves distribution-supplied license identifiers and files and reports missing metadata. It does not judge compatibility or legal compliance. The current inventory covers seven project-environment distributions only; it omits pip bundled inside CPython, CPython/Tcl/Tk notices, and uv notices inside app bundles. Native libraries loaded by wheels still need their own closure and notice accounting.

## Verified

- Rust formatting and Clippy: passed.
- Rust workspace tests: 105 passed, 0 failed.
- Python product acceptance suite: 20 passed, 0 failed.
- Native-wheel package integration: NumPy, orjson and requests were present; the bundle relocated and launched twice; license-file and Python-runtime tampering were rejected.
- Shipped RC42 binary: built the native-wheel app bundle. Its seven Python distributions had complete declared license metadata, zero missing-file warnings, and 25 copied license documents: NumPy 17, orjson 3, and one each for certifi, charset-normalizer, idna, requests and urllib3.
- Shipped source-free task-ledger app: package verification and a state-changing command passed.
- Archive SHA-256 verified: `506a354ad9db459a58ec0856540fc7a14bc796c735d23bd23f5f3b910354a743`.
- A one-off trace of 32 Python/native images found two bundled Tcl/Tk libraries, 23 distinct system libraries, and no unresolved direct references after resolving Mach-O image IDs and runpaths. This is reconnaissance only; no automated product check enforces it yet.

The detailed machine-readable evidence is [packaging-rc42.json](</Users/rmichaelthomas/Documents/Codex/2026-10-22/referenced-chatgpt-conversation-this-is-an/outputs/veritas-foundation-rc42/packaging-rc42.json>); the technical checkpoint is [v1.22 dependency provenance](</Users/rmichaelthomas/Documents/Codex/2026-10-22/referenced-chatgpt-conversation-this-is-an/outputs/veritas-foundation-rc42-checkpoint-v1-22.md>).

## Limits

Evidence is for macOS ARM64 only. Package hashes detect accidental corruption but are unsigned and do not authenticate the publisher. The full virtual environment and uv cache are not independently hashed. The shipped app does not yet inventory dynamic-library dependencies or all notices for CPython/pip, Tcl/Tk, and uv. The assistant recommendation gate remains open; this milestone supplies deployment evidence, not comparative proof of superiority.

## Next build target

Turn the Mach-O trace into a repeatable packaging check. Scan the Veritas runtime, uv, CPython, and project wheels; parse image IDs and runpaths; record direct load edges, bundled binary digests, system dependencies, and unresolved references; fail closed on ambiguity; and connect bundled libraries to available notices. Add CPython/pip and uv notices to app bundles. Re-run relocation and offline launch against native-wheel workloads. Then continue toward clean-host packaging and a controlled independent comparison on a program a developer would reach for today.

## Files

- [RC42 macOS ARM64 archive](</Users/rmichaelthomas/Documents/Codex/2026-10-22/referenced-chatgpt-conversation-this-is-an/outputs/veritas-foundation-rc42/veritas-0.1.0-darwin-arm64.tar.gz>)
- [Archive checksum](</Users/rmichaelthomas/Documents/Codex/2026-10-22/referenced-chatgpt-conversation-this-is-an/outputs/veritas-foundation-rc42/veritas-0.1.0-darwin-arm64.tar.gz.sha256>)
- [Machine-readable acceptance evidence](</Users/rmichaelthomas/Documents/Codex/2026-10-22/referenced-chatgpt-conversation-this-is-an/outputs/veritas-foundation-rc42/packaging-rc42.json>)
