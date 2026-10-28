# Guion Python Bridge Cost Reduction Plan

> **For agentic workers:** Execute this plan inline, in task order. Keep all experiment artifacts under the Guion repository.

**Goal:** Reduce the measured fresh-process overhead of Guion's typed Python bridge on the 100,000-row NumPy/orjson workload while preserving exact behavior.

**Architecture:** Profile the current text-payload path with macOS Time Profiler, then compare it with the existing `Bytes` bridge value. If protocol JSON/base64 handling is a material cost, add a private binary attachment section to the length-prefixed bridge frame and benchmark the unchanged application result. Preserve the artifact format and public Guion syntax.

**Tech Stack:** Rust runtime, embedded Python sidecar, Guion CLI, CPython 3.14.7, NumPy 2.5.1, orjson 3.11.9, `xcrun xctrace`.

**Spec:** [`docs/north-star-design.md`](../../north-star-design.md), especially “Python compatibility and dependencies”; baseline evidence in [`2026-10-27-python-bridge-serialization.md`](../../../evaluation/completion/results/real-task-sprint/2026-10-27-python-bridge-serialization.md).

## Global Constraints

- Keep artifact format 44 and preserve current `Text`, `Bytes`, Python return conversion, timeout, cancellation, output-capture, and native-image reporting behavior.
- Keep frame size at or below 24 MiB, counting metadata, attachment descriptors, and attachment bytes.
- Preserve exact output and error behavior; do not relax data validation or filesystem/network capabilities.
- Use the same pinned workload inputs and Python packages for Guion and direct-Python comparisons.
- Keep trace files, generated inputs, builds, and temporary programs under ignored `target/perf-session/`; remove raw trace scratch after extracting compact evidence.
- Do not start a VM or download dependencies; use the warmed repository and host environment.

## Review Focus

- Large UTF-8 payloads containing non-ASCII text, quotes, slashes, and control characters must preserve exact bytes and Unicode.
- Raw byte values containing NUL and invalid UTF-8 must round-trip without text conversion.
- Multiple or nested byte values must map to the correct attachment index deterministically.
- Empty attachments and frames at the configured size limit must not corrupt subsequent calls.
- Existing error, timeout, cancellation, stdout/stderr, and native-image observations must remain unchanged.

---

### Task 1: Profile and reproduce the current bridge cost

**Files:**
- Create: `evaluation/completion/results/real-task-sprint/2026-10-27-python-bridge-host-profile.md`
- Create: `evaluation/completion/results/real-task-sprint/2026-10-27-python-bridge-host-profile.json`
- Scratch only: `target/perf-session/bridge-host-profile/`

**Interfaces:** Use the existing package-analysis input and adapter. A temporary Guion program repeats the exact typed call long enough for a bounded Time Profiler capture. Compare the stack attribution with the existing 15-pair timing report; the capture is diagnostic and does not replace timing evidence.

- [x] Verify the feature-enabled CLI and pinned Python package versions: Guion reports Python interop enabled; CPython 3.14.7, NumPy 2.5.1, and orjson 3.11.9 are installed.
- [x] Check the installed xctrace templates and attempt one bounded unprivileged Time Profiler capture. The template is absent; CPU Counters preflight also fails with the macOS kernel-policy error and Instruments assertion recorded in the report.
- [x] Use the existing output-checked 0-byte, 534,772-byte, and 5,100,867-byte size-scaling cohort as the fallback diagnostic. It shows where the wall-time gap appears but does not claim CPU-stack attribution.
- [x] Record the profiler availability and fallback measurements in the compact report. Keep the temporary profile program and build backup only in ignored repository scratch until final cleanup.

### Task 2: Add an output-checked Bytes-path comparison

**Files:**
- Create: `evaluation/development/guion_package_analysis/main-text.gu`
- Create: `evaluation/development/guion_package_analysis/main-bytes.gu`
- Create: `evaluation/development/guion_package_analysis/analysis.py`
- Create: `evaluation/development/guion_package_analysis/reference.py`
- Create: `evaluation/development/guion_package_analysis/reference_packages.py`
- Create: `evaluation/development/guion_package_analysis/README.md`
- Create: `evaluation/completion/performance/benchmark_python_bridge_payloads.py`
- Test: `evaluation/tests/test_guion_package_analysis.py`.

