# Richards runtime profile and measured optimization

**Result:** the small-list runtime change improved Richards source execution in a randomized paired comparison against the merged baseline. The fresh-process median was 5.2% lower; the repeated-run median was 14.1% lower. All five established short-source gates still pass.

## What changed

Profiling showed 84,497 list copy-on-write operations per Richards execution, with 66.19 MB of estimated list-copy bytes. Lists created or copied inside native code with up to eight integer or Float64 items now keep their elements inside stable boxed descriptors. Small copy-on-write updates stay inline; larger lists continue using external vectors. Record results materialize inline lists correctly while preserving aliases and guest budget behavior. Native list-set operation counts now exclude invalid indices, matching the VM.

## Paired Richards results

Each lane used 15 randomized baseline/candidate pairs. Every execution was checked against `9297` and `23246`. Profiling was disabled during these timed samples; the counter profile came from separate untimed runs. The repeated lane runs the source 25 times in one process and reports time per logical execution. Ratios are candidate time divided by baseline time; below 1.0 means the candidate took less time.

| Lane | Baseline median | Candidate median | Paired ratio (95% interval) | Candidate lower in |
|---|---:|---:|---:|---:|
| Fresh uncached source | 51.43 ms | 48.18 ms | 0.948 (0.937–0.955) | 14/15 pairs |
| Repeated 25-run source | 14.43 ms | 12.63 ms | 0.859 (0.836–0.912) | 14/15 pairs |

Both confidence intervals stay below 1.0. The gain is larger in repeated execution, where runtime work dominates launch and compilation.

## What the profile says

One untimed candidate execution counted 247,240 successful estimated guest allocation charges (85.75 MB), 294,457 list-set operations, 84,497 copy-on-write copies (66.19 MB estimated copy bytes), 209,960 in-place updates, 162,722 record charges (19.55 MB estimated), and 1,555,377 dynamic guest conditional branches.

These counters establish that Richards performs substantial list copying. They do not identify host allocator calls, prove allocator CPU time is the bottleneck, or measure hardware branch misses. Each native list descriptor carries eight inline slots even when empty or externally stored; peak memory and the memory impact of that fixed storage were not measured. The benchmark also leaves the OS file cache uncontrolled, so it is not a cold-disk result.

## Other speed gates

The final candidate passed the five 21-pair first-source comparisons against Python: tiny (ratio 0.222), arithmetic (0.249), JSON file (0.196), imported helper (0.242), and SQLite/TOML (0.225). Each 95% interval stayed below 1.0. These are first-source results for five small tasks on this Mac, not proof of general Python superiority.

## Verification and limits

- Runtime unit, execution, data-library, and service-library targets: **154 passed**.
- CLI unit tests: **14 passed**.
- Performance harness tests: **28 passed**; first-source harness tests: **5 passed**.
- Formatting and strict Clippy: **passed**.
- The local HTTP fixture tests and nine-application acceptance run were blocked when the OS denied local loopback server binding. One lifecycle test was blocked by process-metric access. No elevation or repeat attempt was made.

Machine: macOS-27.0.1-arm64-arm-64bit-Mach-O (arm64); Python 3.14.7. Guion source-cache bypass was used; the OS file cache was uncontrolled. Binary hashes, paired samples, source-diff hash, profile details, and per-task gates are in [`2026-10-26-richards-profile.json`](2026-10-26-richards-profile.json).
