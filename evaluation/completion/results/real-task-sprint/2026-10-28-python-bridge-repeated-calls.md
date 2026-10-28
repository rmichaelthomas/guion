# Repeated Python bridge calls — 2026-10-28

## Result

One Python call is close to parity; repeated calls expose a clear per-call cost. Each program read a 64-byte file once, called `payload_length` repeatedly in the same process, and printed the sum. Guion started one persistent Python sidecar for its run. Python imported the same NumPy/orjson packages and called the same function directly.

| Calls in one process | Guion median | Python median | Guion/Python ratio (95% interval) |
| ---: | ---: | ---: | ---: |
| 1 | 79.50 ms | 76.16 ms | 1.046 (0.984–1.068) |
| 10 | 81.00 ms | 76.78 ms | 1.066 (1.035–1.078) |
| 100 | 103.03 ms | 76.67 ms | 1.339 (1.303–1.386) |
| 1,000 | 318.47 ms | 76.94 ms | 4.131 (4.046–4.305) |

All timed outputs matched the expected sum. The single-call comparison does not establish a speed difference. At 1,000 calls, Guion took about **4.1× longer** than the direct Python loop.

Subtracting the one-call time from each paired 1,000-call run estimates the added time per call after process startup. Guion added **0.239 ms per call**; Python added **0.00019 ms per call**, which is below the run-to-run noise. The paired excess was **0.241 ms per added call** (95% bootstrap interval **0.236–0.254 ms**), and all 15 paired differences were positive. This includes Guion loop execution, request serialization, pipe round trip, sidecar dispatch, response parsing, and Python loop cost; it does not yet attribute the time to one component.

Follow-up profiling found that full macOS native-image snapshots consumed 0.220 ms per repeated call. Gating snapshots on observed imports, `ctypes.dlopen`, or changed dyld image count reduced the 1,000-call ratio to 1.481 (1.439–1.559) and excess time per added call to 0.0381 ms (0.0324–0.0442). Guion remains slower than direct Python at 1,000 calls; response transport and reader wakeup are the next measured targets. See the [phase-profile report](2026-10-28-python-bridge-phase-profile.md) and [optimized repeated-call samples](2026-10-28-python-bridge-repeated-calls-after-native-image-gate.json).

## Latest response-path optimization

The bridge now uses `orjson` for repeated request parsing and response encoding only after the Python program has already imported it. Guion adds no package requirement and does not import `orjson` just to accelerate the bridge; it keeps the standard-library path otherwise, and falls back to that path when `orjson` cannot encode a value such as an integer outside its supported range. The benchmark fixture imports `orjson`, so it exercises this fast path after the first request.

Against a 15-pair baseline captured immediately before this change, the latest cohort measured:

| Calls in one process | Baseline ratio | Latest ratio (95% interval) |
| ---: | ---: | ---: |
| 1 | 1.008 | 1.015 (1.002–1.051) |
| 10 | 1.032 | 1.051 (1.018–1.063) |
| 100 | 1.055 | 1.048 (1.034–1.078) |
| 1,000 | 1.513 | 1.405 (1.375–1.454) |

At 1,000 calls, Guion's median fell from **116.67 ms** to **110.21 ms**. The excess-time estimate fell from **38.88 μs** to **30.93 μs per added call**. A same-index cross-cohort bootstrap over 20,000 resamples estimated a **9.84 μs reduction per call** (95% interval **2.50–13.17 μs lower**); 13 of 15 same-index trial differences improved. These are separate timing cohorts on one host, not simultaneous paired runs, so this comparison is evidence for this measured workload rather than a universal language-speed claim.

The bridge remains slower than direct Python at 1,000 calls: the current ratio is **1.405×**. One-call and 100-call results remain near parity. The optional fast path benefits programs that already use `orjson`; programs that do not import it retain the standard-library encoder and parser. The intermediate encoder-only cohort had a 1.90 μs median same-index reduction, but its 95% bootstrap interval crossed zero (**11.93 μs lower to 6.83 μs higher**); adding request parsing produced the clearer combined result above. See the [before samples](2026-10-28-python-bridge-repeated-calls-before-fast-json.json), the [encoder-only experiment](2026-10-28-python-bridge-repeated-calls-after-orjson.json), the [combined fast-path samples](2026-10-28-python-bridge-repeated-calls-after-fast-json.json), and the [latest phase profile](2026-10-28-python-bridge-fast-json-phase-profile.json).

A follow-up vectored-write experiment was reverted: the 1,000-call ratio stayed at 1.406×, and the same-index interval for its estimated per-call change crossed zero. The current code remains at the combined fast-JSON result above; see the [vectored-write samples](2026-10-28-python-bridge-repeated-calls-after-vectored-write.json) and [phase profile](2026-10-28-python-bridge-vectored-write-phase-profile.json).

A typed Rust response envelope with empty optional fields omitted also failed to show a reliable end-to-end gain. Its 30-pair cohort measured 1.392× Guion/Python at 1,000 calls; an independent-cohort comparison estimated 0.8 μs less excess cost per added call, with a 95% interval spanning 3.6 μs more to 4.3 μs less. The change was reverted even though its three-run diagnostic profile measured response JSON parsing at 4.42 μs rather than 5.18 μs. See the [candidate profile](2026-10-28-python-bridge-typed-response-phase-profile.json) and [30-pair samples](2026-10-28-python-bridge-repeated-calls-after-typed-response-30-pair.json).

