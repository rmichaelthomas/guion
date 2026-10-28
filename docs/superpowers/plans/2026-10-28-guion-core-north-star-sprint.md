# Guion Core North-Star Sprint Implementation Plan

> **For agentic workers:** Use `superpowers:executing-plans` to implement this plan task by task. Steps use checkbox syntax for tracking.

**Goal:** Complete the Guion core north-star sprint outcomes with reproducible evidence, a coherent typed HTTP/text path, verified capability boundaries, actionable compatibility guidance, and a factual handoff.

**Architecture:** Implement from the current default branch while preserving the existing design commit. First freeze representative workloads and establish the environment; then use measured profiles to select any core optimization. Keep `http_request` and `http_request_with_timeout` compatible, use the existing byte-preserving `HttpResponse` path as canonical, and add explicit text/JSON response helpers. Audit capabilities against the existing `HostPolicy`, runner enforcement, and `inspect --json` output; change enforcement only when evidence finds a defect.

**Tech Stack:** Rust 2021 workspace, Rust/Cargo 1.97.1, locked dependencies, Guion standard library and compiler, python3 for evaluation scripts, and the existing Debian ARM64 image pinned by digest `sha256:6a45e30e542ab1dc5600a1e5a3a7579e3d520107a5fb0c1471af6e6f1e3ca1ac` for the Linux portion. Use one persistent container and sequential workloads.

**Spec:** `docs/superpowers/specs/2026-10-28-guion-core-north-star-sprint-design.md`

## Global Constraints

- The current default branch is `main` at `2058967`, which merges PR #21 (`81a5450`). The named branch's design commit is `36edc13`, parented on `81a5450`. Merge current `main` into the isolated feature worktree before source changes; retain the design commit and record `2058967` as the actual implementation base.
- Preserve user-visible semantics unless a documented migration is explicitly designed and tested.
- Existing `http_request` behavior must remain compatible or have an explicit, tested migration path; do not silently change its result type.
- Python interoperability remains optional; do not spend this sprint on bridge micro-optimization unless a core outcome demonstrably depends on it.
- Use the same container, mounted isolated worktree, and bounded, dedicated build/cache paths for sequential tests and experiments. Run offline after preparation, using local HTTP fixtures and loopback where needed.
- Record hardware, OS, versions, build flags, commit, dependency versions, workload sizes, sample counts, execution order, warmup and cache conditions. Retain raw samples, correctness oracles, and limits; do not generalize from one workload.
- Runtime capability checks are not operating-system process isolation. Target-specific claims must match the environment actually exercised.
- Create or update a PR only if authorized during implementation. Do not merge without the user's direction. Commit at sensible completed checkpoints.
- Do not change any project-level `AGENTS.md` file.

## API and Compatibility Decision

Keep `http_request_bytes(method, url, headers, body, timeout_ms) -> Result[HttpResponse, HttpError]` as the canonical response API; its record continues to expose `status`, lowercase `headers`, and raw `Bytes` `body`. Keep both legacy JSON-envelope request functions unchanged. Add `http_response_text(response: HttpResponse) -> Result[Text, HttpError]` with recoverable `R_HTTP_ENCODING` on invalid UTF-8. Add `http_response_json(response: HttpResponse) -> Result[Json, StdError]` in `std:http`, converting text-decoding errors and JSON parse errors to their existing stable codes/messages. This makes text decoding explicit, keeps JSON parsing composable, and avoids silently changing the existing request result type. Add one append-only opcode at tag 165 and bump the artifact format from 44 to 45; format-44 artifacts remain readable and reject the new instruction when declared as older formats.

## Review Focus

1. An artifact using the new response decoder must require format 45; existing format-44 artifacts and the legacy HTTP result types remain unchanged.
2. Invalid UTF-8 and malformed JSON return recoverable stable errors; non-2xx HTTP statuses remain response values.
3. Runtime URL hosts and every redirect remain subject to explicit grants; dynamic URLs and caller-supplied CLI grants remain marked unknown during inspection.
4. Inspection must not execute guest code or contact a host; file roots, environment reads, cancellation, and resource ceilings must match the actual runner boundary.
5. Benchmark conclusions must preserve equivalence, output checks, paired-run conditions, cache limits, memory-method limits, and the tested target.

---

### Task 1: Align the isolated branch with current `main`

