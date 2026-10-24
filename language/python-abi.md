# Veritas Python bridge ABI

Status: typed declarations, positional and keyword source calls, omission of Python-defaulted parameters, the framed sidecar, `Unit`, `Bool`, `Int`, finite `Float64`, `Text`, `Bytes`, `Json`, nested `List` and string-keyed `Map` conversions, top-level NumPy array materialization for declared list returns, timeout supervision, explicit-interpreter calls, and a managed project dependency workflow are implemented. The current-host installer distribution pins uv 0.13.4; project sync selects an exact CPython patch version, creates a managed environment, preserves uv's native lock, and writes a Veritas metadata lock. Python-assisted app bundles copy the managed CPython runtime and locked environment for same-target, offline launch. `Option` and `Result` conversions remain unimplemented.

## Declaration shape

The current source form is:

```veritas
extern python "math:factorial" fn factorial(value: Int) -> Int
```

The declaration names a module importable from the managed Python environment or the project root, and a callable plus a Veritas signature. The bridge runs CPython in isolated mode and explicitly adds only the project root to the import path; installed packages still come from the locked environment. Calls are statically checked against that signature, then dynamically validated at the bridge. Declarations describe expected input and output; they do not prove the Python callable is pure, safe, or actually honors the declared types. In the current build, import, call, and conversion failures become source-traced runtime errors. Recoverable bridge errors represented as `Result` remain planned. Python modules are trusted native host code, not sandboxed guest code.

Source calls to declared Python functions also support keyword arguments. The declared parameter names are the Python keyword names. Positional arguments fill parameters from left to right; named arguments may fill any remaining declared parameter. Names are checked at compile time, and duplicate/unknown names or positional arguments after a keyword produce diagnostics. A declaration lists supported types but does not mark a parameter as required or optional: omitted arguments are passed through as omitted, so Python defaults work; a missing required Python argument becomes a normal `R_PYTHON_CALL` error.

The initial ABI does not support callbacks into Veritas, generators, async Python functions, arbitrary Python object identity, context managers, or returning live handles.

```veritas
extern python "builtins:pow" fn power(base: Int, exp: Int, mod: Int) -> Int

fn main() -> Int:
    return power(2, exp=10)  # Python's default for mod applies
```

## Framed transport

The runtime supervises one explicitly selected CPython child per loaded project instance. The control channel uses four-byte unsigned big-endian frame lengths followed by UTF-8 JSON. The current protocol version is `1`; the maximum frame is 24 MiB to accommodate base64-encoded byte values up to the default 16 MiB file ceiling, and the maximum nesting depth is 64. Every request and response has a monotonically increasing request ID. Frames contain one complete value; trailing bytes and unknown required fields are protocol errors.

The sidecar duplicates its framed output descriptor, then redirects native OS-level writes to stdout/stderr away from the protocol pipe; this prevents native writes from corrupting frames but currently drops native-level output. Python-level `print` output is captured and returned as bounded log fields. Veritas stdout remains owned by the Veritas host. A request timeout terminates and reaps the child. EOF, invalid JSON, mismatched request IDs, oversized frames, and a child crash produce distinct bridge errors. A timeout can occur after Python has caused external effects; the runtime does not claim rollback.

## Value conversion

| Veritas value | Wire form | Python value | Return conversion |
|---|---|---|---|
| `Unit` | `{"t":"unit"}` | `None` | `None` to `Unit` |
| `Bool` | `{"t":"bool","v":true}` | `bool` | Exact bool only; Python bool is not accepted as integer |
| `Int` | `{"t":"int","v":"-123..."}` | arbitrary precision `int` | Integer encoded as decimal text; exact, digit-limited |
| `Float64` | `{"t":"f64","v":"-0.125"}` | finite Python `float` | Round-trippable decimal representation of the IEEE 754 binary64 value; NaN and infinities are rejected |
| `Text` | `{"t":"text","v":"..."}` | `str` | UTF-8, bounded by configured text limit |
| `Bytes` | `{"t":"bytes","v":"<base64>"}` | `bytes` | Base64 preserves every byte; bounded by configured file-byte limit |
| `List[T]` | `{"t":"list","v":[...]}` | `list`; a top-level NumPy `ndarray` return is first materialized as nested Python lists | Recursively validated against `T`; nested array dimensions must match nested `List` declarations |
| `Map[Text, T]` | `{"t":"map","v":{"key":...}}` | `dict[str, T]` | String keys and values recursively validated against `T`; key order is canonicalized by Veritas |
| `Json` | `{"t":"json","v":<JSON value>}` | JSON-compatible Python value | `None`, exact `bool`/`int`/`str`, finite `float`, exact `list`, and exact `dict` with string keys; returned recursively as Veritas `Json` |