A single-write request-frame candidate also showed no reliable gain. It retained the same 1.406× Guion/Python ratio as the fast-JSON baseline and changed estimated excess cost by only 0.15 μs in the slower direction (95% interval: 5.63 μs lower to 6.17 μs higher), so it was reverted. A five-run profile locates **12.51 μs** between complete request receipt and Python's first response write. The module-cache fast path removed repeated `importlib` calls and reduced measured dispatch by 0.33 μs; its end-to-end cohort remained near the current baseline. A bounded one-slot response channel did not improve the complete bridge workload, so it was reverted. Rust response JSON parsing at **5.47 μs** is the next focused target. See the [first-byte boundary profile](2026-10-28-python-bridge-first-byte-boundary-profile.json), [module-cache profile](2026-10-28-python-bridge-module-cache-phase-profile.json), and [channel handoff evidence](2026-10-28-python-bridge-channel-handoff-microbenchmark.json).

Replacing the per-call stdout/stderr redirect managers with direct stream save/restore inside `try/finally` produced a small repeated-call ratio improvement. Three 30-pair candidate cohorts measured 1.387×, 1.387×, and 1.379× at 1,000 calls; pooled median ratio was 1.385× versus 1.415× in the preceding 30-pair baseline. The ratio reduction's independent-cohort 95% interval narrowly excluded zero, while the estimated 1.9 μs per-call excess reduction had an interval that included zero. The simpler capture path is retained, with the per-call effect still uncertain. See the [baseline](2026-10-28-python-bridge-repeated-calls-before-manual-capture-30-pair.json), [phase profile](2026-10-28-python-bridge-manual-capture-phase-profile.json), and [candidate cohorts](2026-10-28-python-bridge-repeated-calls-after-manual-capture-30-pair.json), [second](2026-10-28-python-bridge-repeated-calls-after-manual-capture-second-30-pair.json), and [third](2026-10-28-python-bridge-repeated-calls-after-manual-capture-third-30-pair.json).

A typed Rust response parser that preserved the existing sidecar wire format reduced the diagnostic JSON-parse phase from 5.472 μs to 5.254 μs, but did not improve the complete workload: the 30-pair 1,000-call ratio was 1.395× versus 1.389× immediately before the change. Its estimated excess-cost reduction was 0.176 μs per added call, with a 95% interval from 2.326 μs more to 3.301 μs less. The interval includes no change, so the code candidate was reverted. See the [typed-parser profile](2026-10-28-python-bridge-typed-parser-current-phase-profile.json) and [30-pair samples](2026-10-28-python-bridge-repeated-calls-after-typed-parser-30-pair.json).

A borrowed-string/`RawValue` parser candidate did not pass the phase gate: five diagnostic runs measured response parsing at 5.557 μs versus the 5.472 μs baseline, with result decoding at 0.104 μs versus 0.096 μs. It was reverted without an end-to-end timing cohort. See the [borrowed-parser profile](2026-10-28-python-bridge-borrowed-rawvalue-phase-profile.json).

Additional sidecar timings assigned about 2.1 μs per call to request setup, protocol/stream setup, stream restoration, response-object creation, and native-image response preparation. The five-run phase profile left about 2.24 μs unassigned after excluding profile-only serialization. These diagnostic values include timer overhead and do not change the end-to-end benchmark; Rust's first-byte wait remains the larger target at 18.61 μs. See the [control-gap profile](2026-10-28-python-bridge-control-gap-profile.json).

A Unix socket-pair transport candidate passed the 26 Rust bridge integration tests but regressed the repeated-call workload. Its 30-pair 1,000-call ratio was 1.488× versus 1.409× for the fresh pipe baseline; same-index excess cost increased by 3.54 μs per added call (95% interval 0.99–8.91 μs more). It was reverted. See the [pipe baseline](2026-10-28-python-bridge-repeated-calls-before-socketpair-30-pair.json), [socket-pair samples](2026-10-28-python-bridge-repeated-calls-after-socketpair-30-pair.json), and [phase profile](2026-10-28-python-bridge-socketpair-phase-profile.json).

## Method and limits

- Host: macOS ARM64; Guion artifact format 44; CPython 3.14.7, NumPy 2.5.1, and orjson 3.11.9.
- 15 randomized fresh-process pairs at each call count, with eight lanes interleaved per pair. The source was run with Guion's compile cache disabled.
- Guion and Python read the same 64-byte payload once and perform the same number of `len` calls through `payload_length`. Package imports are the same in both processes.
- Every timed stdout and stderr was checked against the exact expected total.
- The incremental estimate compares the added time from 1 to 1,000 calls in each paired run, then bootstraps the median difference using 20,000 samples. It includes the Guion and Python loop costs as well as the bridge path.
- OS file-cache state was uncontrolled. This is one host and wall-clock timing; no host CPU stack attribution or whole-process peak RSS is included.

## Evidence

- [Full 1/10/100/1,000-call raw samples](2026-10-28-python-bridge-repeated-calls-1-1000.json)
- [Baseline immediately before the response-path optimization](2026-10-28-python-bridge-repeated-calls-before-fast-json.json)
- [Combined request-parser and response-encoder results](2026-10-28-python-bridge-repeated-calls-after-fast-json.json)
- [Earlier 1/10/100-call raw samples](2026-10-28-python-bridge-repeated-calls-1-100.json)
- [Repeat-call benchmark and fixture](../../performance/benchmark_python_bridge_repeated_calls.py)
