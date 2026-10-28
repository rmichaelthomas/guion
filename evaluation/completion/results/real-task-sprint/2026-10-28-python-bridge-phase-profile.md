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

In the post-image-gate 15-pair cohort, the single-call ratio was within its interval around parity: **1.046** before and **1.016** after (0.967–1.026). At 10 calls the ratio was **1.018** (0.978–1.080), and at 100 it was **1.045** (1.018–1.107). At 1,000 calls Guion was still slower than direct Python. The later fast JSON cohort is recorded below and in the repeated-call report.

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

## Optional fast JSON path

The response-path experiment uses `orjson` only when the user's Python program has already imported it. It does not add a dependency or import the package on its own. A 50,000-iteration local serializer check measured **2.38 μs** for the standard-library response encoding and **0.21 μs** for `orjson`; a matching request-decoding check measured **2.14 μs** and **0.55 μs**. The checks used the same small bridge response and request shapes, and verified matching decoded values, byte-buffer support, duplicate-key handling, and large-integer parsing. A regression test confirms that an out-of-range JSON integer rejected by the fast encoder falls back to the standard encoder without losing value.

In the three-run, 1,000-call phase profile, repeated Python request parsing measured **2.68 μs** and response encoding **1.04 μs**. Rust's end-to-end response wait measured **24.11 μs**, of which **19.94 μs** elapsed before the first response byte; frame completion took **0.89 μs**, and channel delivery **3.18 μs**. Rust response JSON parsing remains **5.18 μs**. The profile confirms that the optional JSON path lowered the measured sidecar parse/encode costs, while the remaining wait is still mostly before the first response byte.

The repeated-call benchmark moved from **1.513×** to **1.405×** Guion/Python at 1,000 calls. The same-index paired excess-cost bootstrap estimated **9.84 μs less per call** (95% interval **2.50–13.17 μs less**). The fast-path profile's Python encode/write timers include the diagnostic profile envelope and are not normal unprofiled timings; the response-wait measurements and separate 15-pair benchmark are the primary end-to-end evidence.

Raw samples: [fast JSON phase profile](2026-10-28-python-bridge-fast-json-phase-profile.json), [pre-change benchmark baseline](2026-10-28-python-bridge-repeated-calls-before-fast-json.json), and [post-change benchmark](2026-10-28-python-bridge-repeated-calls-after-fast-json.json).

## Vectored-write experiment — reverted

The sidecar was changed temporarily to combine the response header and body with `os.writev`, including partial-write handling and a stream fallback for platforms without vectored writes. Its focused tests passed, but the performance evidence did not support keeping it. The three-run profile measured repeated response-write time at **2.43 μs**, compared with **2.33 μs** in the preceding fast-JSON profile. The 15-pair 1,000-call ratio was **1.406×**, effectively unchanged from **1.405×**. A same-index cross-cohort bootstrap estimated **0.92 μs less per call**, but its 95% interval ranged from **3.36 μs less to 4.14 μs more**, with 9 of 15 same-index differences improving. The vectored-write implementation was reverted.

Raw samples from the rejected candidate: [vectored-write phase profile](2026-10-28-python-bridge-vectored-write-phase-profile.json) and [15-pair benchmark](2026-10-28-python-bridge-repeated-calls-after-vectored-write.json).

## Typed response envelope — reverted

A follow-up candidate parsed responses into typed Rust structs instead of a generic JSON tree and omitted empty `stdout`, `stderr`, and `native_images` fields from the sidecar response. The focused bridge checks passed (11 Python checks and 26 Rust bridge tests) while this candidate was active.

Three diagnostic runs showed a lower Rust response-parse median, from **5.18 μs** to **4.42 μs** per repeated call. End-to-end response wait moved from **24.11 μs** to **21.41 μs**, including **17.44 μs** before the first byte in the candidate profile. These are separate diagnostic cohorts and are too small to establish a speed gain by themselves.

The 30-pair end-to-end cohort measured a **1.392×** Guion/Python ratio at 1,000 calls (95% interval **1.380–1.405**), with medians of **113.45 ms** for Guion and **81.12 ms** for Python. Against the preceding 15-pair fast-JSON cohort, estimated excess time fell from **30.93 μs** to **29.97 μs per added call**. An independent-cohort bootstrap estimated only **0.8 μs lower** per call, with a 95% interval from **3.6 μs higher to 4.3 μs lower**. The interval includes no change, so the typed parser and smaller envelope were reverted. The measured first-byte delay remains the next target.

Raw candidate samples: [three-run typed-response profile](2026-10-28-python-bridge-typed-response-phase-profile.json), [initial 15-pair benchmark](2026-10-28-python-bridge-repeated-calls-after-typed-response.json), and [30-pair benchmark](2026-10-28-python-bridge-repeated-calls-after-typed-response-30-pair.json).

## First-byte boundary and single-write request experiment

