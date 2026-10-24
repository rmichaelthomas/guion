# Veritas checkpoint v1.16 — JSON values across the Python ABI

## What changed

Veritas Python declarations now accept `Json`. The bridge sends it as a tagged JSON value, and the sidecar turns it into ordinary Python `None`, booleans, integers, finite floats, strings, lists and dictionaries. A function declared to return `Json` gets the inverse validation and returns a native Veritas JSON value. This preserves heterogeneous nested documents without requiring a Veritas signature for every field.

The conversion preserves Python integer precision, including values beyond JavaScript's exact-number range. It rejects cycles, non-string object keys, non-finite floats and Python values outside the JSON data model. Both directions enforce the existing 64-level nesting limit and framed transport limit. JSON is copied through the bridge; it is not a Python object handle or a zero-copy array interface.

The managed interoperability example now calls the compiled `orjson` wheel to parse a nested object with mixed values. Veritas reads nested object and array fields, checks Unicode and a large integer, and aggregates active records. It then calls pure-Python `requests`. The result is:

```text
9007199254741000
3
python-requests/2.32.5
```

## Verification

- The new source-level `Json` round-trip test failed first because the compiler rejected `Json` in Python declarations. After the compiler, runtime, and sidecar changes, the test passed through source compilation, artifact serialization, Python `json.loads`, a second Python call, and Veritas JSON inspection.
- Bridge integration suite: **16 passed**. Added boundary cases reject cycles, non-finite numbers and non-string dictionary keys.
- Managed dependency integration: **2 passed**, including locked offline sync/run of the `orjson` compiled wheel and `requests` package from a disposable test project.
- Full Rust workspace: **96 passed, 0 failed**. Existing loopback network test ran with local-listener permission. The no-network default run initially failed only because the sandbox disallowed that listener; the same test passed with the required local capability.
- `cargo fmt --all -- --check`: passed.
- `cargo clippy --offline --workspace --all-targets -- -D warnings`: passed.
- The development `python_interop` project was synced and run with pinned uv 0.13.4 and CPython 3.14.8. Its locked packages executed offline after sync, and its expected output matched exactly.

## Limits and next work

This closes one large conversion gap, but does not make Python object interoperability universal. `numpy.ndarray`, tensors, tuples, extension classes, callbacks, generators, object identity and zero-copy buffer sharing remain unsupported. JSON conversion can copy large documents and allocate both Python and Veritas representations. The current workload establishes compatibility for one object-heavy `orjson` document, not broad ecosystem coverage or an overall advantage over Python.

The release remains a macOS ARM64 experiment. A clean machine and other targets are not verified; package setup acquires CPython and wheels on the target host. The larger implementation plan remains active, and the assistant recommendation gate remains unpassed.
