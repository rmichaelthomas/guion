# Veritas foundation build — RC20

Intermediate macOS ARM64 evaluation package from the approved Veritas foundation plan. It contains a working compiler, versioned bytecode VM, CLI, and managed Python bridge. It does not establish that Veritas should be recommended over Python for general work.

## Package

- Archive: `veritas-0.1.0-darwin-arm64.tar.gz`
- SHA-256: `8b4256bd109f81c96f274a2a2d2120d325f346e554945aa37a2056faa4a9fb1e`
- Verified target: macOS ARM64
- Included: Veritas CLI, version-12 artifact format, language reference and examples, pinned uv 0.13.4 with its MIT/Apache-2.0 notices.

## New capability

Arbitrary-precision `Int` supports `/` truncating toward zero, `//` floor division, and `%` remainder following the divisor's sign. Float64 `/` remains floating-point division. Division by zero reports `R_DIV_ZERO`. The compiler, artifact encoder/decoder, stack verifier, and VM all implement these operators.

## Verification

- 63 Rust tests passed, including 4,096-digit division and pinned offline uv integration with managed CPython 3.14.8, `requests==2.32.5`, and compiled `orjson==3.13.0`.
- Rust formatting and Clippy with warnings denied passed; the Python evaluation harness passed 2 tests.
- The extracted archive checksum verified. `doctor`, both numeric examples, version-12 artifact execution after removing its source, and managed Python sync/run passed.
- The packaged Python project printed `76` and `python-requests/2.32.5`.

The candidate remains limited: no records, module imports, general `Option`/`Result` matching, HTTP, relocatable Python environments, or other OS targets. Empty-map construction and broad resource accounting also remain open. See [checkpoint v1.3](veritas_checkpoint_v1_3_integer_arithmetic.md) for evidence and the continuation plan.
