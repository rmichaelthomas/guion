# Package-backed analysis

This retained pre-rename prototype transfers a JSON document through Guion's typed Python bridge to `orjson` and NumPy, computes count/total/min/max/mean over a large integer column and configurable string grouping, and atomically writes JSON that another program can consume. Values must be signed 64-bit integers for NumPy array analysis; the total uses Python's arbitrary-precision integers. It exposes the costs of dependency setup, JSON transfer, native wheels, repeated invocation, and same-host relocation. The standard-library Python reference performs the same observable operation.

The current-format managed package gate now generates a 100,000-row input outside the app bundle, synchronizes NumPy/orjson from the repository's warmed cache, packages the source-free app, relocates it to a path with spaces and Unicode, and compares two launches byte-for-byte with the standard-library reference. Its report records the format, runtime/cache/package sizes, hashes, and exact output. Run it after building a feature-enabled release CLI with:

```sh
cargo build --locked --offline --jobs 2 --release -p guion-cli --features python-interop
python3 -B evaluation/completion/performance/verify_managed_package_analysis.py
```

The runner reuses `target/perf-session/managed-python-package` and removes its temporary project, dataset, and relocated bundle on exit. `--warmed-dir` and `--uv` select a preloaded cache and uv executable for another host platform; `--vm-started` records when that run executes inside a VM. Its result and remaining limits are in the [current-format managed analysis report](../../completion/results/real-task-sprint/2026-10-27-current-format-managed-package-analysis.md). The separate smaller native-wheel fixture is documented in the [package delivery report](../../completion/results/real-task-sprint/2026-10-27-current-format-managed-python-package.md). The bridge speed result remains in the [2026-10-27 report](../../completion/results/real-task-sprint/2026-10-27-python-bridge-package-analysis.md); its [timing runner](../../completion/performance/benchmark_python_package_analysis.py) uses an explicit installed interpreter.
