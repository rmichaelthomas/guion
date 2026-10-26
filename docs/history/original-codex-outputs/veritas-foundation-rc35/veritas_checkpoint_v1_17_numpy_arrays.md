# Veritas checkpoint v1.17 — NumPy arrays through typed lists

## What changed

A Python call declared to return `List[T]` can now return a top-level NumPy `ndarray`. The sidecar recognizes NumPy's array type and materializes it with `tolist()`. Veritas then validates the resulting nested values against the declared list depth and element type. A 2D array can therefore map to `List[List[Float64]]`; a mismatched shape or value type fails the normal runtime return-type check.

This is explicit, copy-based interoperability. NumPy allocates Python lists, the sidecar serializes the list over its framed channel, and Veritas reconstructs its own list values. The bridge does not share the array's memory, expose strides, or accept arrays inside arbitrary Python objects. It does not turn Veritas into a NumPy API or promise that array transfer is cheap.

The managed `python_interop` project now exercises three packages: compiled `orjson` parses nested heterogeneous JSON into `Json`; compiled `numpy` returns an `ndarray` from `numpy.arange` into `List[Int]`; pure-Python `requests` returns its user-agent string. It prints:

```text
15
9007199254741000
3
python-requests/2.32.5
```

The project pins `numpy==2.5.4` and checks in the updated uv dependency graph and Veritas lock. The matching CPython 3.14 macOS ARM64 wheel is present in that lock with its hash.

## Verification

- The new NumPy bridge test failed first with `R_PYTHON_CALL` because ndarray values were not supported. With top-level list-return materialization, direct calls to `numpy.arange` and `numpy.eye` passed as `List[Int]` and `List[List[Float64]]` respectively.
- The full managed project synced CPython 3.14.8 and the exact three locked dependencies, then ran offline and produced the expected output above.
- The exact packaged candidate is revalidated after the source changes; results and package hash are recorded in the release report.

## Limits and next work

The converter handles only a top-level NumPy `ndarray` when the declared return type is `List[T]`. The `tolist()` step can be expensive and may temporarily hold both Python and Veritas copies. It does not support zero-copy arrays, memory views, tensors, object-dtype arrays containing unsupported values, or arrays nested inside a Python object. Measure large-array transfer and document resource behavior before claiming scientific workloads are competitive.

The candidate remains macOS ARM64 and author-tested. Python-assisted packaging still downloads CPython and wheels on sync. General-purpose use and an assistant recommendation advantage remain unproven.
