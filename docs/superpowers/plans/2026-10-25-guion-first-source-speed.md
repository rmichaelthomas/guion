# Guion First-Source Execution Speed Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make fresh-process Guion source runs beat pinned Python 3.14.7 on five useful short tasks while preserving language correctness and bootstrap evidence.

**Architecture:** Measure the existing self-hosted CLI as a retained baseline, then route normal source compilation through the existing Rust compiler library embedded in the Guion executable. Keep the Guion compiler source, seed validation, bootstrap, and 195-case parity suite intact; fingerprint Rust compiler inputs into the source-cache identity and report truthful phase timings.

**Tech Stack:** Rust workspace and Cargo, existing `guion-compiler` and `guion-runtime`, Python 3.14.7 standard library for paired measurements and bootstrap confidence intervals.

**Spec:** `docs/superpowers/specs/2026-10-25-guion-first-source-speed.md`

## Global Constraints

- Compare five or more representative fresh-process tasks with 21 randomized Guion/Python pairs each.
- Invoke Guion with `run --no-cache`; do not reuse compiled source results in the primary lane.
- Use pinned Python 3.14.7 and verify every timed output against its expected value.
- Require every primary workload's paired median Guion/Python ratio and bootstrap 95% upper bound to be below 1.0 before claiming the gate.
- Keep Cargo output, profiler data, fixtures, and temporary work beneath `target/guion-first-source-speed/`; reuse one Cargo target directory and remove the sprint scratch tree once after reports are saved.
- Do not install dependencies or start containers; use `--locked --offline` and `CARGO_INCREMENTAL=0`.
- Preserve imports, path confinement, grants, artifact decoding, source cache validation, bootstrap, and self-host compiler parity.

## Review Focus

- Missing or malformed imports must keep structured diagnostics with the Rust compiler's source identity; pin with `default_cli_reports_rust_compiler_import_diagnostics`.
- Standard-library and package imports must remain confined to their granted roots; pin with `default_cli_resolves_granted_standard_module` and the existing package-root conformance cases.
- Editing an imported source or compiler implementation must invalidate cached artifacts; pin with existing direct/transitive cache tests and `compiler_cache_identity_includes_compiler_build_fingerprint`.
- Runtime file access in JSON and SQLite workloads must remain inside explicit roots; pin with benchmark correctness checks and existing file-grant tests.
- Bootstrap and self-hosted compiler parity must remain available after changing the CLI default; pin with `bootstrap` and the 195-case language conformance corpus.

---

### Task 1: Add a Reproducible Fresh-Source Benchmark Harness

**Files:**
- Create: `evaluation/first_source_speed/run.py`
- Create: `evaluation/first_source_speed/test_run.py`
- Create: `evaluation/first_source_speed/workloads/tiny/main.gu`
- Create: `evaluation/first_source_speed/workloads/tiny/main.py`
- Create: `evaluation/first_source_speed/workloads/arithmetic/main.gu`
- Create: `evaluation/first_source_speed/workloads/arithmetic/main.py`
- Create: `evaluation/first_source_speed/workloads/json_file/main.gu`
- Create: `evaluation/first_source_speed/workloads/json_file/main.py`
- Create: `evaluation/first_source_speed/workloads/json_file/records.json`
- Create: `evaluation/first_source_speed/workloads/imported_helper/main.gu`
- Create: `evaluation/first_source_speed/workloads/imported_helper/main.py`
- Create: `evaluation/first_source_speed/workloads/imported_helper/helper.gu`
- Create: `evaluation/first_source_speed/workloads/imported_helper/helper.py`
- Create: `evaluation/first_source_speed/workloads/sqlite_toml/main.gu`
- Create: `evaluation/first_source_speed/workloads/sqlite_toml/main.py`
- Create: `evaluation/first_source_speed/workloads/sqlite_toml/settings.toml`
- Test: `evaluation/first_source_speed/test_run.py`

**Interfaces:**
- Consumes: Guion executable paths and the absolute path to Python 3.14.7.
- Produces: `python3 evaluation/first_source_speed/run.py --python <python3.14> --guion baseline=<path> [--guion candidate=<path>] --trials 21 --output <repo-relative-json>`.
- Each command is run in a fresh copied project directory; setup and report writing stay outside measured intervals. Guion uses `run --no-cache --file-root <project> main.gu`; Python receives the same arguments and fixture. The report stores every sample, workload correctness, medians, nearest-rank p95, paired median ratios, deterministic bootstrap 95% intervals, binary hashes, interpreter/version, platform, and exact commands.

