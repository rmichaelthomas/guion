# Python bridge package analysis — 2026-10-27

## Result

The current Guion bridge can call a real native-wheel package stack: NumPy 2.5.1 and orjson 3.11.9 under CPython 3.14.7. The Guion app processed 100,000 JSON rows, grouped them into 13 categories, and matched the independent Python report bytes and console output on every run.

This confirms useful package access for this workload. It is not a speed win, and it does not close managed dependency installation or package relocation.

## Timing comparison

Seven randomized fresh-process pairs were run in three lanes. Guion source compilation was bypassed from cache on every Guion run. Input generation, preflight, and output checking were outside the timer; every timed output was checked against the same exact report. The operating-system file cache was uncontrolled.

| Lane | Median | Paired ratio (95% interval) |
| --- | ---: | ---: |
| Guion calling NumPy/orjson through its Python bridge | 173.5 ms | 1.187× vs direct Python using the same packages (1.162–1.262); 1.482× vs standard-library Python (1.458–1.503) |
| Direct Python calling the same NumPy/orjson analysis function | 149.3 ms | 1.255× vs standard-library Python (1.195–1.284) |
| Standard-library Python reference | 119.0 ms | baseline |

Each interval is a bootstrap percentile interval for the median of paired time ratios. All seven pairs were slower for Guion than both Python lanes. One of the seven Guion samples took 454 ms; the complete samples are retained in the [raw report](2026-10-27-python-bridge-package-analysis.json). Do not read these seven trials as a tail-latency guarantee.

An untimed, output-checked phase profile attributed about **0.40 ms** to source compilation, **0.10 ms** to runtime loading, and **156.6 ms** to program execution. Compilation is not the source of this gap. The current phase report does not separate protocol serialization, sidecar startup/import, or Python function time.

## Bridge boundary and evidence

Each Guion invocation made one typed Python call. It sent a 5,100,867-byte JSON document as `Text` plus the three-byte grouping key and returned a 1,256-byte `Json` report. These are semantic payload sizes; exact framed protocol bytes are not yet exposed by the runtime. The generated dataset was removed after the run; its SHA-256 is in the raw report.

The direct-Python control imports and calls the same NumPy/orjson function without Guion, which separates package work from the additional Guion/sidecar cost. On this task the bridge adds about **19%** over that matched package implementation. The standard-library implementation is faster than the NumPy/orjson implementation for this particular operation.

Passing a parsed `Json` object instead of `Text` was also checked. That path took **445 ms** median versus Python's **112 ms** median, or **3.94× slower** (95% interval 3.86–4.12), despite matching output. The measurements are in the [typed-Json probe](2026-10-27-python-bridge-json-input-probe.json). The experiment was not adopted because it adds Guion parsing and recursive bridge conversion.

## Build correction

Enabling the optional `python-interop` feature exposed a stale `Arc<str>` conversion in the bridge after shared text storage changed. The bridge now serializes the shared text through `&str` and converts returned strings back into shared storage. A regression check round-trips Unicode text. The focused bridge suite passes **22/22** tests, the feature-enabled release build succeeds, and the default Python-free release build was restored afterward.

## What this does and does not establish

- **Established:** one current-format Guion program can use a pure application module plus native NumPy/orjson wheels through one typed bridge call and preserve exact output on a 100,000-row document.
- **Not established:** pinned managed dependency sync, bundled Python, source-free package assembly, same-host relocation, another target, or a clean-machine install. This run used an explicit system Python and its already-installed wheels because the repository environment has no `uv` executable.
- **Still open:** bridge protocol/sidecar cost, broader package coverage, and performance for applications that need both Guion and Python libraries. The current app is slower than both Python comparisons.

The [reproducible runner](../../performance/benchmark_python_package_analysis.py) creates data only under ignored repository scratch and removes it when finished.
