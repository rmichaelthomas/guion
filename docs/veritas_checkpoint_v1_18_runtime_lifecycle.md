# Veritas checkpoint v1.18 — Persistent Python bridge lifecycle stress

## What changed

Added a repeatable author-run workload for one compiled Veritas function that calls `numpy.arange`, sums the returned typed list, checks the exact value and output on every call, and keeps one Runtime instance and Python sidecar alive across the run. It records warmups, burst throughput and p50/p95/p99/max call latency, then separate paced-call latency and scheduler lateness, close time, and use-after-close behavior. The Python runner records host/runtime versions and attempts 50 ms RSS sampling of the Rust benchmark process only; it does not include the Python sidecar.

The 2026-10-22 macOS 26.5.2 arm64 run used Python 3.14.8, NumPy 2.5.4, 512 array elements, 50 warmups, 10,000 burst calls, then 250 calls at a 10 ms requested pace. Every returned sum and printed line matched exactly. Burst p50/p95/p99/max were 0.675/0.775/0.831/0.949 ms, with 1,455.6 calls/s over 6.870 s. Paced invocation p50/p95/p99/max were 0.953/3.846/5.191/8.961 ms, over 2.502 s. Scheduler lateness p50/p95/p99/max was 1.710/2.149/3.135/10.420 ms. Closing took 1.158 ms; a subsequent call returned `R_INSTANCE_CLOSED`.

## Limits and interpretation

This measures the author’s current host, compiler-built workload, and one NumPy conversion shape. It is not an independent language comparison, broad user-task result, or recommendation result. The sandbox refused `ps` process inspection with `Operation not permitted`, so no RSS samples or memory trend are claimed. The benchmark does not capture Python sidecar memory. The 10 ms pacing target is only a harness setting, not a general responsiveness guarantee. The benchmark exercises lifecycle correctness and host-to-Python round trips, not arbitrary application lifetimes or many concurrent Runtime instances.

## Next work

Repeat with a supported memory-observation mechanism on a non-restricted host and longer runs; include Python-sidecar memory. Continue ordinary multi-class projects, installed-product maintenance and clean-machine/platform evidence. Keep the assistant recommendation claim open until independent comparative sessions meet the pre-registered evaluation protocol.