**Files:** Git history only; preserve `docs/superpowers/specs/2026-10-28-guion-core-north-star-sprint-design.md`.

- [x] Merge `main` at `2058967` into `codex/guion-core-sprint-spec` in its existing clean worktree.
- [x] Review the merge result and record the actual base. Confirm PR #21's merged runtime state plus the current `docs/agents/guion-reference.md`, `language/host-api.md`, and `language/conformance/manifest.json` content are present.
- [x] **Gate:** the isolated branch contains the design commit and current-main changes, with no conflict or unrelated checkout edits.

### Task 2: Prepare one bounded environment and freeze the baseline

**Files:** Create a workload manifest and new reports under `evaluation/development/core_north_star_sprint/` and `evaluation/completion/results/real-task-sprint/`; do not rewrite historical evidence.

- [x] Inventory the repository toolchains, fixtures, Python baseline dependencies, optional features, disk space, Colima/Docker state, and the pinned image. Record starting disk use and image/container identity.
- [x] Create one persistent Debian ARM64 container from the pinned existing image, mount this isolated worktree, and use dedicated Cargo/build paths. Prefetch only required dependencies once and prove the locked offline build before lengthy runs. Do not prune shared images, caches, or containers.
- [x] Freeze equivalent tasks for fresh source/short scripts, text and JSON, collections/control flow, HTTP through a local fixture, and one real application workflow. Keep Richards as a diagnostic. For each task record inputs, output oracle, command, baseline commit, sample count, and cache/warmup policy.
- [x] Run sequential randomized paired baseline measurements; retain medians, uncertainty intervals, raw samples, peak-memory method, and any unmeasured dimensions. Do not start candidate optimization until this baseline is frozen.
- [x] **Gate:** the same pinned environment builds offline and every timed baseline run checks equivalent output.

### Task 3: Profile and optimize only measured core costs

**Files:** Change only the compiler/runtime/allocator/text code selected by the frozen profile; add focused tests alongside the owning crate. Store raw candidate results and the report under the Task 2 evidence directories.

- [ ] Profile the selected representative workloads before choosing a code path. State the correctness oracle and baseline comparison before each candidate run.
- [ ] Implement one focused compiler, runtime, allocation, or text optimization at a time. Each candidate must preserve output, security behavior, and meaningful memory limits; revert regressions or inconclusive candidates and retain their evidence as such.
- [ ] Do not optimize the optional Python bridge unless a measured core outcome demonstrably depends on it.
- [ ] **Gate:** each retained speed change is tied to a commit and a repeatable paired result; unsuccessful candidates are labeled and not shipped.

### Task 4: Add explicit HTTP response text and JSON helpers

**Files:** Modify `crates/model/src/lib.rs`, `crates/compiler/src/builtin_catalog.rs`, `crates/compiler/src/lib.rs`, `crates/vm-core/src/lib.rs`, `crates/runtime/src/lib.rs`, `language/self_host/builtin_catalog.gu`, `language/builtin-signatures.json`, `language/conformance/manifest.json`, `language/artifact-format.md`, `language/spec.md`, `language/host-api.md`, `language/stdlib/http.gu`, `docs/agents/examples/http_request.gu`, `docs/agents/recipes.md`, `CHANGELOG.md`, and `crates/runtime/tests/http.rs` plus the relevant compiler/CLI integration tests.

- [ ] Add failing coverage first: runtime tests `http_response_text_decodes_utf8_and_preserves_non_2xx_response`, `http_response_text_returns_recoverable_error_for_invalid_utf8`, `http_response_json_parses_valid_json_body`, and `http_response_json_returns_recoverable_error_for_malformed_json`; compiler test `http_response_text_is_typechecked_for_http_response_only`; model test `http_response_text_requires_format_45_and_round_trips`.
- [ ] Add the append-only `HttpResponseText` operation at tag 165, verify its `HttpResponse -> Result[Text, HttpError]` stack contract, implement checked UTF-8 conversion, and enforce format 45 in both serialization and reading paths.
- [ ] Add `std:http.http_response_json` using the text helper and existing JSON parser; normalize failures to `StdError` without changing either legacy request function.
- [ ] Update the catalog, self-hosted compiler signature source, conformance inventory, artifact-format contract, language/host API docs, runnable example, and generated agent reference.
- [ ] Demonstrate malformed JSON, host denial, timeout, response-size limits, and local loopback success through documented commands. Keep request counts and performance claims measured separately.
- [ ] **Gate:** focused runtime/compiler/CLI checks pass; format-44 compatibility and format-45 serialization are covered; examples match the actual typed API.

