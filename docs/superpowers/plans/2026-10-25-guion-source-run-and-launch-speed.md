# Guion Source Run and Launch Speed Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task by task. Steps use checkbox (`- [ ]`) syntax for tracking. This is a proposed plan; no performance implementation is authorized by this document alone.

**Goal:** Make repeated `guion run` from source feel close to running a prebuilt Guion artifact, then reduce measured first-run and process-launch costs without weakening correctness or growing unbounded build output.

**Architecture:** One repository-local performance session prepares the pinned tools and CLI once, keeps generated data under the ignored `target/perf-session/`, measures cold source, repeat source, prebuilt artifact, and matched Python in separate lanes, and writes a new evidence cohort without altering the frozen 2026-10-25 reports. Source-run acceleration uses a content-validated compiled-artifact cache; any uncertain input forces recompilation. The harness stays loaded while it coordinates fresh Guion and Python processes, so process startup remains part of each measured run. The cold path is changed only after phase timings identify its actual bottleneck.

**Tech stack:** Rust/Cargo 1.97.1, core `guion-cli`, Python 3.14.7 standard library and existing benchmark runner. Invoke `python3` directly; do not invoke `python` or install a virtual environment for this standard-library-only suite.

## Why this is next

The saved macOS ARM64 Guion report measured source versus pinned GIL-enabled CPython 3.14.7 as 951 ms versus 72 ms for Richards, 364 ms versus 210 ms for N-body, and 65 ms versus 43 ms for Mandelbrot. The corresponding prebuilt Guion artifacts took 69, 147, and 16 ms. These are named workloads, not universal language rankings. The CLI's source path calls `compile_project` on every `run`, while the artifact path loads compiled bytes. The measured gap justifies a source-loop sprint, but the exact breakdown of compiler setup, graph work, artifact loading, and runtime startup must be profiled before optimization.

## Gate 0 — execution environment and permission preflight

- [ ] Run this work from `/Users/rmichaelthomas/guion`, the sole Guion work/output directory. Check Git status, writable root, available disk, host architecture, `python3 -V`, `cargo -V`, `rustc -V`, offline Cargo dependency availability, and absence of running project containers before the long suite.
- [ ] Use the existing task's effective permission profile honestly. This projectless chat's current writable roots do not include `~/guion`. A Python virtual environment, Docker container, or script cannot grant that access. Prefer a Codex task/workspace rooted in `~/guion` with workspace-write permission. If executing in this chat, request the needed sandbox escalation *up front* for the planned repo write or benchmark phase, batch that phase in one command/session, and use a reusable narrowly scoped approval rule when available. Never run a known-to-fail sandboxed command merely to discover that elevation is needed. Auto-review may still deny an action; report that directly rather than retrying the same boundary.
- [ ] Keep `CARGO_TARGET_DIR` and all benchmark scratch/output under the repository's ignored `target/perf-session/`. Use `cargo build --release --locked --offline -p guion-cli` once per source revision and reuse the resulting binary. Pin the Python baseline to the verified 3.14.7 `python3`; no package download, new clone, Colima boot, or virtual environment is part of normal macOS profiling.
- [ ] Before any Debian gate, confirm a required target environment is actually available. Start only the needed target once, reuse it for that gate, and stop it afterward; otherwise record the target gate as incomplete. Record build-directory size and a cleanup command. Never leave a running container or deleted-but-open build cache behind.
- [ ] **Gate:** one preflight record states tools, paths, permissions route, available space, target availability, and exact commands; no benchmark starts with an unresolved permission or dependency boundary.

## Task 1 — one reusable performance session and baseline

**Files:** Create `evaluation/completion/performance/session.py`; modify `evaluation/completion/performance/runner.py` to accept an explicit scratch directory and output path; extend `evaluation/completion/performance/test_runner.py`; update `docs/development/performance-workspace.md` (create if absent) and `.gitignore` only if the chosen repository-local output path needs it.

**Interface:** `python3 evaluation/completion/performance/session.py preflight|baseline|compare|clean` runs from the repo root. `baseline` builds the CLI at most once for the current revision, records its identity, runs the existing correctness-checked suite plus tiny launch and two representative application cases, and writes a new dated report under `evaluation/completion/results/source-loop/`. `compare` reads two named reports and prints deltas without overwriting either. `clean` removes only the session-owned `target/perf-session/` tree after confirming no owned process is running.

- [ ] Add tests showing that an unpinned Python, wrong working root, inaccessible repo-local scratch, missing offline dependency, or insufficient disk fails preflight before any build or benchmark starts.
- [ ] Add tests showing the session reuses an unchanged release binary, rebuilds after source revision changes, never writes scratch outside the repo, and never overwrites the frozen Guion report.
- [ ] Implement the session and runner options with a single manifest of tool versions, source revision, binary hash, target, scratch size, commands, and outcomes. The normal runner remains compatible with its existing CLI.
- [ ] Record cold first source run, repeat source run, prebuilt artifact run, bare CLI launch, and matched Python cold/warm runs separately. Use paired randomized repetitions, correctness checks, medians and p95, and explicit cache state. Do not mix a Guion cached run with an uncached Python run under one unlabeled ratio.
- [ ] **Gate:** a second `baseline` invocation reuses the build and creates a separate result; generated scratch and build bytes stay in the ignored repo-local session area, while compact reports are retained under `evaluation/completion/results/source-loop/`; reports identify cold/warm conditions and exact binary/tool identities.

## Task 2 — safe reuse for `guion run` on source

