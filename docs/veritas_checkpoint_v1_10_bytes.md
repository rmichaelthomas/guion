# Veritas checkpoint v1.10: byte-preserving values and interop

## Goal

Give binary-oriented programs a typed, lossless path through Veritas file operations and the supported Python bridge.

## Delivered

- Added immutable `Bytes` values with safe display (`Bytes(n)`), equality, byte-count `len`, resource-limit checks, and artifact encoding under format 17.
- Added `read_bytes(path) -> Result[Bytes, FileError]` and `write_bytes(path, content) -> Result[Unit, FileError]`. They share the project's root and `max_file_bytes` policy with text file access.
- Added Python `bytes` conversion using base64 over the existing framed JSON sidecar. The bridge frame ceiling is 24 MiB so a default-limit 16 MiB byte payload fits after encoding.
- Kept format 14, 15, and 16 readable. Byte values and byte host operations are rejected when writing an older artifact version.
- Added a runnable `language/examples/binary_copy.vr` example and an end-to-end acceptance test containing NUL and invalid UTF-8 bytes through artifact serialization, file reading/writing, and Python conversion.

## Verification

Pending final workspace and packaged-product verification for RC27.

## Limits and next work

There is not yet a source byte literal, byte indexing/slicing, a `Byte` scalar type, native C ABI, or relocatable Python runtime/wheel bundle. The sidecar copies and base64-encodes bytes, adding memory and CPU overhead. The 24 MiB frame accommodates payloads up to the default file ceiling; deployments that raise `max_file_bytes` can hit the bridge frame ceiling earlier. Direct filesystem writes can leave partial output on OS failure. These limits are explicit and do not establish a general native FFI contract.

Next approved-plan work remains package lifecycle and portability, broader host services, comparative workloads, and independent multi-class evaluation.