### Task 5: Audit capabilities and resource boundaries

**Files:** Review `crates/runtime/src/lib.rs`, `crates/runtime/src/resources.rs`, `crates/cli/src/main.rs`, `crates/runtime/tests/http.rs`, `crates/runtime/tests/execution.rs`, `crates/cli/tests/cli.rs`, `docs/agents/cli-reports.schema.json`, and the capability sections of `language/host-api.md`; create an audit report under `evaluation/completion/results/real-task-sprint/`.

- [ ] Exercise exact and dynamic URL hosts, redirects, denied grants, file roots and traversal, exact environment grants, cancellation, and resource ceilings. Compare enforcement with `inspect --json` and its schema.
- [ ] Extend regression coverage only for confirmed gaps; inspection remains non-executing, dynamic URL targets remain unknown, and guest code cannot expand host grants.
- [ ] State exercised boundaries and limitations, including DNS rebinding and the lack of OS process isolation. Do not describe static declarations as proof of arbitrary dynamic behavior.
- [ ] **Gate:** each claimed boundary has a source read or a deterministic local test; unresolved gaps are named with a concrete next action.

### Task 6: Reconcile tooling, compatibility, and agent guidance

**Files:** Modify `README.md`, `docs/status.md`, `docs/compatibility-and-migrations.md`, `docs/agents/index.json`, `docs/agents/index.schema.json`, `docs/agents/start-here.md`, `docs/agents/recipes.md`, `language/host-api.md`, and generated `docs/agents/guion-reference.md` only where measured evidence shows a mismatch.

- [ ] Compare target claims, CLI output/schema, compatibility boundaries, examples, and supported-target evidence. Correct only claims supported by current retained reports or checks run in this sprint.
- [ ] Update the agent-facing instructions for the typed response path and its recoverable text/JSON errors. Keep the distinction between release-target evidence and the Ubuntu source-check job.
- [ ] Generate and validate the agent reference from its authoritative source files; run the schema, reference, formatting, and artifact-format audits.
- [ ] **Gate:** README, compatibility docs, machine-readable index/schema, examples, and generated agent reference agree with code and retained evidence.

### Task 7: Prepare and run an unfamiliar-builder challenge

**Files:** Create a frozen brief, acceptance cases, run instructions, and a results record under `evaluation/development/core_north_star_sprint/builder_challenge/` and `evaluation/completion/results/real-task-sprint/`.

- [ ] Freeze the application brief and acceptance cases before the challenge; prepare the normal Guion documentation/tooling packet and a two-day run window.
- [ ] Before sharing any repository materials with an evaluator, ask the user to approve the specific evaluator and transfer. Previously approved studies do not authorize this transfer.
- [ ] If approved, capture time, attempts, errors, documentation gaps, completed functionality, and reproducible acceptance results. Identify an isolated agent evaluator as such if selected; do not present it as broad adoption evidence.
- [ ] Fix the highest-impact in-scope obstacles and rerun affected acceptance checks. If no suitable evaluator is authorized or available, record the exact blocker and leave this acceptance outcome open.
- [ ] **Gate:** no hidden solutions or mid-run rule changes; the challenge either completes as specified or is explicitly reported incomplete.

### Task 8: Final measurements, acceptance, and handoff

**Files:** Create a plain-English sprint report and raw final samples in the Task 2 evidence directories; update the project docs from Task 6.

- [ ] Re-run the frozen baseline workloads after code changes in the same pinned environment, with sequential randomized paired runs and checked outputs. Record results, intervals, peak-memory method, target, commit, versions, and limitations.
- [ ] Run the relevant Rust formatting, artifact audit, compiler/runtime/CLI tests, conformance, bootstrap, agent-reference, and offline workspace checks. Report exact commands, counts, failures, and skips; run cross-target gates only where an available target can be exercised.
- [ ] Review source and generated-file scope by name, check staged content, commit completed checkpoints, and preserve all raw samples and useful reports. Do not create a PR or merge without authorization.
- [ ] **Gate:** every sprint acceptance item is evidenced, or its specific unresolved blocker, incomplete status, and next action are recorded.
