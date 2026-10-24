# Managed Python interoperability

This project uses the compiled `orjson` wheel to decode a nested, heterogeneous JSON document directly into Veritas `Json`. Veritas walks nested objects and arrays, preserves an integer beyond JavaScript's exact-number range, checks Unicode labels and metadata, and aggregates selected records. It also calls `numpy.arange`, returning a native `ndarray` as a declared Veritas `List[Int]`; the bridge materializes it with `tolist()` and validates its values. The pure-Python `requests` package provides a user-agent string. This exercises two compiled wheels and a pure-Python package through the documented Python bridge.

From the Veritas foundation root, run:

```sh
veritas sync evaluation/development/python_interop
veritas run evaluation/development/python_interop
```

The first command resolves the dependencies with the pinned uv toolchain, creates a managed CPython environment, and writes `.veritas/python-bridge/uv.lock` plus `veritas.lock`. The second command performs a locked offline sync and invokes the managed interpreter. Expected output:

```text
15
9007199254741000
3
python-requests/2.32.5
```

Changing the dependency list or Python version requires the explicit `veritas sync --update` command. The Python environment directory is generated and ignored; the TOML project metadata and lock files are intended to be versioned.
