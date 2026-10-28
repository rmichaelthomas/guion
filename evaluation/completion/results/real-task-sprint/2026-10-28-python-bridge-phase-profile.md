# Python bridge phase profile and native image observation gate — 2026-10-28

## Result

The 1,000-call profile found that macOS dyld image enumeration was consuming most of the repeated-call budget. Before the change, the full image snapshot averaged **0.220 ms per repeated call**; Rust's end-to-end wait for each response averaged **0.256 ms**. The sidecar was rebuilding a set of every loaded native image after each call even when no image had changed.

The sidecar now takes a full snapshot when Python reports an import or `ctypes.dlopen`, or when the dyld image count changes. It uses the dyld count as a cheap check for persistent image additions made by native code outside those Python events. Cached calls with no image changes skip the full scan. The dynamic `ctypes` loading tests still report the newly loaded library, including when the Python function raises afterward.

| Measurement | Before | After |
| --- | ---: | ---: |
| Full native image snapshot, calls 2–1,000 | 0.220 ms/call | 0 ms/call |
| Cheap dyld count check, calls 2–1,000 | — | 0.00080 ms/call |
| Rust response wait, calls 2–1,000 | 0.256 ms/call | 0.0288 ms/call |
| Guion/Python ratio at 1,000 calls | 4.131 (4.046–4.305) | 1.481 (1.439–1.559) |
| Excess time per added Guion call | 0.2408 ms (0.2364–0.2542) | 0.0381 ms (0.0324–0.0442) |

The 1,000-call Guion median fell from **318.47 ms** to **116.31 ms**; Python measured **76.94 ms** and **78.37 ms** in the respective cohorts. The excess per-call estimate is descriptively **84.2% lower** after the change. The old and new measurements are separate 15-pair cohorts, so that percentage is not a paired before/after estimate.

The single-call ratio remains within its interval around parity: **1.046** before and **1.016** after (0.967–1.026). At 10 calls the latest ratio was **1.018** (0.978–1.080), and at 100 it was **1.045** (1.018–1.107). At 1,000 calls Guion is still slower than direct Python, so the repeated-call gap is reduced but not closed.

## Phase detail

Three opt-in diagnostic runs each made 1,000 bridge calls. The first call is separated from the remaining 999 so one-time package imports do not inflate the repeated-call estimate. On the first call, importing the fixture and its NumPy/orjson dependencies took about **31 ms** in Python. On later calls the measured Python work was small: about 4.7 μs parsing the request, 0.72 μs decoding its arguments, 0.98 μs dispatching the already-imported function, 0.26 μs calling it, and 0.45 μs encoding its result. The dyld count check averaged 0.80 μs, and no full scan ran on those calls.

The Rust response-wait phase is end-to-end. The sidecar's frame-read timing mostly measures time waiting for Guion to send the next request, not Python CPU work. A follow-up profile now separates the response's first-byte delay, frame completion, and reader-thread channel delivery; see the next section.

## Response-channel follow-up

Three more opt-in profiles each made 1,000 calls, and all printed the expected `64000` result. The latest response wait measured **35.10 μs per repeated call**. Its recorded components were:

| Response phase | Median per call |
| --- | ---: |
| Request write complete to first response byte | 30.54 μs |
| Read remaining frame after first byte | 0.91 μs |
| Reader-thread channel delivery | 3.53 μs |
| End-to-end response wait | 35.10 μs |
| Rust response JSON parse, after receipt | 5.14 μs |

About **87%** of the measured response wait elapsed before the first response byte; about **10%** was channel delivery and **3%** completed the frame. The measured channel handoff is a relatively small target, so a synchronous-reader rewrite is not the next change. The profile points the next experiment toward response construction and parsing, while the pre-first-byte interval still includes Python work and host scheduling that this profile cannot separate cleanly.

The Python-side `request_frame_read` timer includes waiting for Guion's next request after Python has already sent its response, so it does not line up with the Rust response interval. The Python `response_json_encode` and `response_write` timers include the diagnostic profile payload and should not be treated as normal, unprofiled costs. These values are diagnostic medians from three runs, not a new speed-comparison cohort.

Raw samples: [response-channel phase profile](2026-10-28-python-bridge-response-channel-profile.json).

The profiling hook is off by default. Set `GUION_PYTHON_BRIDGE_PROFILE_OUT` to a JSON path to write diagnostic totals; normal runs do not collect phase timers or attach profile data to bridge responses.

## Method and limits

- Host: macOS ARM64; CPython 3.14.7, NumPy 2.5.1, and orjson 3.11.9.
- The phase profiles are three diagnostic runs, not a timing benchmark. The separate speed comparison used 15 randomized fresh-process pairs at 1, 10, 100, and 1,000 calls. Every timed output matched the expected sum.
- The image observer is still a boundary observer. A native image loaded and unloaded inside one call can escape observation. A persistent image-count change or an import/`ctypes.dlopen` event triggers a full snapshot. A balanced image replacement that keeps the count unchanged and emits neither event may be missed.
- This result covers one fixture and host. It does not establish that Guion is faster than Python for arbitrary programs.

## Evidence

- [Before optimization phase samples](2026-10-28-python-bridge-phase-profile-before.json)
- [After optimization phase samples](2026-10-28-python-bridge-phase-profile-after.json)
- [After optimization 15-pair repeated-call samples](2026-10-28-python-bridge-repeated-calls-after-native-image-gate.json)
- [Repeated-call benchmark harness](../../performance/benchmark_python_bridge_repeated_calls.py)
- [Phase-profile harness](../../performance/profile_python_bridge_repeated_calls.py)
