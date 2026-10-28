# Python bridge raw-byte attachment benchmark — 2026-10-27

## Result

The private raw-byte request path materially reduces the cost of the 100,000-row NumPy/orjson task. Guion's `Bytes` lane was **140.59 ms** median, compared with **161.60 ms** for Guion's `Text` lane. Across 15 randomized paired fresh-process runs, the `Bytes`/`Text` median ratio was **0.863** (95% bootstrap interval **0.859–0.879**), a repeatable 13.7% reduction for this workload.

That brings Guion close to the same package-backed Python function: direct Python `bytes` measured **137.93 ms**, and Guion's `Bytes` lane was **1.025×** as long (95% interval **1.001–1.027**). The interval narrowly remains above parity, so this does not establish that Guion beats Python on the task. The direct Python standard-library reference was **114.38 ms**; Guion `Bytes` was **1.220×** as long (95% interval **1.211–1.234**).

| Lane | Median | Paired comparison |
| --- | ---: | --- |
| Guion `Text` | 161.60 ms | 1.146× direct Python `Text` (95% interval 1.114–1.157) |
| Guion `Bytes` | 140.59 ms | 0.863× Guion `Text` (95% interval 0.859–0.879) |
| Python package `str` | 141.83 ms | Direct NumPy/orjson function |
| Python package `bytes` | 137.93 ms | Guion `Bytes`/Python `bytes`: 1.025× (95% interval 1.001–1.027) |
| Python standard library | 114.38 ms | Guion `Bytes`/stdlib Python: 1.220× (95% interval 1.211–1.234) |

The binary protocol sends request-side Guion `Bytes` as indexed raw attachments beside JSON metadata. It avoids base64 expansion for these inputs while retaining the 24 MiB frame bound. The response path is unchanged: Python byte returns remain base64-encoded JSON. Artifact format 44, source syntax, and the Python function are unchanged.

## Workload and method

- 100,000 rows; 5,100,867-byte input; 13 groups; one typed Python call per Guion run.
- Result files and stdout matched the independent Python standard-library output byte-for-byte in every preflight and timed sample. The result count was 100,000 and the total was 2,500,116,287.
- 15 randomized fresh-process samples per lane, with randomized lane order and paired ratios. The 95% percentile intervals bootstrap the paired per-trial ratios with 20,000 resamples.
- Host: macOS 27.0.1 ARM64. Guion 0.1.0, artifact format 44; CPython 3.14.7, NumPy 2.5.1, and orjson 3.11.9.
- OS file-cache state was uncontrolled. The separate [host-profile note](2026-10-27-python-bridge-host-profile.md) records that this Mac's installed Instruments setup could not provide the requested CPU-stack capture. No CPU attribution or peak-memory claim is made.

An untimed Guion phase profile measured about **0.41 ms** source compilation, **0.10–0.11 ms** runtime loading, and **133.7 ms** program execution for `Bytes`; the `Text` program execution was **151.9 ms**. This shows the difference is in the measured program path, not compiler startup, but does not split Rust bridge work from Python parsing and analysis.

## Reproduction and provenance

The fixed workload and runner are in [`evaluation/development/guion_package_analysis`](../../../development/guion_package_analysis) and [`benchmark_python_bridge_payloads.py`](../../performance/benchmark_python_bridge_payloads.py). The raw samples, all paired ratios, output hashes, phase timings, build hash, and source hashes are in [`2026-10-27-python-bridge-bytes.json`](2026-10-27-python-bridge-bytes.json). The measurement used clean commit `bd66f58d64d905f1aab52601a6bb05d9a03aeeee`; its worktree was clean.

```sh
GUION_PYTHON=/path/to/python3.14 python3 -B evaluation/completion/performance/benchmark_python_bridge_payloads.py \
  --binary target/release/guion \
  --python /path/to/python3.14 \
  --trials 15
```
