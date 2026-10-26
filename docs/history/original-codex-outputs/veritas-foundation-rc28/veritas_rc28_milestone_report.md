# Veritas RC28 milestone report

**Date:** October 22, 2026  
**Status:** Foundation build continues. This milestone does not establish that an assistant should recommend Veritas over Python.  
**Build root:** `work/veritas-foundation/`

## What changed

RC28 adds source-level keyword arguments for declared Python functions and advances the bytecode artifact format to 18. Authors can mix positional and named values in Python calls, with compile-time checks for declared parameter names, value types, duplicates, and argument order. Omitted arguments stay omitted across the bridge, allowing Python's defaults to apply. If a required Python argument is omitted, the Python exception is returned as `R_PYTHON_CALL`.

The source example calls `builtins.pow(2, exp=10)` and lets Python use its default `mod=None`. The package exposes `VERITAS_PYTHON` as an explicit interpreter choice and does not guess between `python` and `python3`.

## Evidence

- `cargo fmt --all -- --check` passed.
- `cargo test --workspace --offline` passed all **81 Rust tests** in the working tree and after fresh extraction of the source archive.
- `cargo clippy --offline --workspace --all-targets -- -D warnings` passed.
- `python3 -m unittest discover -s evaluation/tests -v` passed both evaluation harness tests.
- The packaged macOS ARM64 `veritas` executable ran `language/examples/python_keywords.vr` with the explicitly selected interpreter and printed `1024`; inspection reported format 18.
- The packaged doctor path resolves `toolchains/uv` relative to the bundle. Both archive SHA-256 sidecars verified.

## Artifacts

| Deliverable | SHA-256 |
|---|---|
| `veritas-0.1.0-darwin-arm64.tar.gz` | `bb58870bcc75c73f773b23249f794b970911c6dcfb4cebc8ade7128e2c354f77` |
| `veritas-foundation-source-rc28.tar.gz` | `4a21ac83521e47e0e0d721ed022f51d5353623d4d822b1d8e10cc0bdb0216d98` |

The runnable bundle targets macOS ARM64 and is unsigned. Its uv resolver is bundled; CPython and project wheels are acquired separately on first dependency sync, so this does not demonstrate a fully relocatable, offline Python runtime.

## Boundaries and next proof

This milestone improves access to Python APIs but does not prove broad Python replacement value. The ABI still cannot transfer NumPy arrays, tensors, arbitrary Python objects, or live handles. Bridge calls use framed JSON with copying and have runtime-error semantics rather than recoverable `Result` values. Veritas source keyword arguments are currently Python-specific. The package is only built for this host, and a standalone local binary is not yet a complete portable application bundle.

The next north-star proof should focus on a representative end-to-end builder task that exercises real third-party Python ecosystem APIs, data transfer costs, concurrent I/O or CPU work, packaging, and repeatable setup. It should measure setup steps, task completion, runtime, memory, and failure behavior against an equivalently equipped Python project. Existing sales-report measurements are development evidence only; this milestone does not change or re-label them.
