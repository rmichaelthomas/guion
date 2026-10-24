# Veritas checkpoint v1.11: source-level Python keyword calls

## Goal

Let Veritas programs use Python APIs with the named parameters their authors and documentation expect, while keeping declarations typed and checked.

## Delivered

- Source calls to `extern python` declarations accept positional arguments followed by named arguments.
- Keyword names map to declared parameter names. Unknown names, duplicates, duplicate positional-plus-named values, and positional arguments after a keyword produce compiler diagnostics.
- Declared but omitted parameters are passed through as omitted, allowing Python defaults to apply. Python-required parameters that were omitted fail at the Python boundary as `R_PYTHON_CALL`.
- Artifact format 18 adds the verified Python keyword-call instruction. Formats 14 through 17 remain readable; older formats cannot encode the new instruction.
- Added an executable `python_keywords.vr` example and integration coverage for a mixed positional/named call, a defaulted Python parameter, artifact serialization, and invalid calls.

## Verification

- `cargo fmt --all -- --check` passed.
- `cargo test --workspace --offline` passed all 81 Rust tests in the working tree and again after extracting the source archive into a fresh temporary directory.
- `cargo clippy --offline --workspace --all-targets -- -D warnings` passed.
- `python3 -m unittest discover -s evaluation/tests -v` passed both evaluation-harness tests.
- The packaged macOS ARM64 executable reports artifact format 18, and the bundled `python_keywords.vr` example ran through the package with an explicitly configured interpreter and printed `1024`.
- The package doctor reports its `uv` path relative to the bundle. The package archive checksum verified.

## Limits and next work

Named arguments apply to declared Python calls; ordinary Veritas functions and built-ins remain positional. Declarations still cannot express optionality, callbacks, Python object identity, or recoverable `Result`-based bridge errors. Python calls remain opaque effects. Rich NumPy object/array transfers and compiled-wheel compatibility evaluation remain open, as do package portability and independent task-class comparisons.
