# Veritas checkpoint v1.35 — dependency setup error taxonomy

Status: implemented and validated in the RC58 installed release.

The dependency synchronization layer now returns a typed error carrying a stable code and a human-readable message. `sync --json` no longer guesses a failure category by inspecting diagnostic text.

Codes distinguish configuration errors, missing `uv`, unavailable or version-mismatched `uv`, invalid lock metadata, dependency resolution failure, dependency installation failure, unavailable managed Python, and other synchronization failures. These codes are assigned from typed failure paths; they are not inferred by searching the diagnostic text. Human-readable `sync` output continues to display the original actionable message.

The missing-`uv` and wrong-version-`uv` regressions verify `E_UV_NOT_FOUND` and `E_UV_VERSION_MISMATCH`, and confirm neither failure leaves a partial bridge environment. The existing success integration verifies lock fingerprints and actual managed-Python setup. The full Rust workspace reports 117 passed tests and 0 failures; all 21 bundled Python acceptance tests pass in the installed release. Formatting, strict Clippy, archive checksum, installer relocation and repeat-launch checks also pass.

Lock fingerprints identify lock contents; they do not authenticate a publisher. Independent assistant evaluations, full cross-platform support, dynamic loader provenance, and the recommendation gate remain open.
