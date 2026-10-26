# Richards Runtime Profile and Targeted Optimization Plan

> **For agentic workers:** execute this plan task by task in the current branch; keep the progress ledger under ignored `.superpowers/sdd/`.

**Goal:** Establish whether Richards is limited by list copies/allocations or control-flow/helper overhead, then optimize its largest measured runtime cost without weakening correctness.

**Architecture:** Add opt-in execution counters to the existing runtime and native backend, report them only in untimed diagnostic runs, and extend the existing performance harness with one-shot and repeated-source Richards lanes. Preserve ordinary runtime behavior and performance by leaving instrumentation disabled by default.

**Tech Stack:** Rust workspace, existing Guion performance runner, pinned Python 3.14.7, repository-local Cargo target and `target/perf-session/` scratch.

**Spec:** `docs/superpowers/specs/2026-10-26-guion-richards-runtime-profile.md`

## Global Constraints

- Work on branch `codex/guion-richards-profile` in the local `/Users/rmichaelthomas/guion` repository; all source, documentation, evidence, and scratch stay inside this repository.
- Use `evaluation/completion/performance/session.py preflight` before building; reuse one repository-local Cargo target, build with locked offline dependencies, and set `CARGO_INCREMENTAL=0`.
- Keep timing samples free of diagnostics. Use 15 randomized samples and check expected output on every sample. The one-shot uncached source lane must pass `--no-cache`; call this a Guion-cache bypass, not a cold-disk run.
- A repeated lane runs Richards 25 times in one source process so the first JIT compilation is amortized. Preserve compiler timings from separate untimed profiles and do not subtract them from wall-clock samples as though that were a direct measurement.
- Report estimated guest allocation charges/bytes and dynamic guest branches with precise labels. Do not describe these as physical allocator counts or hardware branch misses.
- Do not touch project `AGENTS.md`, change language semantics or runtime limits, install benchmark tools, or create files outside the repo.

## Review Focus

- Profiling is disabled by default and introduces no per-allocation or per-branch counter update on ordinary runs.
- Every counter has a documented scope, including native list-set copies, record charges, and VM/native conditional branches.
- The warm lane validates the expected Richards output 25 times and uses the same execution mode as the one-shot lane.
- Candidate source and benchmark evidence are compared against the merged `main` behavior using the same binary configuration, host, and workload.
- Any optimization is justified by the profile and has a regression check for output, aliasing, limits, and relevant performance.

---

### Task 1: Measure the Unmodified Richards Runtime

**Files:**
- Create: `evaluation/completion/results/runtime-efficiency/2026-10-26-richards-profile.json`
- Scratch: `target/perf-session/`

- [x] Run the existing session preflight once; confirm repository root, Python pin, offline Cargo readiness, scratch write access, and available disk.
- [x] Build the merged branch's release CLI once into the single session Cargo target and retain a baseline copy in the ignored session scratch.
- [x] Run 15 output-checked one-shot and repeated-source samples. Capture current phase timings and the binary hash.
- [x] Record baseline timing and phase evidence before adding diagnostics or changing runtime behavior.

### Task 2: Add Opt-In Runtime Cost Counters

**Files:**
- Modify: `crates/runtime/src/lib.rs`
- Modify: `crates/runtime/src/native_backend.rs`
- Modify: `crates/cli/src/main.rs`
- Modify: runtime and CLI tests adjacent to the affected behavior

- [x] Add failing tests proving diagnostics are absent by default and the profile reports allocation charge events/bytes, list-set totals, in-place updates, copy count/bytes, record charge count/bytes, and dynamic guest conditional branch executions when enabled.
- [x] Cover both VM and native execution paths. Keep hardware branch misses and physical allocator calls explicitly outside the metric names.
- [x] Implement counters behind an opt-in profile flag used only in diagnostic runs; ensure the disabled path does not update counters.
- [x] Run focused runtime/CLI tests and verify the diagnostics do not change program output or error behavior.

### Task 3: Extend the Richards Profile Harness

**Files:**
- Modify: `evaluation/completion/performance/runner.py`
- Modify: `evaluation/completion/performance/test_runner.py`
- Modify: the focused performance-session entry point if needed
- Update: `evaluation/completion/results/runtime-efficiency/2026-10-26-richards-profile.json`

- [x] Add harness tests for one-shot and 25-repeat lane construction, profile parsing, output validation, and precise report labels.
- [x] Collect untimed one-shot and repeated runtime profiles, including allocation/copy/branch counters and native compilation phase breakdown.
- [x] Run 15 randomized uninstrumented samples per lane with output validation; record cache conditions, machine, interpreter, CLI hash, and trial order.
- [x] Analyze whether list copies/allocation charges or dynamic branch/helper activity best explains the remaining cost. State clearly what the measurements can and cannot establish.

### Task 4: Optimize the Measured Bottleneck

**Files:**
- Determine from Task 3; expected scope is one focused runtime/native compiler change and its regression test.
- Update: the Richards profile report and summary

- [x] Write regression tests for the measured behavior and confirm they fail before the change where practical. The 8-item integer and float inline-storage tests failed before implementation and pass now; the 9-item external-storage check passes. A native profile-counter test also exposed and fixed invalid-index overcounting.
- [x] Implement the smallest safe optimization that targets the measured cost; preserve output, aliasing, instruction limits, and allocation limits.
- [x] Rebuild in the same Cargo target and rerun one-shot/repeated profiles and output-checked timing samples.
- [x] Keep the optimization only if its target metric improves and the result is not explained by changed correctness, workload, build mode, or cache conditions. Final randomized paired timing shows improvement in both lanes; all five short-source speed gates pass.

### Task 5: Verify, Review, and Prepare the PR

**Files:**
- Update: this plan and the profile summary
- Create: `evaluation/completion/results/runtime-efficiency/2026-10-26-richards-profile-summary.md`

- [x] Run focused Rust and Python harness tests, workspace formatting, strict Clippy for touched crates, and the five short-source speed gates against the final candidate.
- [x] Review the complete diff and report; check no secret, unrelated file, or generated scratch artifact entered the commit.
- [x] Remove task-owned `target/perf-session/` scratch after preserving the compact report; all benchmark and test command sessions have exited.
- [x] Commit, push `codex/guion-richards-profile`, open a PR against `main`, attach the PR, and leave it unmerged for user review. PR #3: https://github.com/rmichaelthomas/guion/pull/3.

## Plan Self-Review

- The plan first measures the exact merged runtime, then adds diagnostic-only counters, then selects an optimization from evidence instead of assuming that Richards is allocation-bound or branch-bound.
- Timing and profile lanes are separate, so profiler overhead cannot create the reported speedup or regression.
- The repeated in-process workload makes JIT compilation visible as a one-time cost while preserving a fresh-process one-shot result.
- User-facing claims will distinguish this workload's runtime, source/JIT compilation, estimated budget charges, and dynamic guest branches.
