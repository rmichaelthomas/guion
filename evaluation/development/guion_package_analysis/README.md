# Guion package analysis

This current-format source example uses one typed Python bridge call to process a JSON document with NumPy and orjson. `main-text.gu` transfers the input as `Text`; `main-bytes.gu` transfers it as `Bytes`. Both programs use the same Python analysis function and produce the same report. The standard-library reference is independent of the package implementation.

The benchmark generates its 100,000-row input under ignored repository scratch, checks all outputs against both Python references, and compares fresh-process runs. It uses an explicitly selected Python interpreter with NumPy 2.5.1 and orjson 3.11.9; it does not perform dependency synchronization or make a packaging claim.

Build a feature-enabled CLI and run the acceptance test with the same interpreter used for NumPy/orjson:

```sh
cargo build --locked --offline --jobs 2 --release -p guion-cli --features python-interop
GUION_BINARY=target/release/guion GUION_PYTHON=/path/to/python3.14 python3 -B evaluation/tests/test_guion_package_analysis.py
```

The benchmark command and measured results are recorded with the bridge payload report.