**Files:** Create `crates/cli/src/source_run_cache.rs`; modify `crates/cli/src/main.rs` and `crates/cli/src/compiler_driver.rs` only at their source-run and compiler-input boundaries; add cases to `crates/cli/tests/compiler_driver.rs` and `crates/cli/tests/cli.rs`; update `.gitignore`, `docs/getting-started.md`, and `docs/agents/start-here.md` for the bounded project cache and user commands.

**Interface:** `source_run_cache::load_or_compile` is used only by `guion run` on source. It receives the resolved entry, project, standard-library and package roots, compiler identity, and a compile fallback; it returns a validated `Artifact` or the existing `Diagnostic`. `guion run --no-cache` forces compilation; `guion cache clean` removes only Guion-managed cache files for the selected project. On a hit the exact current source graph, manifest, lock/dependency roots, standard library, compiler seed/schema/ABI, target and relevant build options must match the recorded identities. Use content hashes, not modification times alone; cache writes are atomic and confined to a bounded project-local directory. If any source or dependency cannot be read or validated, do not use the cached artifact.

- [ ] Start with failing cases for repeat-run hit, edited direct/transitive import, changed manifest/lock/standard library/compiler identity, corrupt entry, symlink/root escape, concurrent writers, and `--no-cache`. Confirm failures or changed output are never hidden by a stale hit.
- [ ] Implement graph validation and atomic reuse without changing the existing `build`, `check`, `test`, or package semantics. Validate the loaded artifact using the same decoder as an explicit `.gub` run.
- [ ] Bound cache size (initial proposal: 128 MiB per project), prune deterministically, and report/clean it without touching source or packages. Cache bytes are ignored by Git and never enter a release archive.
- [ ] **Gate:** every cache mutation case passes; repeated source execution is correct and measurably approaches prebuilt execution on the three compute workloads; cache growth remains within its cap.

## Task 3 — reduce first-run and launch costs from phase evidence

**Files:** Modify only the measured bottleneck owners among `crates/cli/src/main.rs`, `crates/cli/src/compiler_driver.rs`, `crates/runtime/src/lib.rs`, and `language/self_host/`; retain phase timing in the new session reports and add focused regression coverage beside the touched code.

- [ ] Profile a no-op CLI invocation, tiny source, Richards, N-body, and Mandelbrot into separate process startup, seed validation/load, source graph read, self-hosted compile, artifact decode/load, native compilation, and program execution phases. Measure first uncached and repeated runs separately.
- [ ] Pick the largest *measured* controllable first-run or launch cost; record the choice and expected change in the report before editing. Optimize that path without skipping seed integrity checks, source diagnostics, artifact validation, or runtime policy checks.
- [ ] Verify cold runs improve against Task 1's baseline with unchanged outputs and that prebuilt artifact speed does not regress materially. If profiling shows no safe change can meet the target, record that result rather than changing architecture blindly.
- [ ] **Gate:** first-run Richards and N-body medians are at least 50% lower than the saved baseline, and tiny-program launch median/p95 are reported with no hidden warmup. If the threshold is not achieved, mark this gate incomplete and record the measured bottleneck.

## Task 4 — whole-product validation and decision

**Files:** Add the final dated `evaluation/completion/results/source-loop/` comparison and update `docs/pre-release-readiness.md`, `docs/status.md`, and `docs/history/evidence-index.md` with exact bounded claims. Keep the original Guion final cohort intact.

- [ ] Run formatting, strict Clippy, relevant Rust/Python tests, the 195-case language conformance corpus, cache invalidation cases, self-hosted bootstrap identity, all 36 application reports, and macOS/Debian runtime/release gates where the target environment is available. A missing target is incomplete, not a pass.
- [ ] Rerun paired Python comparisons on macOS ARM64 under matched cold and warm conditions. A repeat source run should be within 20 ms or 25% of its corresponding prebuilt artifact, whichever allowance is larger, on Richards, N-body, and Mandelbrot. Require source-run median parity or better against Python for those three warm cases. Keep installed-artifact correctness and require the upper 95% ratio bound to remain below 1.0 against pinned Python for each of the six previously required installed comparisons; report any regression in the exact medians and intervals.
- [ ] Compare the tiny-program launch and two real application cases; include median, p95, memory and generated cache size. Publish unfavorable outcomes and separate first-run results from repeat-run results. The next independent builder/recommendation study remains a later decision, not a claim from this speed suite.
- [ ] Remove only session-owned generated scratch after preserving compact reports, verify no process holds deleted build files open, and record final Git/remote parity and disk usage. Do not publish a release under this plan.
- [ ] **Gate:** a newcomer can reproduce the setup and read the cold, warm, artifact, and Python results without this chat; no required validation is missing or silently skipped.

## Review focus

1. An imported module changes while the entry file does not: the next run must recompile and show the new behavior.
2. A corrupted or forged cache entry tries to bypass artifact validation: it must be rejected and rebuilt safely.
3. A package lock, standard library, compiler seed, or runtime policy changes: no stale artifact may execute under a different identity or permission set.
4. A benchmark reuses Guion's compiled result but gives Python a cold first run: the report must keep those conditions separate.
5. The harness writes into `/tmp`, starts Colima unnecessarily, or grows without a cap: preflight or cleanup must flag it before another disk incident.

## Execution boundary

This is a proposed implementation plan. Review it before product code changes. The execution workflow will use one repository-local session harness and proactive permission routing; the harness does not override Codex's sandbox. If the app cannot give this task repo-root write access, known out-of-root operations will be submitted for elevation from the outset instead of attempted in the sandbox first.