- [x] **Step 1: Write tests for nearest-rank p95, paired median ratios, deterministic bootstrap intervals, output mismatch rejection, and randomized paired execution order.**
- [x] **Step 2: Run `python3 -m unittest evaluation.first_source_speed.test_run` and verify it fails because the harness functions are absent.**
- [x] **Step 3: Implement the standard-library-only runner and five matched workloads. Use the existing `docs/agents/examples/file_data_cli.gu` and `sqlite_toml.gu` behavior as references, with fresh SQLite state per trial.**
- [x] **Step 4: Build the current self-hosted-default release CLI using `CARGO_TARGET_DIR=target/guion-first-source-speed/cargo`, `CARGO_INCREMENTAL=0`, `cargo build --locked --offline --release -p guion-cli`; copy that one baseline executable to `target/guion-first-source-speed/baseline/guion`.**
- [x] **Step 5: Run a one-trial smoke cohort against the retained baseline executable and pinned Python; verify each expected output and report schema.**

### Task 2: Make the In-Process Rust Compiler the Normal CLI Backend

**Files:**
- Modify: `crates/cli/Cargo.toml`
- Modify: `crates/cli/build.rs`
- Modify: `crates/cli/src/compiler_driver.rs`
- Modify: `crates/cli/src/main.rs`
- Test: `crates/cli/tests/compiler_driver.rs`
- Test: `crates/cli/tests/cli.rs`

**Interfaces:**
- Consumes: `guion_compiler::compile_project_with_roots(entry, project_root, stdlib_root, package_roots)`.
- Produces: the same `Artifact` and `Diagnostic` interface used by `check`, `build`, `run`, `inspect`, `lint`, and source-cache compilation; phase reports identify `compiler_backend: "rust"` and `rust_compiler_compile_ns`.

- [x] **Step 1: Add a CLI integration test that compares a missing-import diagnostic from normal `guion check --json` with `guion_compiler`'s diagnostic, and update the phase-report test to require the Rust backend name and compile duration.**
- [x] **Step 2: Run the targeted compiler-driver and phase-report tests; verify the new assertions fail against the self-hosted default.**
- [x] **Step 3: Promote `guion-compiler` from dev-dependency to normal dependency and route the CLI compiler driver through `compile_project_with_roots`, preserving root canonicalization, locked package roots, source graph digest validation, and artifact format.**
- [x] **Step 4: Remove per-process self-host seed verification from `execute`; retain build-time seed/manifest validation and all bootstrap inputs. Replace stale self-hosted phase names with truthful Rust compiler timings.**
- [x] **Step 5: Have `crates/cli/build.rs` fingerprint sorted Rust compiler sources, its manifest, and workspace `Cargo.lock`; include that fingerprint in `cache_identity()` and add `compiler_cache_identity_includes_compiler_build_fingerprint`.**
- [x] **Step 6: Run the focused CLI tests and cache invalidation tests; verify they pass with the Rust compiler as the default.**

### Task 3: Preserve Compiler Parity, Bootstrap, and Runtime Boundaries

**Files:**
- Modify: `crates/cli/tests/compiler_driver.rs`
- Modify: `crates/cli/tests/cli.rs`
- Existing verification: `crates/cli/tests/language_conformance.rs`
- Existing verification: `crates/cli/tests/bootstrap.rs`
- Existing verification: `crates/cli/tests/compiler_conformance.rs`

**Interfaces:**
- Consumes: the Rust-default CLI from Task 2, the retained Guion-authored compiler artifact and source, and existing stage-zero conformance helpers.
- Produces: preserved test evidence that the installed CLI works without an external compiler executable, while the self-host compiler still bootstraps and agrees on the full conformance corpus.

- [x] **Step 1: Update default-compiler assertions to describe an in-process Rust backend and assert structured source diagnostics, while retaining the empty-`PATH` and standard-module-grant checks.**
- [x] **Step 2: Run `cargo test --locked --offline -p guion-cli --test compiler_driver` and verify the updated CLI contract passes.**
- [x] **Step 3: Run `cargo test --locked --offline -p guion-cli --test language_conformance`, `cargo test --locked --offline -p guion-cli --test bootstrap`, and `cargo test --locked --offline -p guion-cli --test compiler_conformance -- --test-threads=1`; require the full corpus, bootstrap, and compiler boundary checks to pass.**
- [x] **Step 4: Run existing package-root, file-grant, artifact round-trip, direct/transitive source-cache invalidation, corrupt-cache, no-cache, and concurrent-cache tests.**

### Task 4: Measure the Candidate and Optimize Every Miss