The earlier profile measured Rust's wait for the first response byte but did not show how much of that interval Python spent processing the request before it began writing. An opt-in timer now measures from receipt of the complete request frame to the start of the response write. Across five diagnostic runs of 1,000 calls, Python used **12.51 μs** in that interval. Rust measured **18.80 μs** from finishing the request write to receiving the first response byte, **5.25 μs** writing the request, and **22.87 μs** through receipt of the complete response and channel handoff.

The Python timer sums all work up to the first response write, while the existing phase timers attribute about **7.7 μs** of it to request parsing (2.56 μs), argument decoding (0.78 μs), module dispatch (1.17 μs), function call (0.28 μs), result encoding (0.50 μs), output capture (0.37 μs), native-image checking (0.86 μs), response envelope creation (0.16 μs), and JSON encoding (1.03 μs). Roughly **4.8 μs** remains between these instrumented phases. The difference between Rust's wait and Python's pre-write timer includes request delivery and wakeup before Python starts, plus response-write delivery to the Rust reader; it is not a direct CPU attribution.

Because a normal request has no binary attachments, a candidate serialized its four-byte length prefix and JSON body into one frame buffer and wrote that buffer once. A focused writer test confirmed the old path made two writes and the candidate made one; the 26 Python bridge integration tests passed. The performance result did not justify keeping it: the 30-pair ratio was **1.406×** (95% interval **1.362–1.427**), essentially the same as the preceding 1.405× fast-JSON baseline. An independent-cohort bootstrap estimated **0.15 μs more** excess cost per added call, with a 95% interval from **5.63 μs less to 6.17 μs more**. The candidate's request-write phase also stayed at **5.25 μs**, so it was reverted.

Before the module-cache fast path, repeated module dispatch took **1.17 μs** per call, including `importlib.import_module`, function lookup, and cached `orjson` lookup. A local microbenchmark found a larger potential saving in the two per-call stdout/stderr redirect contexts, so that path was tested first.

Raw samples: [five-run first-byte boundary profile](2026-10-28-python-bridge-first-byte-boundary-profile.json), [single-write candidate phase profile](2026-10-28-python-bridge-single-write-phase-profile.json), and [30-pair single-write benchmark](2026-10-28-python-bridge-repeated-calls-after-single-write-30-pair.json).

## Manual stdout/stderr capture scopes — retained

Each Python call previously constructed two `contextlib.redirect_*` managers. The sidecar now saves the current streams, assigns the per-call `StringIO` buffers directly, and restores both streams in `finally`, including when the Python function raises. This keeps output capture and exception behavior while removing the two context-manager objects from every call.

The five-run profile measured **11.30 μs** from full request receipt to the first response write, versus **12.51 μs** in the preceding five-run profile. In three separate 30-pair cohorts, the 1,000-call ratio was **1.387×**, **1.387×**, and **1.379×**. Pooled across those 90 candidate pairs, the median ratio was **1.385×**, compared with **1.415×** in the immediately preceding 30-pair baseline. An independent-cohort bootstrap estimated the ratio fell by **0.0296** (95% interval **0.0017–0.0513**). The paired excess-time estimate fell from **30.88 μs** to **28.96 μs per added call**, but its 95% interval for the reduction (**−0.61 to 3.89 μs**) still includes no change. This is a small, repeatable ratio improvement on the tested workload; the per-call estimate remains uncertain, and Guion is still slower than Python at 1,000 calls.

Raw samples: [baseline 30-pair cohort](2026-10-28-python-bridge-repeated-calls-before-manual-capture-30-pair.json), [candidate profile](2026-10-28-python-bridge-manual-capture-phase-profile.json), [first candidate cohort](2026-10-28-python-bridge-repeated-calls-after-manual-capture-30-pair.json), [second candidate cohort](2026-10-28-python-bridge-repeated-calls-after-manual-capture-second-30-pair.json), and [third candidate cohort](2026-10-28-python-bridge-repeated-calls-after-manual-capture-third-30-pair.json).

## Cached module lookup — retained

After the sidecar imports a Python module once, subsequent calls now check `sys.modules` directly and call `importlib.import_module` only for a cache miss. If user code replaces `importlib.import_module`, the sidecar keeps calling that replacement so the hook remains observable. The profile records the import fallback separately: it ran for the first fixture call and measured **0 μs** across the next 999 calls. Repeated `module_dispatch` fell from **1.19 μs** to **0.86 μs**.

The focused phase improved, but the 30-pair end-to-end cohort did not show a whole-workload gain: its 1,000-call ratio was **1.389×**, close to the preceding manual-capture result of **1.385×**. The small, allocation-free cache check is retained; Guion's full repeated-call advantage from this change is unproven. The test asserts that the first call takes the import fallback and later cached calls do not. See the [module-cache profile](2026-10-28-python-bridge-module-cache-phase-profile.json) and [30-pair benchmark](2026-10-28-python-bridge-repeated-calls-after-module-cache-30-pair.json).

The response-channel handoff now measures **2.89 μs** per call, making bounded single-response delivery the next experiment.

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
