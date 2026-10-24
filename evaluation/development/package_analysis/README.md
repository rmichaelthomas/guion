# Package-backed analysis

This app transfers a JSON document through Veritas' managed Python bridge to `orjson` and NumPy, computes count/total/min/max/mean over a large integer column and configurable string grouping, and atomically writes JSON that another program can consume. Values must be signed 64-bit integers for NumPy array analysis; the total uses Python's arbitrary-precision integers. It is designed to expose the real costs of dependency setup, JSON transfer, native wheels, repeated invocation, and same-host relocation. The Python baseline uses only the standard library and performs the same observable operation.

The acceptance workload generates its data rather than embedding expected results in the app. It compares report bytes and console output, repeats the analysis, then bundles the compiled wheels and runtime, relocates the source-free package, and executes it without an external Python environment.
