# Richards runtime follow-up — 2026-10-26

**Current fresh-source result:** In a randomized, output-checked cohort of 15 runs per arm on macOS ARM64, Guion took a median **46.00 ms** and CPython 3.14.7 took **67.43 ms**. Guion used 68.5% of Python's time (candidate/Python ratio **0.685**, 95% interval **0.673–0.690**). In the same cohort, the current Guion build was 3.9% faster by median than the previous Guion build (candidate/baseline **0.961**, interval **0.933–0.970**).

## Comparison with the earlier Guion build

The earlier separate Richards comparison reported **5.2% lower fresh-source latency** and **14.1% lower per-run latency** in a 25-repeat process. Those are retained as results from that earlier cohort. The follow-up reran a fresh-source three-arm comparison against CPython and the previous Guion build; it did not rerun the 25-repeat speed comparison. Its direct same-cohort fresh comparison measured a 3.9% median reduction, with the interval shown above. See the preserved earlier report inside [`2026-10-26-richards-profile.json`](2026-10-26-richards-profile.json).

Each current A1 arm used 15 randomized samples and two warmups; every run's output was checked against `9297` and `23246`. Guion's source cache was bypassed with `--no-cache`; OS file-cache conditions were uncontrolled. Timed runs had profiling disabled.

## Peak memory

Five untimed runs per binary in each lane used `resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss`. Every run's program output was checked.

| Lane | Previous build median | Current build median | Current minus previous |
|---|---:|---:|---:|
| Fresh uncached source | 31,211,520 B (29.75 MiB) | 30,097,408 B (28.70 MiB) | −1,114,112 B (−3.57%) |
| 25 executions in one process | 33,898,496 B (32.33 MiB) | 31,080,448 B (29.64 MiB) | −2,818,048 B (−8.31%) |

Neither lane exceeded the 5% regression threshold. The machine-readable report contains all samples, commands, binary hashes, and revisions.

## Remaining cost

The opt-in runtime profile counted **84,497** copy-on-write list copies in one Richards execution and **2,112,425** in the 25-execution lane. Every copy in both profiles had 0–8 elements, accounting for 100% of copy count and 100% of estimated copy bytes. Those list-copy estimates were 77.2% of the estimated guest allocation-budget charges. These counters measure guest operations and estimated bytes; they are not host allocator calls or CPU-time measurements.

The 25-execution profile also counted 38,884,476 dynamic guest conditional branches and 6,181,179 successful estimated guest allocation-budget charges. macOS blocked `/usr/bin/sample` from inspecting the running process: `sample[74208]: sample cannot examine process 74207 (guion) for unknown reasons, even though it appears to exist; try running with sudo.` No elevated retry was made. The available counters identify list-copy/allocation volume as the largest measured guest-side cost, but they cannot determine whether allocation, native helpers, or VM dispatch dominates CPU time.

## Verification

- Local HTTP fixture tests: **13 passed**.
- Nine source-free relocated application packages: **9/9 passed**, including all 18 generated cases and the webhook concurrency check.
- Performance harness tests: **33 passed**; Python syntax compilation and `cargo fmt --all -- --check` passed.
- Runtime histogram boundary test and native/VM profile integration test passed.
- The five short-source Python comparisons from the earlier merged build remain in the historical report; all five were passing there. This follow-up did not rerun them.

The fresh-source ratio, RSS samples, package identities, and verification summaries are in [`2026-10-26-richards-followup.json`](2026-10-26-richards-followup.json). The histogram, profile counters, sample attempt, and preserved earlier profile report are in [`2026-10-26-richards-profile.json`](2026-10-26-richards-profile.json). These results describe the named workloads on this Mac and do not establish that Guion generally outperforms or replaces Python.
