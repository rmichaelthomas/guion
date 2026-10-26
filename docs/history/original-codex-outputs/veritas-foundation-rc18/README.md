# Veritas foundation build — RC18

This is an intermediate macOS ARM64 evaluation package from the approved Veritas foundation plan. It is a working compiler, bytecode VM, CLI, and managed Python bridge. It does not establish that Veritas should be recommended over Python for general work.

## Package

- Archive: `veritas-0.1.0-darwin-arm64.tar.gz`
- SHA-256: `4a83794dfa0854500985e1fcfe08d4423c39a8883de91d47735764e79be6bcc3`
- Verified target: macOS ARM64
- Included: Veritas CLI, version-11 VM/artifact format, language reference and examples, pinned uv 0.13.4 with its MIT/Apache-2.0 notices.

## What changed

Veritas now supports finite IEEE 754 `Float64` literals, function types, arithmetic, comparison, unary negation, JSON conversion with `json_as_float`, versioned artifact serialization, and Python bridge transport. It rejects implicit integer/float mixing, non-finite values and division by zero. The included `float64.vr` example parses JSON, explicitly converts a number, calls a typed function, and prints `5`.

## Verification

- 59 Rust tests passed, including the pinned offline uv integration with managed CPython 3.14.8, `requests==2.32.5`, and compiled `orjson==3.13.0`.
- Rust formatting and Clippy with warnings denied passed.
- The Python evaluation harness passed 2 tests.
- The extracted archive checksum verified; bundled `doctor` and Float64 source example passed.
- A compiled version-11 Float64 artifact ran after its source copy was removed and printed `5`.
- The packaged Python interoperability project synced offline and printed `76` and `python-requests/2.32.5`.

The candidate remains limited: no records, module imports, integer division/floor division/remainder, general `Option`/`Result` matching, HTTP, relocatable Python environments, or other OS targets. Empty-map construction and broad resource accounting also remain open. The checkpoint details the evidence and remaining work: [Veritas checkpoint v1.2](veritas_checkpoint_v1_2_numeric_foundation.md).
