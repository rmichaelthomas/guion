# Veritas RC27 milestone report

**Date:** October 22, 2026  
**Status:** Foundation build continues; this milestone does not establish that an assistant should recommend Veritas over Python.  
**Build root:** `work/veritas-foundation/`

## What changed

RC27 adds an immutable `Bytes` value and advances the artifact format to 17. Veritas source can read and write arbitrary file bytes under the existing project-root and file-size policy. `len(Bytes)` counts bytes. Python declarations can accept and return Python `bytes`; the framed bridge uses base64 so NUL and invalid UTF-8 do not get corrupted by JSON text encoding.

Artifacts in formats 14, 15, and 16 remain readable. A format below 17 cannot encode byte values or byte host operations. The Python bridge frame ceiling is now 24 MiB, which accommodates base64 expansion for the default 16 MiB file limit. Raising `max_file_bytes` above the default can exceed that transport ceiling.

Package validation exposed a stale path in the bundled doctor report. The packaging script now generates that report from the final package directory and records the bundled uv path relative to the bundle.

## Evidence

- The acceptance test preserves a payload containing NUL and invalid UTF-8 through compiler, format-17 artifact encode/decode, `read_bytes`, Python `bytes` conversion, and `write_bytes`.
- Runtime tests cover byte copy and the configured file-size ceiling. Model coverage verifies format-17 byte round-trip, rejects byte features in older-format writes, and retains format-15 and format-14 compatibility.
- `cargo test --workspace --offline` passed all **78 Rust tests** in the working tree and again after extracting the source archive into a fresh temporary directory.
- `cargo fmt --all -- --check`, `cargo clippy --offline --workspace --all-targets -- -D warnings`, and `python3 -m unittest discover -s evaluation/tests -v` passed. The Python evaluation harness contains 2 tests.
- The extracted macOS ARM64 bundle's `doctor.txt` points to the bundle-relative `toolchains/uv`. Its binary-copy example printed `7`; building and inspecting the example showed format 17, then running the artifact and comparing the copied file confirmed byte-for-byte equality.
- Both archives' SHA-256 sidecars verified successfully.

## Artifacts

| Deliverable | SHA-256 |
|---|---|
| `veritas-0.1.0-darwin-arm64.tar.gz` | `71ae7bea99e43b8420cbac42c408ff838117b6059601d6687abb890d28e93507` |
| `veritas-foundation-source-rc27.tar.gz` | `d532c882370259e1dbc691717d01dd9fbe2e0753ed18313e67605fb3820a11d5` |

The bundle targets macOS ARM64 and is unsigned. It does not contain a relocatable CPython or wheel environment.

## Boundaries and next work

Bytes currently enter through file reads or Python calls; there is no source byte literal, indexing/slicing, or `Byte` scalar type. The Python path copies and base64-encodes data, so it is lossless but not a zero-copy or high-throughput native interface. No native C/FFI contract or implementation has been added. File writes remain direct writes and can leave partial output on an operating-system error. Allocation accounting remains an estimate, not an operating-system memory boundary.

The broader approved build remains open: package/dependency portability, remaining host operations, sustained representative workloads, and independent multi-class comparisons against Python with equally capable tooling. The existing sales-report measurement is a known development case and is not new RC27 evidence. Veritas has not passed the recommendation goal yet.
