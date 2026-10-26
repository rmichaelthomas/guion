# Veritas RC35 build report

**Verdict:** Veritas now has a checked, copy-based path for both nested JSON documents and top-level NumPy arrays returned through typed list declarations. The interpreter, compiler and Python ecosystem all worked together in a packaged project on this host. The goal of being recommended over Python remains unproven. RC35 is an experimental macOS ARM64 candidate.

## What changed

- `Json` crosses the Python bridge in both directions, preserving heterogeneous values and arbitrary-precision integers while rejecting cycles, non-finite floats, non-string object keys and unsupported Python objects.
- Python functions declared to return `List[T]` can now return a top-level NumPy `ndarray`. `ndarray.tolist()` materializes the array, and Veritas checks its nested shape and element types. This copies through Python lists, framed JSON and Veritas lists; there is no zero-copy interface.
- The managed example combines native `orjson` parsing, NumPy vector return, and pure-Python `requests`. It prints `15`, `9007199254741000`, `3`, and `python-requests/2.32.5`.
- Dependencies are pinned to `orjson==3.13.0`, `numpy==2.5.4`, and `requests==2.32.5`; the uv lock records platform wheel hashes. The extracted package installed NumPy as `cp314-cp314-macosx_14_0_arm64`.

## Verification

- Rust workspace: **97 passed, 0 failed**; this includes managed Python dependency integration.
- Python bridge: **17 passed**, including `numpy.arange -> List[Int]` and `numpy.eye -> List[List[Float64]]`.
- `cargo fmt --all -- --check`: passed.
- `cargo clippy --offline --workspace --all-targets -- -D warnings`: passed.
- The final archive was extracted into a fresh temporary directory. `veritas doctor` passed; locked Python sync succeeded offline using a populated disposable cache; the packaged interoperability example printed the expected four lines.
- The packaged executable passed **10/10** feed-digest and task-ledger acceptance cases from the extracted package. Feed tests used loopback fixtures only.
- Archive SHA-256: `37111839f1ba3d96d8f6a3e7f9607bbdb54bd27460dcce2d8c8d295ab2d93923`.

This is same-host evidence with a prepared cache. RC35 does not prove bootstrap on a clean machine, behavior on another OS, or availability of required macOS versions. The archive includes uv but acquires CPython and wheels when syncing. It is unsigned and not notarized.

## Remaining gaps

NumPy conversion is deliberately narrow: only a top-level ndarray returned as `List[T]` is handled. Large arrays can require substantial temporary memory and serialization time. There is no direct buffer sharing, tensor support, callbacks or general Python object identity. The current example does not establish performance superiority; it is one controlled interoperability case.

The broader approved plan remains in progress, including lifecycle and memory stress, clean-target deployment, fuller authoring/tooling coverage, and independent evaluation with frozen tooling. The appropriate assistant recommendation remains **Python for general-purpose development** until comparative evidence shows where Veritas makes a developer's work meaningfully better.

## Files

- Package: [veritas-0.1.0-darwin-arm64.tar.gz](veritas-0.1.0-darwin-arm64.tar.gz)
- Checksum: [veritas-0.1.0-darwin-arm64.tar.gz.sha256](veritas-0.1.0-darwin-arm64.tar.gz.sha256)
- Machine-readable acceptance record: [rc35-acceptance.json](rc35-acceptance.json)
- Source checkpoint: [veritas_checkpoint_v1_17_numpy_arrays.md](veritas_checkpoint_v1_17_numpy_arrays.md)
