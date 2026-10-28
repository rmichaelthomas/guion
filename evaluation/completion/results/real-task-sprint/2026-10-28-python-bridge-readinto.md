# Python bridge frame-reader allocation and timing follow-up — 2026-10-28

## Result

The sidecar now preallocates the known frame size and fills it with `readinto`. On a buffered 5,100,867-byte frame, the source-level allocation check measured **5,101,732 bytes** of peak allocation, down from **10,201,824 bytes** with the prior `read` plus `bytearray.extend` loop. Short reads, clean EOF, and truncated frames retain their expected behavior.

The 100,000-row NumPy/orjson application remained at parity with the same package-backed Python function. Across 15 randomized fresh-process pairs, Guion `Bytes` measured **145.64 ms** and Python `bytes` **142.00 ms**; the paired ratio was **1.019** (95% interval **0.995–1.046**). That does not establish a speed change. Guion `Bytes` remained **12.6% faster** than its `Text` path and **21.5% slower** than the standard-library Python reference. All outputs matched.

The no-op control also found no speed win. With an empty payload, the Guion/Python ratio was **1.015** (95% interval **0.986–1.034**); with 5.1 MB, it was **1.051** (**1.032–1.067**). The payload penalty was **1.031** (**1.019–1.038**). An independent-cohort bootstrap comparing the prior and current payload-penalty ratios estimated a candidate/baseline ratio of **0.987** (**0.953–1.018**). The equivalent full-application ratio comparison was **0.999** (**0.980–1.033**). Neither comparison establishes a runtime speed change.

This pass closes one measurable allocation gap while leaving bridge speed effectively unchanged. The next experiment will try returning a complete frame directly from `read()` and use the preallocated `readinto` path only when the pipe returns a short read. That may avoid zero-filling a frame buffer on the common full-read path; it must retain the existing EOF and truncation behavior.

## Method and limits

- Host: macOS ARM64; Guion artifact format 44; CPython 3.14.7, NumPy 2.5.1, and orjson 3.11.9.
- The application and no-op controls each used 15 randomized fresh-process pairs. Every timed stdout/stderr and application output was checked against its expected result.
- The cohort comparison independently resampled the 15 per-pair ratios from each run 100,000 times, with fixed seeds 20261036 and 20261037. The raw cohorts are linked below so the comparison can be reproduced.
- The previous and current cohorts were run separately; OS file-cache state was uncontrolled. The bootstrap measures their observed difference but does not make them a randomized before/after trial.
- No host CPU attribution or whole-process peak RSS was available. The release build used two Cargo workers, and benchmark trials ran sequentially. Temporary benchmark inputs were removed automatically; the existing build/cache directory was retained.
- The benchmark recorded unrelated untracked site files in the worktree; those files were not used or changed by this experiment.

## Evidence

- [Application raw samples](2026-10-28-python-bridge-readinto-application.json)
- [Empty/5.1 MB no-op raw samples](2026-10-28-python-bridge-readinto-startup.json)
- [Prior empty/5.1 MB control](2026-10-28-python-bridge-startup-sizes.md)
- [Prior full application comparison](2026-10-28-python-bridge-single-copy.md)
