# Workstream D and D1 status — 2026-10-26

## What the “independent second evaluator” meant

It was a rule written in the repository’s v4 protocol notes. No second person had been named by you. I incorrectly treated that internal review rule as a blocker after you had already authorized the Codex run. Your authorization was recorded as an owner override, and the frozen prompts, briefs, and schedules were left unchanged. I should not have framed this as something you needed to arrange.

## D4: what ran

The authorized Codex portion of D4 is complete: 18 fresh product-information choices and 18 fresh cold choices, using `gpt-6.1-sol`. Every session completed inside its five-minute budget and used zero tools. The saved records and the repository validator report 18/18 valid decisions in each arm. These are language-suitability opinions before implementation; they are not speed tests, implementation results, or evidence that Guion beats Python.

| Arm | Guion choices | Python choices | Either | Active time (median; range) |
|---|---:|---:|---:|---:|
| Product information | 3/18 | 15/18 | 0 | 26.2 s; 13.7–39.5 s |
| Cold (brief only) | 0/18 | 18/18 | 0 | 17.5 s; 13.8–29.2 s |

Product-information choices by task class:

- Small automation CLIs: Python 6/6, Guion 0/6.
- Concurrent/network integrations: Python 4/6, Guion 2/6.
- Maintainable applications: Python 5/6, Guion 1/6.

Cold choices were Python 6/6 in each of the three classes. The product-information arm’s three Guion choices came from two network-integration choices and one lockfile-auditor choice. No class had a Guion majority, so the protocol’s direct-recommendation signal is “insufficient.” That is a useful finding about the current product story and confidence; it is not a verdict on implementation performance. Claude and Gemini were not launched from this Codex host, as the plan specified.

### Why there are no visible choice chats now

The protocol calls for one fresh session per judgment: 9 briefs × 2 judgments × 2 arms = 36 Codex sessions. I created them concurrently instead of running them one at a time, which needlessly crowded your chat list and created extra empty workspaces. All 36 sessions are now archived, and their 36 empty external workspaces have been removed. The full transcripts and usage snapshots are preserved inside this repository. I will not create more sessions for this run.

## D1: what “partial” and “open” mean

The D1 ledger contains 11 closed topics, 4 partial topics, and 4 open topics. These labels classify evidence or a conditional choice factor; they are not failed tests or incomplete implementation tasks by themselves.

### Partial topics and the concrete closure work

**Source-free packages, managed runtimes, and supported targets.** Guion can package source-free applications with its runtime and managed Python environment on verified targets. Platform coverage and task-specific relocation evidence remain bounded to the tested matrix. Core commands do not discover or launch Python. Python-assisted projects use the explicit managed profile. Other operating systems, libc variants, and unvalidated target combinations remain outside the proven matrix.

**Close it by:** lock the release target matrix, then run clean install, relocation, integrity, and no-Python/no-uv checks on each supported target. Publish per-target package sizes and only claim coverage for targets that passed.

**Native library breadth and pre-release compatibility policy.** Guion now covers many common data, database, crypto, calendar, and HTTP tasks natively, while broader third-party library availability and pre-release compatibility remain narrower than Python’s ecosystem. Partial because the native surface has expanded, while compatibility guarantees and broad ecosystem parity remain work in progress.

**Close it by:** publish the pre-1.0 compatibility policy and a maintained native-library coverage list. Prioritize the libraries demanded by real Guion projects instead of chasing all of Python’s ecosystem at once.

**Task-specific setup, package size, and performance evidence.** There are scoped measurements for selected Guion workloads, but the choice briefs do not have comparable end-to-end setup, package-size, and acceptance measurements. Partial: measured speed evidence exists for named workloads, but not these nine brief-specific implementations, setup costs, and package sizes. Do not infer a general recommendation advantage.

**Close it by:** build matched Guion and Python versions of the nine frozen briefs. Check acceptance behavior first, then measure fresh setup time, delivered size, cold/warm execution, and peak memory. D4’s choice counts do not answer any of those performance questions.

**Network grants for runtime-supplied endpoints.** Network is denied by default and grants are exact-host, but programs with dynamic endpoint inputs need the runner or manifest to provide the matching host set. The runtime boundary exists and cannot be widened by program code. Static declaration/audit of a program’s required hosts is not provided, so orchestration still owns grant discovery.

