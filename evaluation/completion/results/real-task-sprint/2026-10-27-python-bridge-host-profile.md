# Python bridge host profiling availability — 2026-10-27

## Result

This Mac cannot provide the requested unprivileged xctrace CPU-stack profile with its installed Instruments templates. The `Time Profiler` template is absent. A `CPU Counters` preflight failed before recording with a macOS kernel-policy error followed by an Instruments assertion. No trace was captured, and this is not a benchmark failure.

The already output-checked size-scaling cohort provides a useful wall-time diagnostic for the current text-payload bridge. With an empty payload, Guion and the direct NumPy/orjson Python lane are effectively tied. The difference grows with payload size: at 5,100,867 bytes, the Guion median is 105.9 ms versus 85.8 ms for direct Python (1.252×, 95% interval 1.220–1.301). This points toward work that scales with the transferred payload, but wall time cannot distinguish Rust frame serialization from sidecar JSON parsing or other payload handling.

| Input payload | Guion median | Direct Python median | Guion / Python (95% interval) |
| ---: | ---: | ---: | ---: |
| 0 bytes | 81.1 ms | 82.0 ms | 0.990× (0.953–1.000) |
| 534,772 bytes | 85.8 ms | 82.6 ms | 1.031× (1.017–1.101) |
| 5,100,867 bytes | 105.9 ms | 85.8 ms | 1.252× (1.220–1.301) |

These diagnostic ratios come from seven randomized output-checked pairs per size; the separate 15-pair full application report remains the primary comparison. No claim of CPU attribution is made. The next step is to benchmark the existing typed `Bytes` path and, if needed, remove large raw payloads from the JSON envelope.

## Environment record

- Host: macOS 27.0.1, ARM64.
- xctrace listed `Allocations`, `CPU Counters`, `Core AI`, `File Activity`, `Foundation Models`, `Leaks`, `Network`, `Power Profiler`, `Processor Trace`, `Swift Concurrency`, and `SwiftUI`; it did not list `Time Profiler`.
- `xcrun xctrace record --template 'Time Profiler' ...` exited 33 before recording with `Cannot find template matching name: Time Profiler`.
- `xcrun xctrace record --template 'CPU Counters' --show-recording-options` failed before recording with `(kernel) Failed to query kext info (MAC policy error 0x1)` and an Instruments `XRAugmentationManager` assertion.
- The probe program's output was checked byte-for-byte against the Python reference. The attempted xctrace run produced no trace file.
- Raw size-scaling samples and hashes remain in [`2026-10-27-python-bridge-serialization.json`](2026-10-27-python-bridge-serialization.json).
