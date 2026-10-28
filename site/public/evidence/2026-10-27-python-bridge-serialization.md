# Python bridge serialization pass — 2026-10-27

## Result

The implementation now serializes borrowed Guion values into the framed request and lets the Python sidecar parse its bytearray directly. A same-host, randomized **15-pair** comparison did not establish a speed improvement or a regression on the 100,000-row package-analysis workload: the candidate/pre-change median ratio was **1.012** (95% bootstrap interval **0.985–1.025**), and the candidate was slower in 10 of 15 pairs.

The candidate took **1.129×** as long as the same NumPy/orjson function run directly in Python (95% interval **1.106–1.149**) and **1.406×** as long as the standard-library Python reference (95% interval **1.373–1.442**). All 15 candidate pairs were slower than both Python lanes. The code removes payload-sized intermediate copies by construction, but peak RSS was not measured; no speedup claim is made.

| Fresh-process lane | Median | Paired comparison |
| --- | ---: | ---: |
| Pre-change Guion bridge | 166.7 ms | Baseline |
| Candidate Guion bridge | 167.9 ms | 1.012× vs pre-change (0.985–1.025); inconclusive |
| Direct NumPy/orjson Python | 150.7 ms | Candidate is 1.129× slower (1.106–1.149) |
| Standard-library Python | 119.9 ms | Candidate is 1.406× slower (1.373–1.442) |

Each lane processed the same **100,000 rows** from a **5,100,867-byte** input, produced 13 groups with total 2,500,116,287, and matched the same 1,256-byte report and console output. The input and output checks ran before measurement and on every timed sample.

## What changed

The Rust bridge now serializes borrowed Guion values directly into the request frame instead of first building an owned JSON value tree. This avoids copying large `Text` and `Json` inputs into a second tree before serialization. The Python sidecar now passes its frame bytearray directly to `json.loads`, avoiding an intermediate bytes copy; the JSON parser still performs the necessary UTF-8 decoding. The framed protocol and value tags remain unchanged.

## Payload-size diagnostic

A separate seven-pair size probe used empty, 534,772-byte, and 5,100,867-byte text inputs with the same NumPy/orjson imports and output validation. Its medians and intervals vary by payload size and the full application comparison above remains the primary result. The probe is retained in the raw JSON as diagnostic data, not evidence of a broad speedup.

## Method and verification

The comparison ran on macOS 27.0.1 ARM64 with CPython 3.14.7, NumPy 2.5.1, and orjson 3.11.9. Pre-change and candidate release binaries were interleaved with the same two Python references. Each lane had an output-checked preflight, followed by 15 randomized fresh-process trials. Source caching was disabled. Several lanes had host-scheduling spikes, and OS file-cache state was uncontrolled. The raw per-trial samples, orders, binary hashes, and source hashes are in the [JSON report](2026-10-27-python-bridge-serialization.json).

Verification passed: the full feature-enabled runtime suite completed **221 tests with 0 failures**, the release build with `python-interop` succeeded offline, and `cargo fmt --all -- --check` passed. The test build emitted one existing unused-import warning for `CalendarEvent` in `crates/runtime/tests/stdlib_services.rs`; it did not fail the suite.

The bridge still trails the direct NumPy/orjson lane by about 17.1 ms at the candidate median. Further work needs CPU and memory attribution before selecting another optimization. Package interoperability, this bridge result, and Guion's broader performance remain separate claims.
