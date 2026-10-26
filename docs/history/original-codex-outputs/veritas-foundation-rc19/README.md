# Veritas foundation build — RC19

This is an intermediate macOS ARM64 evaluation package from the approved Veritas foundation plan. It contains a working compiler, versioned bytecode VM, CLI, and managed Python bridge. It does not establish that Veritas should be recommended over Python for general work.

## Package

- Archive: `veritas-0.1.0-darwin-arm64.tar.gz`
- SHA-256: `bf036f75021055d4398c0b161d531716378efe1465d3c29e6286b4f7954937c5`
- Verified target: macOS ARM64
- Included: Veritas CLI, version-12 artifact format, language reference and examples, pinned uv 0.13.4 with its MIT/Apache-2.0 notices.

## What changed

Arbitrary-precision `Int` now supports `/` truncating toward zero, `//` floor division, and `%` remainder following the divisor's sign. Float64 `/` remains floating-point division. Division by zero reports `R_DIV_ZERO`. The compiler, artifact encoder/decoder, stack verifier, and VM all implement the operators.

## Verification

- 62 Rust tests passed, including pinned offline uv integration with managed CPython 3.14.8, `requests==2.32.5`, and compiled `orjson==3.13.0`.
- Rust formatting and Clippy with warnings denied passed; the Python evaluation harness passed 2 tests.
- The extracted archive checksum verified. `doctor`, the integer division example, and the Float64 example passed.
- A compiled version-12 integer division artifact ran after deleting its source copy and returned the same outputs.
- The packaged Python interoperability project synced offline and printed `76` and `python-requests/2.32.5`.

The candidate remains limited: no records, module imports, general `Option`/`Result` matching, HTTP, relocatable Python environments, or other OS targets. Empty-map construction and broad resource accounting also remain open. See [checkpoint v1.3](veritas_checkpoint_v1_3_integer_arithmetic.md) for evidence and the remaining plan.
