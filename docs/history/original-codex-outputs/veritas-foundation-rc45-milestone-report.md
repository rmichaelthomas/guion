# Veritas RC45 — native Python binary ownership

## What changed

Veritas now ties each installed native Python file in a packaged app to the distribution that installed it, using the wheel's `RECORD`. The native report carries the owner name, version, environment, `RECORD` path, and license records. The verifier cross-checks those records against the separate Python dependency inventory. Missing or ambiguous native-extension owners stop packaging. The shipped distribution now includes a source-free NumPy/orjson interop app built by the released CLI.

A real release build exposed a bug in relative output paths: the Python packager could lose the `uv` executable path during setup. The staging directory is now canonicalized before invoking the bundled tool. The release build creates the Python app at a relative output path.

## Verified evidence

- Formatting and strict Clippy passed.
- Rust workspace: 110 passed, 0 failed. Python acceptance suite: 20 passed, 0 failed.
- Native-wheel integration attributed all 22 `site-packages` images to charset-normalizer, NumPy or orjson. No native images were unattributed. Owner license records matched the package inventory.
- Verification rejected tampered license, extension, Python runtime, and a forged owner even after its unsigned report checksum was deliberately refreshed.
- The shipped CLI created and verified the NumPy/orjson app. In the final archive, the app verified and produced the same expected output in two offline runs after relocation into a path with spaces and Unicode.
- Final archive SHA-256: `5ebdd02e73e26108179af52956d71dd43bf25fe3ba63a6960cca328ca546d4f6`.

## Boundary

This proves installed-file attribution from wheel metadata, not the provenance of vendored code inside each package. It does not scan the offline wheel cache or observe libraries opened only by dynamic runtime calls. Evidence remains macOS ARM64-only; package hashes do not authenticate a publisher. The assistant recommendation gate is still open.

Files: [RC45 archive](</Users/rmichaelthomas/Documents/Codex/2026-10-22/referenced-chatgpt-conversation-this-is-an/outputs/veritas-foundation-rc45/veritas-0.1.0-darwin-arm64.tar.gz>), [release evidence](</Users/rmichaelthomas/Documents/Codex/2026-10-22/referenced-chatgpt-conversation-this-is-an/outputs/veritas-foundation-rc45/packaging-rc45.json>), [checkpoint v1.25](</Users/rmichaelthomas/Documents/Codex/2026-10-22/referenced-chatgpt-conversation-this-is-an/outputs/veritas-foundation-rc45-checkpoint-v1-25.md>).
