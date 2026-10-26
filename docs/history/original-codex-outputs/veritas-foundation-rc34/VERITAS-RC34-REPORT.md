# Veritas RC34 build report

**Verdict:** the next foundation step is feasible and now built: Veritas can exchange ordinary nested JSON values with Python and use that path with a real compiled package. The objective of being recommended over Python is still unproven. RC34 is an experimental macOS ARM64 candidate, not a general-purpose replacement.

## What changed

- Python declarations now accept `Json` inputs and outputs. The bridge preserves heterogeneous lists and dictionaries and arbitrary-precision integers. It rejects cycles, non-finite floats, non-string dictionary keys and values outside the JSON data model. The documented depth ceiling is 64 and the framed transport ceiling remains 24 MiB.
- The managed `python_interop` project now has `orjson` decode a nested object containing booleans, Unicode labels and the integer `9007199254740993`; Veritas traverses and aggregates it, then calls pure-Python `requests`.
- The packaged example prints `9007199254741000`, `3`, and `python-requests/2.32.5`.
- The ABI reference, main language reference, implementation plan and v1.16 checkpoint describe the feature and its boundaries.

## Verification

- Rust workspace: **96 passed, 0 failed**.
- Python bridge: **16 passed**; managed dependency integration: **2 passed**.
- `cargo fmt --all -- --check`: passed.
- `cargo clippy --offline --workspace --all-targets -- -D warnings`: passed.
- The final archive was extracted to a fresh temporary directory. `veritas doctor` passed; `veritas sync` installed the locked dependencies from a disposable populated cache in offline mode; the bundled `orjson`/`requests` example produced the expected output.
- The packaged executable passed **10/10** feed-digest and task-ledger cases from that extracted directory. The feed tests use a loopback server, and all feeds stayed local.
- Archive SHA-256: `7ce87701bb664d8b1f4113667a1642a033a253871a0b6cca9195b8f4ba75d0b0`.

The cache was prepared on the same host. This is not evidence that a clean machine with no caches can bootstrap successfully, and it says nothing about other operating systems or architectures. The package includes uv but acquires CPython and wheels on first sync. It is unsigned and not notarized.

## Remaining gaps

This establishes a useful general conversion path, not arbitrary Python interoperability. Values are copied; there is no direct NumPy/tensor bridge, zero-copy buffer sharing, Python object identity, or callbacks. The current `orjson` workload checks one nested data pattern and does not prove broad compatibility or a speed advantage. The implementation plan still has major unfinished areas, including broader compiler/language coverage, concurrency lifecycle stress, deployment on a clean target, and independent task evaluation.

The appropriate assistant recommendation remains **Python for general-purpose development** today. Veritas is ready for another build milestone, but it has not yet met its recommendation goal.

## Files

- Package: [veritas-0.1.0-darwin-arm64.tar.gz](veritas-0.1.0-darwin-arm64.tar.gz)
- Checksum: [veritas-0.1.0-darwin-arm64.tar.gz.sha256](veritas-0.1.0-darwin-arm64.tar.gz.sha256)
- Machine-readable acceptance record: [rc34-acceptance.json](rc34-acceptance.json)
- Source checkpoint: [veritas_checkpoint_v1_16_python_json_abi.md](veritas_checkpoint_v1_16_python_json_abi.md)