**Close it by:** retain runner-controlled grants, and add a source or project declaration of required exact hosts that `check`/`inspect` can compare with the grants. Existing `inspect --json` already exposes reachable effect categories, manifest grants, and dynamic-target status; the remaining gap is declaring the exact host requirement. Source declarations must never grant access on their own.

### Open topics and the concrete closure work

**Reuse of task-specific adapters in an existing Guion project.** Whether a builder already has verified helpers or adapters is project-specific; the language cannot establish that fact for a future brief. Open as a condition: assess the actual project’s dependencies and validated adapters when the brief is supplied.

**Close it operationally:** add a project intake step that inventories the actual repository’s adapters, dependencies, and acceptance tests before making a language recommendation. It is intentionally project-specific.

**Conditions that depend on a changed task brief or deployment constraint.** Some conditions ask whether a task removes exact-byte or durable-state requirements, changes target constraints, or makes a specific delivery mode primary. They require a revised brief before a new recommendation. This is not a language feature. Preserve each trigger and reassess only against a formally changed brief or delivery contract.

**Close it operationally:** retain explicit change triggers and rerun the comparison only when the brief or deployment contract changes. This is a decision-process condition, not a compiler feature.

**Recoverable errors across the managed Python bridge.** Bridge import, call, and conversion failures remain runtime errors; Option and Result wire conversions are not implemented. Native service errors are typed. Recovering from arbitrary Python bridge failures as Guion Result values remains open.

**Close it technically:** add a typed bridge error and conversions that return Guion `Result`/`Option` values for import, call, and conversion failures; define which Python exceptions are recoverable and test them without turning them into interpreter crashes.

**Crash-atomic updates across multiple output files.** Atomic text replacement protects an individual file, but it does not provide one transaction spanning multiple outputs. The frozen briefs and current atomic-file contract do not provide a multi-file transaction. Keep this explicit in any release-notes or report generator design.

**Close it technically:** define a staged multi-file commit or journal API with clear crash guarantees, then test process termination at each commit phase. Current single-file atomic replacement does not provide a transaction across multiple files.

## Richards host-CPU attribution

A 3000-repetition Richards artifact completed successfully with exit code 0 and all 6000 expected output lines. The host profile also completed successfully after local Terminal authentication. Its raw file is 17,372,558 bytes and its SHA-256 is recorded in the metadata.

**What the profile says:** it contains 98 Guion process samples over 0.98 seconds. The command requested four seconds at 10 ms intervals, but spindump reported that it throttled sampling to control memory. In the sampled stacks, `Instance::invoke_inner` appears in all 98 samples, and `NativeCode::invoke` appears in 92. Counting each terminal stack frame once gives 59 samples in unsymbolicated generated native code, 12 ending in `native_list_set`, 14 ending in allocator/free/memory-move routines, 11 ending in fuel/cancellation/record helpers, one kernel sample, and one timekeeping sample. These terminal-frame buckets sum to 98; nested stack counts are inclusive and must not be added together. Some memory-management stacks are nested beneath `native_list_set`.

**Plain-English interpretation:** the measured Richards work is chiefly running in Guion's generated native code, so the profile does not point to VM dispatch as the main CPU cost. List mutation and memory-management routines are visible, but the generated-code addresses are not symbolized and this throttled 98-sample capture is too coarse to separate branch work from allocation/copy work inside Richards. The remaining allocation-versus-branch question is still open; this is a real first host-side CPU attribution, not another failed or skipped run.

The raw spindump was removed from the repository on 2026-11-03 because it captured every process running on the host, not only Guion; the figures above and its SHA-256 remain in the metadata. The attempt history is saved in [`2026-10-26-host-cpu-attribution.json`](../../completion/results/runtime-efficiency/2026-10-26-host-cpu-attribution.json). The run record is [`2026-10-26-richards-host-cpu-attribution.json`](../../completion/results/runtime-efficiency/2026-10-26-richards-host-cpu-attribution.json); the local command remains [`run-richards-host-profile.sh`](../../completion/performance/run-richards-host-profile.sh).

## Evidence in this repository

- D1 classification: `evaluation/agent-choice/switch-conditions.json`.
- Codex run state: `execution-status.json`.
- Per-arm validated summaries: `summaries/codex-product-information.json` and `summaries/codex-cold.json`.
- Full recommendation records: `records/codex-product-information/` and `records/codex-cold/`.
- Response transcripts and read-thread snapshots: `responses/` and `usage-snapshots/`.
- Owner authorization: `owner-authorization.md`.
