# Veritas checkpoint v1.11: source-level Python keyword calls

## Goal

Let Veritas programs call Python APIs using named parameters and Python's documented defaults while keeping declared input/output types checked.

## Delivered

- Python external calls accept positional arguments followed by named arguments.
- Names are checked against the typed external declaration. Unknown names, duplicate names, duplicate positional-plus-named values, and positional arguments after a keyword produce compiler diagnostics.
- Omitted values are left out of the Python call so Python defaults apply; if Python requires the missing value, the bridge reports `R_PYTHON_CALL`.
- Artifact format 18 stores Python keyword calls; earlier formats remain readable and cannot encode this instruction.
- Added a runnable `python_keywords.vr` example and success/error integration coverage, including artifact encode/decode.

## Verification

- Formatting, all 81 Rust tests, Clippy with warnings denied, and both Python evaluation harness tests passed.
- All 81 Rust tests passed after extracting the source archive into a fresh temporary directory.
- The packaged macOS ARM64 executable reports format 18 and ran the keyword/default example with the explicit interpreter, printing `1024`.
- The bundled doctor reports `uv` using a package-relative path. Both archive checksum sidecars verified.

## Limits and next work

Keywords are supported on declared Python calls; Veritas functions and built-ins still accept positional arguments. Declarations do not express required/optional parameters, callbacks, live Python objects, or recoverable `Result`-based bridge errors. NumPy arrays and arbitrary object graphs are outside the typed ABI. Package portability, broad host operations, representative sustained workloads, and independent multi-class comparisons are still required before Veritas could merit a general recommendation over Python.
