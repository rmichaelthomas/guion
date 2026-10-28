# Python bridge direct-read fast-path experiment — 2026-10-28

## Result

This experiment returned a complete buffered frame directly from `read()` and retained the `readinto` assembly path for partial reads. It passed the frame and bridge tests, and every benchmark output matched.

It did not produce a measurable speed improvement over the readinto-only version. The 100,000-row application measured **139.40 ms** for Guion `Bytes` and **134.30 ms** for the same package-backed Python function. The paired ratio was **1.038** (95% interval **0.987–1.059**), which establishes neither a win nor a loss. The no-op payload penalty was **1.034** (**1.000–1.115**). An independent-cohort bootstrap against the readinto-only run estimated **1.018** (**0.967–1.043**) for the full-application ratio and **1.003** (**0.975–1.072**) for the payload penalty. Both intervals include no change.

The direct-read path measured **5,101,212 bytes** peak for a 5,100,867-byte complete frame, compared with **5,101,732 bytes** for the readinto-only path. That 520-byte difference is negligible. The fast path therefore adds complexity without a demonstrated speed or memory benefit and is reverted; the committed readinto implementation remains in place.

The next target is the payload copy that remains after a complete frame has been read: the sidecar retains the full frame while materializing each immutable Python `bytes` attachment. A streaming binary-frame reader may be able to read attachment bytes directly while preserving the public `bytes` contract. It must retain exact frame-boundary validation, short-read handling, and malformed-frame behavior.

## Method and limits

- Host: macOS ARM64; Guion artifact format 44; CPython 3.14.7, NumPy 2.5.1, and orjson 3.11.9.
- The application and no-op controls each used 15 randomized fresh-process pairs. Every timed output was checked against its expected result.
- The direct-read and readinto-only cohorts ran separately. Their ratio comparison independently resampled the 15 within-cohort paired ratios 100,000 times, using fixed seeds 20261040 and 20261041. These estimates do not replace a same-session randomized binary comparison.
- OS file-cache state was uncontrolled. Host CPU attribution and whole-process peak RSS remain unavailable. The release build used two Cargo workers, and benchmark trials ran sequentially.
- The benchmark recorded unrelated untracked site files in the worktree; those files were not used or changed.

## Evidence

- [Application raw samples](2026-10-28-python-bridge-direct-read-application.json)
- [Empty/5.1 MB no-op raw samples](2026-10-28-python-bridge-direct-read-startup.json)
- [Readinto-only comparison](2026-10-28-python-bridge-readinto.md)
