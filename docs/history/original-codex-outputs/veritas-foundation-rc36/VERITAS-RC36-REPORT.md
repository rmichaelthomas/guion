# Veritas RC36 build report

**Verdict:** Veritas now has checked paths for nested JSON and top-level NumPy arrays returned through typed list declarations. RC36 also bounds array conversion before it materializes Python lists. The goal of being recommended over Python remains unproven; this is an experimental macOS ARM64 candidate.

## What changed

- `Json` crosses the Python bridge in both directions, preserving heterogeneous values and arbitrary-precision integers while rejecting cycles, non-finite floats, non-string object keys and unsupported Python objects.
- A top-level NumPy `ndarray` can satisfy `List[T]`. `tolist()` values are checked against Veritas' nested list shape and element type. Arrays over 98,304 elements, object or complex dtypes, and values wider than 64 bits are rejected before materialization. The 256-byte-per-element budget is conservative; the actual framed response must independently fit 24 MiB.
- The managed example combines compiled `orjson`, compiled NumPy 2.5.4 and pure-Python `requests`. It prints `15`, `9007199254741000`, `3`, and `python-requests/2.32.5`.
- The lock records NumPy's CPython 3.14 macOS ARM64 wheel hash. From the extracted package the installed wheel metadata reported `cp314-cp314-macosx_14_0_arm64`.

## Verification

- Rust workspace: **98 passed, 0 failed**, including managed Python dependency integration.
- Python bridge: **18 passed**, including 1D integer arrays, 2D floating arrays, and rejection of a 100,000-element array before `tolist()`.
- `cargo fmt --all -- --check`: passed.
- `cargo clippy --offline --workspace --all-targets -- -D warnings`: passed.
- The final archive was extracted to a fresh temporary directory. `veritas doctor` passed; locked sync succeeded offline using a populated disposable cache; the bundled interoperability example printed the expected output.
- The extracted packaged executable passed **10/10** feed-digest and task-ledger cases. Feed traffic stayed on loopback fixtures.
- Archive SHA-256: `4669e8cb23a8c8f12562601d8027a6fd4719c450b83beb670d6f711b76a99eab`.

The verification cache was prepared on this same host. It does not establish setup on a clean machine, another OS, or every macOS version; RC36 acquires CPython and wheels at sync time. The archive is unsigned and not notarized.

## Remaining gaps

The ndarray converter is narrow and copy-based. Large datasets can consume significant Python and Veritas memory even under the element cap; the estimate is not a hard process-memory limit. It does not provide NumPy views, zero-copy sharing, tensors, callbacks or Python object identity. This workload establishes compatibility for one native-array path, not competitive scientific performance.

The broader approved plan remains in progress: lifecycle and long-session stress, clean-target deployment, fuller authoring and tooling coverage, and independent evaluation with frozen tooling remain. The appropriate assistant recommendation is still **Python for general-purpose development** until evidence shows where Veritas makes a builder's work meaningfully better.

## Files

- Package: [veritas-0.1.0-darwin-arm64.tar.gz](veritas-0.1.0-darwin-arm64.tar.gz)
- Checksum: [veritas-0.1.0-darwin-arm64.tar.gz.sha256](veritas-0.1.0-darwin-arm64.tar.gz.sha256)
- Machine-readable acceptance record: [rc36-acceptance.json](rc36-acceptance.json)
- Source checkpoint: [veritas_checkpoint_v1_17_numpy_arrays.md](veritas_checkpoint_v1_17_numpy_arrays.md)