**Interfaces:** The current Guion project is separate from the retained pre-rename fixture. Its text entry point calls `analysis:analyze_document(source: Text, group_by: Text) -> Json`; its Bytes entry point calls `analysis:analyze_document_bytes(source: Bytes, group_by: Text) -> Json`. Both adapters share the same analysis function. The benchmark reports `guion_text`, `guion_bytes`, `python_packages`, and `python_stdlib` lanes.

- [x] Add an acceptance case that runs both Guion entry points on the same fixed input and compares their files and stdout byte-for-byte with the independent reference.
- [x] Run that case before implementation and confirm it fails because the current-format project is missing.
- [x] Add the Bytes adapter and entry point with equivalent output and matching error behavior.
- [x] Run the acceptance case and confirm all four outputs match.

### Task 3: Add binary attachments to the private bridge frame

**Files:**
- Modify: `crates/runtime/src/python_bridge.rs`
- Modify: `crates/runtime/python/sidecar.py`
- Modify: `crates/runtime/tests/python_bridge.rs`
- Test: unit tests in `crates/runtime/src/python_bridge.rs` plus runtime bridge integration tests.

**Interfaces:** Requests use private protocol version 2. The existing outer big-endian `u32` remains the frame-payload size. Requests without byte values remain JSON frames. Requests with byte values use `GBIN`, `u32 metadata_length`, JSON metadata, `u32 attachment_count`, then repeated `u32 attachment_length` plus raw bytes. A Bytes value is represented in metadata by its deterministic attachment index. Responses remain JSON-only frames. Both sides reject truncated, oversized, out-of-range, or trailing data.

- [x] Add failing serializer tests for deterministic attachment indexes, nested/multiple byte values, and total-frame size checks; they first failed before the frame writer existed and now pass.
- [x] Add an end-to-end Python bridge test that round-trips NUL, invalid UTF-8, and Unicode-adjacent binary bytes.
- [x] Implement protocol version 2 with borrowed Rust attachment writes and sidecar reconstruction to Python `bytes`.
- [x] Verify malformed-frame rejection for truncated metadata, truncated attachments, out-of-range indexes, trailing bytes, and oversized outer frames.
- [x] Run focused bridge tests and the full runtime suite with `python-interop` enabled: all 225 runtime tests pass.
- [x] Verify `cargo fmt --all -- --check` and `cargo clippy --locked --offline --workspace --all-targets --features python-interop -- -D warnings`.

### Task 4: Measure and document the result

**Files:**
- Create: `evaluation/completion/performance/benchmark_python_bridge_payloads.py`
- Create: `evaluation/tests/test_python_bridge_frame.py`
- Create: `evaluation/completion/results/real-task-sprint/2026-10-27-python-bridge-bytes.json`
- Create: `evaluation/completion/results/real-task-sprint/2026-10-27-python-bridge-bytes.md`
- Modify: `docs/status.md`, `docs/north-star-design.md`, `docs/north-star-sprint-log.md`, `docs/architecture.md`.

**Interfaces:** Use randomized paired fresh-process trials; check every output before recording a timing. Compare the unchanged Python function with both Guion transfer modes, retaining raw samples, source/binary hashes, and exact versions.

- [ ] Run 15 randomized output-checked pairs per lane on the fixed 100,000-row workload.
- [ ] Report medians and paired 95% intervals for each Guion lane against direct Python and against the prior text lane.
- [ ] Claim a bridge speed win only if the comparison's 95% interval excludes 1.0 in Guion's favor; otherwise record the measured reduction or lack of signal and keep the outcome open.
- [ ] Update current documentation and the sprint log with the result and its limits.
- [ ] Run final focused validation, `git diff --check`, and remove all scratch processes and raw profile files.
