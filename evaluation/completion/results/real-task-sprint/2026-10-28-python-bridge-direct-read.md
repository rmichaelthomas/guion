# Python bridge direct-read fast-path experiment — 2026-10-28

## Result

This experiment returned a complete buffered frame directly from `read()` and retained the `readinto` assembly path for partial reads. It passed the frame and bridge tests, and every benchmark output matched.

It did not produce a measurable speed improvement over the readinto-only version. The 100,000-row application measured **139.40 ms** for Guion `Bytes` and **134.30 ms** for the same package-backed Python function. The paired ratio was **1.038** (95% interval **0.987–1.059**), which establishes neither a win nor a loss. The no-op payload penalty was **1.034** (**1.000–1.115**). An independent-cohort bootstrap against the readinto-only run estimated **1.018** (**0.967–1.043**) for the full-application ratio and **1.003** (**0.975–1.072**) for the payload penalty. Both intervals include no change.

The direct-read path measured **5,101,212 bytes** peak for a 5,100,867-byte complete frame, compared with **5,101,732 bytes** for the readinto-only path. That 520-byte difference is negligible. The fast path therefore adds complexity without a demonstrated speed or memory benefit and is reverted; the committed readinto implementation remains in place.

I also screened a streaming-reader prototype for a single 5.1 MB attachment. The buffered pipe supplied the attachment in fragments, so the sidecar had to assemble a mutable buffer and copy it into the required immutable `bytes`; measured peak allocation was **10,203,084 bytes**, above the **7,651,300-byte** one-copy bound. The prototype was dropped before being retained in the branch. For this single-large-attachment case, the existing frame reader already avoids the extra conversion copy that the streaming prototype reintroduced.

The next measurement will compare repeated small Python calls within one Guion process. The current one-call probe combines interpreter startup and one bridge call, so it does not reveal whether the persistent sidecar has excessive per-call overhead after startup is amortized.

## Method and limits

- Host: macOS ARM64; Guion artifact format 44; CPython 3.14.7, NumPy 2.5.1, and orjson 3.11.9.
- The application and no-op controls each used 15 randomized fresh-process pairs. Every timed output was checked against its expected result.
- The direct-read and readinto-only cohorts ran separately. Their ratio comparison independently resampled the 15 within-cohort paired ratios 100,000 times, using fixed seeds 20261040 and 20261041. These estimates do not replace a same-session randomized binary comparison.
- The streaming-reader prototype was checked against the same allocation bound and discarded when partial pipe reads forced an additional mutable-to-immutable copy.
- OS file-cache state was uncontrolled. Host CPU attribution and whole-process peak RSS remain unavailable. The release build used two Cargo workers, and benchmark trials ran sequentially.
- The benchmark recorded unrelated untracked site files in the worktree; those files were not used or changed.

## Evidence

- [Application raw samples](2026-10-28-python-bridge-direct-read-application.json)
- [Empty/5.1 MB no-op raw samples](2026-10-28-python-bridge-direct-read-startup.json)
- [Readinto-only comparison](2026-10-28-python-bridge-readinto.md)
