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

## Method and limits

- Host: macOS ARM64; Guion artifact format 44; CPython 3.14.7, NumPy 2.5.1, and orjson 3.11.9.
- 15 randomized fresh-process pairs at each call count, with eight lanes interleaved per pair. The source was run with Guion's compile cache disabled.
- Guion and Python read the same 64-byte payload once and perform the same number of `len` calls through `payload_length`. Package imports are the same in both processes.
- Every timed stdout and stderr was checked against the exact expected total.
- The incremental estimate compares the added time from 1 to 1,000 calls in each paired run, then bootstraps the median difference using 20,000 samples. It includes the Guion and Python loop costs as well as the bridge path.
- OS file-cache state was uncontrolled. This is one host and wall-clock timing; no host CPU stack attribution or whole-process peak RSS is included.

## Evidence

- [Full 1/10/100/1,000-call raw samples](2026-10-28-python-bridge-repeated-calls-1-1000.json)
- [Earlier 1/10/100-call raw samples](2026-10-28-python-bridge-repeated-calls-1-100.json)
- [Repeat-call benchmark and fixture](../../performance/benchmark_python_bridge_repeated_calls.py)