`Json` preserves heterogeneous arrays and objects without requiring callers to declare their internal shape. Python integers remain exact, including values beyond binary64 precision; Python floating-point values must be finite. The bridge rejects cycles, non-string object keys, tuples, subclasses and other non-JSON Python objects. JSON conversion is limited to 64 nested levels and to the existing 24 MiB framed transport ceiling. Nested `Json` is one value and is not coerced into a typed `List` or `Map`.

When a Python function is declared to return `List[T]` (including nested lists), a top-level NumPy `ndarray` with boolean or numeric elements up to 64 bits is converted with its `tolist()` method and validated against the declared Veritas shape and element type. Object, complex and wider-than-64-bit dtypes are rejected. Before materializing, the sidecar rejects arrays with more than 98,304 elements; this uses a conservative 256-byte per-element temporary-object budget against the 24 MiB frame ceiling. The final encoded frame must also fit the 24 MiB limit. This allocates Python lists and then serializes the values through the framed channel; it is a convenience conversion with copy and transfer costs, not a zero-copy array interface. Arrays embedded inside unrelated Python objects are not recursively adapted.

`Unit`, `Bool`, `Int`, finite `Float64`, `Text`, `Bytes`, `Json`, and recursively nested `List[T]` and `Map[Text, T]` values currently cross source-level calls. Python dictionaries used as typed `Map` values must have string keys. Python exceptions become `R_PYTHON_CALL` runtime errors with exception type, bounded message and Python traceback; a future bridge surface will represent recoverable errors as `Result`.

`Any` is not a public Veritas type in this ABI. Cycles, tuples, sets, arbitrary classes, file handles, NumPy arrays returned against types other than `List[T]`, tensors, decimal objects, and extension-defined values are rejected unless a documented adapter maps them into the table. `Json` is a data interchange shape, not zero-copy access to Python objects or numeric arrays. Large arrays are copied into Python lists, serialized, and reconstructed as Veritas lists; adapter and transfer costs are included in comparisons.

## Error records

Bridge errors carry a stable Veritas code, Python exception class, bounded message, bounded formatted traceback, module/call name, and request ID. The Python traceback is diagnostic text, not a Veritas source trace. Error messages and logs are treated as untrusted data. Secret values are not included in automatic argument dumps.

## Environment and locking

Projects can declare managed Python dependencies in `veritas.toml`:

```toml
[python]
version = "3.14.8"
dependencies = ["requests==2.32.5", "orjson==3.13.0", "numpy==2.5.4"]
```

`veritas sync <project>` creates a separate uv project under `.veritas/python-bridge`, retains uv's complete `uv.lock`, and writes `veritas.lock` with the Veritas manifest digest, uv and CPython versions, target, and uv-lock digest. The checked-in `uv.lock` retains the full resolved graph, artifact hashes, markers, indexes, and wheel identities; Veritas does not parse or rewrite uv's lock format. A first sync resolves dependencies; later syncs check the lock. `veritas sync --update <project>` explicitly changes the resolution. A run verifies both lock digests, syncs offline with uv's locked mode, verifies the managed interpreter's exact version, and starts the sidecar with that interpreter. A stale or missing lock fails with repair guidance. The per-project `.venv` is generated and should not be committed.

The current-host installer distribution pins uv 0.13.4 separately for macOS ARM64 and GNU/Linux ARM64. During development, `veritas sync` uses uv to acquire the exact managed CPython version and locked dependencies. When `veritas package` builds a Python-assisted app from a synced project, the bundle includes managed CPython, the locked bridge environment, required wheels, and the immutable offline cache; its launcher verifies recorded hashes before each run. Relocated offline launch has been verified on macOS ARM64 and Debian 12-compatible GNU/Linux ARM64 for selected packages such as NumPy and `orjson`. Arbitrary native wheels, other Linux distributions, libc implementations, and architectures remain unverified. Local source builds use the bundled resolver when present, otherwise `VERITAS_UV` or a matching uv on `PATH`. Projects without `[python]` can still use the explicit interpreter path supplied through `VERITAS_PYTHON` or the embedding `HostPolicy`; Veritas never searches for `python` or `python3` by itself.

Pure-Python and native-wheel behavior are tested separately. A compiled extension may depend on system libraries or platform details that prevent relocation. The package command must list those dependencies or reject the bundle; a working local import alone is not portable-package evidence.