**Files:**
- Modify: `evaluation/first_source_speed/run.py`
- Create: `evaluation/first_source_speed/long_regressions.py`
- Create: `evaluation/first_source_speed/test_long_regressions.py`
- Create: `evaluation/completion/results/first-source-speed/2026-10-25.json`
- Create: `evaluation/completion/results/first-source-speed/2026-10-25-summary.md`
- Existing reference workloads: `evaluation/development/famous_benchmarks/richards.vr`, `nbody.vr`, `mandelbrot.vr` and matching Python references.

**Interfaces:**
- Consumes: baseline executable saved in Task 1, candidate release CLI, five workload definitions, and the pinned interpreter.
- Produces: paired raw-sample JSON and a plain-language summary that calls the gate achieved only when all five median ratios and bootstrap upper bounds are below 1.0.

- [ ] **Step 1: Build the candidate release CLI into the same Cargo target directory and run the harness with both `baseline=<saved executable>` and `candidate=<release executable>`, using 21 randomized paired trials and correctness checks.**
- [ ] **Step 2: Run the three preserved Richards/N-body/Mandelbrot source programs and equivalent Python references as long-run regressions; record source, artifact, and Python timings where existing committed fixtures support all three.**
- [ ] **Step 3: If any primary workload misses the gate, use `GUION_PHASE_TIMINGS=1` on that workload first, keep any compact diagnostics under `target/guion-first-source-speed/`, and implement the smallest compiler/runtime optimization behind a regression test. Use system sampling only if it is available without requesting extra permissions. Repeat relevant cohort measurements; do not stop at a documented miss while a concrete optimization remains.**
- [ ] **Step 4: Save only compact, reproducible reports under `evaluation/completion/results/first-source-speed/`; report OS file-cache state as uncontrolled and avoid describing these as cold-disk results.**

### Task 5: Run the Final Gates, Clean the Sprint Cache, and Open the First PR

**Files:**
- Modify: `docs/superpowers/specs/2026-10-25-guion-first-source-speed.md`
- Modify: `docs/superpowers/plans/2026-10-25-guion-first-source-speed.md`
- Modify: `evaluation/completion/results/first-source-speed/2026-10-25-summary.md`

**Interfaces:**
- Consumes: implementation, parity evidence, benchmark JSON, and regression results from Tasks 1–4.
- Produces: completed verification record on `codex/guion-first-source-speed`, pushed branch, and a first PR targeting `main` with all performance claims tied to retained evidence.

- [ ] **Step 1: Run `cargo fmt --all -- --check`, `cargo clippy --locked --offline -p guion-cli --all-targets -- -D warnings`, focused cache and CLI tests, compiler conformance with `--test-threads=1`, bootstrap, all five benchmark correctness checks, and the final paired cohort; use only the sprint Cargo target directory and set `TMPDIR` to its absolute `target/guion-first-source-speed/tmp` path.**
- [ ] **Step 2: Remove `target/guion-first-source-speed/` once after retaining compact reports; verify no benchmark/compiler process remains and confirm the repository status contains only intended source, test, and report files.**
- [ ] **Step 3: Stage only the enumerated implementation, test, spec/plan, and report paths; inspect the complete staged diff and scan staged content for secrets, generated binaries, profiler dumps, and unrelated files.**
- [ ] **Step 4: Commit and push `codex/guion-first-source-speed`; open the PR against `main`, attach its URL to this task, and report the achieved and unmet gates plainly.**

## Plan Self-Review

- Spec coverage: benchmark fairness, five short tasks, correctness on each sample, compiler path change, cache identity, source validation, bootstrap/parity, preserved long workloads, compact reporting, cleanup, and PR delivery each have an owning task.
- The prior installed-release suite has six workloads, but this pass reruns the three canonical compute programs plus five short no-cache tasks. The older parallel CPU and 20,000-row JSONL comparisons remain historical evidence; current-build results for those two workloads are outside this focused pass and are called out in the final report.
- Step scan: each task starts with failing tests or a baseline measurement, then a bounded implementation and named verification; performance misses stay active targets.
- Type/interface consistency: the CLI compiler API remains `Result<Artifact, Diagnostic>`; the report schema uses `rust_compiler_compile_ns`; harness invocation inputs and outputs are named in Task 1.
- Review focus: import diagnostics, standard/package root confinement, cache invalidation, runtime file roots, and bootstrap parity have explicit tests or benchmark checks.
- Proportion: implementation tasks divide into benchmark harness, backend switch, compatibility, measured optimization, and release review; no task rewrites the whole product.
