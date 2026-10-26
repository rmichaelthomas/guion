# Guion Runtime Efficiency at Useful Scale Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extend Guion's measured first-source advantage to larger data and concurrent workloads while reducing any runtime or memory bottleneck that the new evidence identifies.

**Architecture:** Extend the repository's existing performance session and runner instead of adding a second harness. Measure the merged Rust-default CLI first, save its binary and raw report under one repository-local scratch tree, then use the same harness and Cargo target for a measured optimization and candidate comparison.

**Tech Stack:** Rust workspace, existing Guion performance-session Python harness, pinned Python 3.14.7 standard library, existing Guion `GUION_PHASE_TIMINGS`, and the existing `resource.getrusage` sampler.

**Spec:** `docs/superpowers/specs/2026-10-26-guion-runtime-efficiency.md`

## Global Constraints

- Work only on `codex/guion-runtime-throughput`; keep source, docs, results, and scratch inside this checkout.
- Run `evaluation/completion/performance/session.py preflight` before a build or benchmark. Reuse `target/perf-session/cargo`; set `CARGO_INCREMENTAL=0`, use `--locked --offline`, and do not install packages.
- Keep before/after binaries and transient projects inside `target/perf-session/`; do not create another Cargo target tree.
- Compare fresh uncached source, cached source, artifact, and matched Python separately. Do not call `--no-cache` a cold-disk test.
- Benchmark output checks run on every sample. Phase-timed and memory-sampled invocations run outside measured timing lanes.
- Preserve the five short first-source speed gates, the six installed suite workloads, the 20,000-row JSONL task, and add a 200,000-row JSONL scale point.
- Do not touch project `AGENTS.md`, publish a release, weaken grants or runtime limits, or request elevated OS sampling.

## Review Focus

- Verify `--no-cache` is present on every uncached-source command and cache-hit states are reported separately.
- Verify the independent Python and SQLite outputs before timing; JSONL outputs compare as parsed data and parallel outputs remain exact.
- Verify `ru_maxrss` units and identify whether values describe one process, per-worker values, or a summed upper bound.
- Keep phase telemetry outside timing comparisons and confirm its output does not change program results.
- Check that documentation distinguishes source compilation, artifact execution, runtime work, and OS file-cache conditions.

---

### Task 1: Extend the Existing Harness for Uncached Scale and Memory Evidence

**Files:**
- Modify: `evaluation/completion/performance/runner.py`
- Modify: `evaluation/completion/performance/session.py`
- Modify: `evaluation/completion/performance/test_runner.py`
- Modify: `evaluation/development/famous_benchmarks/parallel_cpu.py`

**Interfaces:**
- Consumes: existing 3 compute, 2 parallel-CPU, JSONL 20,000-row workloads and the five first-source workloads.
- Produces: correctness-checked uncached source samples for all six established workloads, 200,000-row JSONL scaling samples, phase profiles, peak RSS evidence, and explicit cache/memory scope metadata.

- [x] **Step 1: Add failing tests for no-cache source command construction, phase profile correctness, memory units/scope metadata, and the parallel worker-memory report.**
- [x] **Step 2: Run the focused performance harness tests and confirm each new assertion fails for the missing behavior.**
- [x] **Step 3: Extend the existing session/runner with uncached source lanes, the 200,000-row JSONL scale point, and separately timed phase/RSS observations. Keep cached source and existing installed-artifact lanes intact.**
- [x] **Step 4: Add an untimed Python worker RSS mode for process-pool comparisons; report individual worker peaks and their sum as an upper bound. Keep ordinary benchmark stdout unchanged.**
- [x] **Step 5: Run focused Python harness tests and the existing performance-session tests; require the sampler check to return a positive value without elevation.**

### Task 2: Measure the Merged CLI Before Runtime Changes

**Files:**
- Create: `evaluation/completion/results/runtime-efficiency/2026-10-26-baseline.json`
- Scratch: `target/perf-session/baseline/`

**Interfaces:**
- Consumes: the merged `main` binary built from `791b4a60020e7760efc6ff74bd5dc2f9cf58e469`, the extended harness, and Python 3.14.7.
- Produces: a retained baseline binary and complete report for all six established workloads, plus the 200,000-row JSONL scale measurement.

