# Veritas checkpoint v1.46 — packaged lifecycle stress

## Change

Updated the lifecycle probe's RSS sampler to follow the benchmark process tree, including the Python sidecar. It discovers descendants from a process table snapshot every 50 ms, records root/descendant/tree RSS separately, and notes shared-page double counting and missed short peaks. The shipped evaluation tests cover nested child accounting, malformed process rows, and a process that exits before sampling.

## RC69 packaged evidence

Run against a fresh RC69 extraction whose path contained spaces and Unicode, with bundled CPython 3.14.8 and NumPy 2.5.4:

- 50 warmups, 10,000 burst calls, and 250 calls scheduled every 10 ms all returned the exact expected arithmetic sum and output.
- Burst p50/p95/p99/max latency: 0.863/0.980/1.068/1.201 ms; throughput 1,144.983 calls/s.
- Paced call p95: 1.327 ms; p95 scheduling lateness: 4.256 ms.
- Shutdown: 1.153 ms; use after close: `R_INSTANCE_CLOSED`.
- Across 137 samples after the first second, Python sidecar RSS remained 36.375 MiB. Rust process RSS moved from 3.781 MiB to 3.891 MiB; tree RSS moved 112 KiB. Sampled whole-run tree peak: 40.25 MiB.

Raw author-run evidence: `evaluation/results/rc69-packaged-lifecycle-stress.json`. Shared pages are counted for every process, brief peaks can be missed, and a single host/run does not prove a general memory bound or recommendation advantage.

## RC70 repeat

A second fresh extracted-package run used the bundled CPython 3.14.8 and NumPy 2.5.4, with the same 50 warmups, 10,000 burst calls and 250 calls paced at 10 ms. All calls returned the expected result. Burst p50/p95/p99/max was 0.863/0.981/1.073/1.236 ms at 1,144.666 calls/s; paced p50/p95/p99/max was 1.217/1.361/1.434/1.482 ms, with p95 scheduling lateness of 4.032 ms and maximum 5.019 ms. Close took 1.506 ms and use after close returned `R_INSTANCE_CLOSED`.

Across 138 samples after two seconds, the sidecar descendant RSS stayed at 41.969 MiB. The Rust root moved by 128 KiB, and the process-tree RSS moved by 128 KiB. The whole-run sampled tree peak was 45.844 MiB across 164 samples, including interpreter startup. Raw report: `evaluation/results/rc70-packaged-lifecycle-stress.json`.

The steady sidecar figure differed from RC69's 36.375 MiB. These are separate observed runs, not a memory guarantee; host state, interpreter warmup, RSS shared-page counting and sample timing affect the measurement.

## Verification and boundary

The process-tree helpers passed their unit tests. The packaged runner itself was executed with process inspection enabled. The run is on macOS arm64 only and is not independent recommendation evidence.
