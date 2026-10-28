# Python bridge attachment copy reduction — 2026-10-28

## Result

The sidecar's binary-frame parser now extracts each attachment through a `memoryview`, then makes the single immutable `bytes` copy required by the Python bridge contract. The previous `bytearray` slice followed by `bytes(...)` made two full-size allocations. A regression check against the parser's actual source failed before the change at **10,202,573 bytes** of peak temporary allocation for a 5,100,867-byte attachment; it now stays below the test's **7,651,300-byte** limit while still returning an exact `bytes` value.

The full 100,000-row NumPy/orjson workload passed all output checks in 15 randomized fresh-process pairs. Guion `Bytes` measured **138.49 ms** median; direct Python package `bytes` measured **136.05 ms**. The paired ratio was **1.020** (95% interval **0.988–1.031**), so this run does not establish a speed win or loss against the same package function. Guion `Bytes` was **15.1% faster** than Guion `Text` (paired ratio **0.849**, 95% interval **0.831–0.881**), and **19.9% slower** than the standard-library Python reference (ratio **1.199**, 95% interval **1.177–1.225**).

Every lane's report file and stdout matched the standard-library reference byte-for-byte. Compared with the earlier 2026-10-27 run, the separate medians moved from 140.59 ms / 137.93 ms (Guion / package Python) to 138.49 ms / 136.05 ms. Those are separate cohorts, not a randomized before/after comparison, so they do not prove the single-copy change caused the difference.

## Method and limits

- Host: macOS ARM64; Guion artifact format 44; CPython 3.14.7, NumPy 2.5.1, and orjson 3.11.9.
- Workload: 100,000 rows, 13 groups, 5,100,867-byte input, one typed Python call per Guion run.
- 15 randomized fresh-process trials per lane; each timed result was checked against the exact reference report and stdout.
- The existing benchmark report records a dirty checkout because unrelated site files were untracked during the run. The benchmark's relevant source hashes are recorded in the JSON; the site files were not used by the workload.
- OS file-cache state was uncontrolled. This run does not provide host CPU attribution or whole-process peak memory.
- No VM or dependency download was used. The release build used two Cargo workers; benchmark trials ran sequentially. Temporary benchmark inputs were removed automatically. The raw evidence is [`2026-10-28-python-bridge-single-copy.json`](2026-10-28-python-bridge-single-copy.json).

The bridge remains a live optimization target: the same-package comparison is statistically at parity in this run, not a demonstrated Guion win, and the standard-library reference remains faster. The next pass will isolate fixed bridge startup and no-op call cost before selecting another optimization.