- [x] **Step 1: Run the documented preflight once; verify repository root, Python pin, offline Cargo dependencies, scratch write access, and available disk before building.**
- [x] **Step 2: Build the merged baseline once into `target/perf-session/cargo`; retain a copy at `target/perf-session/baseline/guion` and record its hash.**
- [x] **Step 3: Run all correctness-checked lanes and capture baseline phase and RSS data. Save the raw report under `target/perf-session/` until the candidate report is ready.**
- [x] **Step 4: Inspect the paired ratios, phase breakdowns, RSS scopes, and scaling results. Choose the smallest high-impact compiler/runtime change that addresses a measured bottleneck; record any rejected avenue and why.**

### Task 3: Optimize the Measured Runtime Bottleneck

**Files:**
- Determine from Task 2; expected scope is a focused file under `crates/runtime/` or the compiler only if its phase data warrants it.
- Add a targeted regression test beside the affected behavior.

**Interfaces:**
- Consumes: the preserved baseline report and the selected workload's profile.
- Produces: a test-proven change with a measured improvement in its target time or RSS metric and no short-source gate regression.

- [x] **Step 1: Write a regression test for the measured behavior and observe it fail against the unoptimized implementation.**
- [x] **Step 2: Implement the smallest safe change supported by the profile; preserve capability checks, instruction/allocation limits, and output semantics.**
- [x] **Step 3: Run the focused regression test, relevant runtime/CLI suites, and strict Clippy; compare against the retained baseline without rebuilding a second Cargo target.**

### Task 4: Re-run the Candidate and Align Product Claims

**Files:**
- Create: `evaluation/completion/results/runtime-efficiency/2026-10-26-candidate.json`
- Create: `evaluation/completion/results/runtime-efficiency/2026-10-26-summary.md`
- Modify: `README.md`
- Modify: `docs/status.md`
- Modify: `docs/pre-release-readiness.md`

**Interfaces:**
- Consumes: the optimized candidate, preserved baseline, same harness, same pinned interpreter, and unchanged workload inputs.
- Produces: comparable before/after reports, preserved short-task gates, and accurate source/artifact/runtime wording in current project docs.

- [x] **Step 1: Rebuild the candidate in the existing Cargo target and rerun the six established workloads, 200,000-row JSONL scale case, phase profiles, and memory observations with the same trial counts and inputs.**
- [x] **Step 2: Rerun the five 21-pair short fresh-source gates and relevant existing CLI/compiler/runtime correctness checks.**
- [x] **Step 3: Compare candidate and baseline by workload; document medians, paired ratios/intervals, phases, memory method, RSS scope, correctness, and OS cache limits.**
- [x] **Step 4: Update README, status, and readiness docs to describe the compiler change, measured runtime results, memory findings, and exact evidence links without broad Python replacement claims.**

### Task 5: Final Verification, Cleanup, and Next PR

**Files:**
- Modify: `docs/superpowers/plans/2026-10-26-guion-runtime-efficiency.md`
- Modify: `docs/superpowers/specs/2026-10-26-guion-runtime-efficiency.md`

**Interfaces:**
- Consumes: implementation, before/after reports, docs, and regression coverage.
- Produces: a reviewed branch `codex/guion-runtime-throughput`, compact committed evidence, and a pull request against `main`.

- [x] **Step 1: Run the final relevant Rust tests, Python harness tests, formatting, and strict Clippy; read each result and confirm every workload output check passed.**
- [x] **Step 2: Remove only `target/perf-session/` after reports are retained; confirm task-owned command sessions have exited and the worktree contains only intended source/docs/reports.**
- [x] **Step 3: Inspect the staged diff, secret scan, exact report paths, and benchmark claim scope.**
- [x] **Step 4: Commit and push the branch, open a PR to `main`, attach its URL, and state any remaining performance or memory gap plainly.**

## Plan Self-Review

- The existing performance session owns preflight, one Cargo target, report identity, scratch, and cleanup; the plan extends those interfaces rather than creating a parallel workflow.
- The baseline and candidate use identical runner code, Python version, inputs, trial counts, and report fields.
- Timing, phase telemetry, and RSS sampling are distinct lanes, so diagnostics do not contaminate headline elapsed-time comparisons.
- The 200,000-row JSONL point makes scaling visible while the established 20,000-row point and parallel workloads preserve historical comparability.
