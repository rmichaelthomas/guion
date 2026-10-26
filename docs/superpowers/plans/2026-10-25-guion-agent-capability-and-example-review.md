# Guion Agent Capability and Example Review Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task by task. Steps use checkbox (`- [ ]`) syntax for tracking. This plan precedes the source-run speed plan and is proposed for review; it authorizes no product-code change on its own.

**Goal:** Make Guion's agent-facing capability guidance and recipes accurately show what can be inspected, what the caller grants, and how programs fail safely.

**Architecture:** Keep the existing caller-controlled `HostPolicy` and project manifest as the authority for actual grants. Improve `inspect --json` so an agent can compare statically reachable effects with manifest grants while marking dynamic targets and later CLI grants as unknown. Repair the canonical examples and target metadata, then prove the documented failure paths with local, deterministic cases. Do not add a new source `requires` syntax in this pass: that would change the language and artifact contract, while a URL from `args()` still could not be resolved statically.

**Tech stack:** Rust CLI/runtime, existing JSON schemas and Python 3 standard-library repository checks. Invoke `python3` directly. Keep all new files and generated evidence inside `/Users/rmichaelthomas/guion`; no external endpoint or new dependency is required.

## Verified starting point and corrections to the outside review

- `Result` already has postfix `?` and exhaustive `match` for `Ok`/`Err` in `language/spec.md` and compiler conformance. Fix teaching examples before proposing another language operator. `unwrap` remains a runtime trap if used without a guard.
- `guion inspect --json` already reports transitive bytecode effects, including `network.http`, and whether the entry can reach a network or clock call. The gap is an explicit, reliable comparison to grants and clearer agent documentation. It cannot infer the hostname of `args()[0]`.
- Project `guion.toml` already accepts exact `[network].hosts`; CLI `--allow-http-host` can add a grant for a run. The runner remains the grant authority. Source code must never be able to grant itself access.
- The runtime rejects ungranted hostnames for the initial URL and each redirect, disables ambient proxies, and keeps `http_request` redirects on the initial origin. The host API documentation says DNS rebinding and broader egress isolation require operating-system controls. Therefore do not describe exact-host checks alone as complete SSRF protection.
- `http_request_bytes` already returns typed `HttpResponse`; the older text `http_request` returns a `Json` envelope. Teach the typed API where appropriate, but do not silently change the older API or claim all HTTP responses are untyped.
- `docs/agents/index.json` says the current Guion target matrix is pending, while `README.md`, `docs/status.md`, and `docs/pre-release-readiness.md` say it passed. `docs/compatibility-and-migrations.md` also contains an older pending sentence. Correct the stale metadata and page using the retained final matrix as evidence.

## Gate 0 — repo and evidence preflight

- [ ] Work from the Guion repo with one repo-local scratch/build area. Confirm clean Git scope, available tools, permission route, and the saved macOS ARM64/Debian ARM64 target aggregates before editing. If this chat lacks write access, arrange access or request the needed narrow elevation up front; do not cycle through predictable sandbox failures.
- [ ] Record the current `inspect --json` shape, index schema, recipes, manifest policy, and runtime host-check behavior as the baseline. Do not run a public network service or leave a local server/container running.
- [ ] **Gate:** every proposed behavior change below is mapped to a current source file and has an existing baseline to compare against.

## Task 1 — correct the agent contract and target metadata

**Files:** Modify `docs/agents/index.json`, `docs/agents/index.schema.json`, `docs/agents/start-here.md`, `docs/agents/recipes.md`, and `docs/compatibility-and-migrations.md`; modify `language/host-api.md` only where wording is stale. Use `evaluation/completion/results/guion-final/README.md` and its target aggregates as evidence; leave those frozen reports intact.

- [ ] Update `supported_targets.current_guion_validation` from the stale pending marker to a schema-valid passed state. Add fields for the two exact validated targets and an evidence path; update the schema's allowed properties and point the index's `source` to current readiness evidence. Keep the distinction between tested release targets and the Ubuntu x86_64 source-check job.
- [ ] Explain in agent docs: static effect inspection, project-manifest grants, additional run-time CLI grants, and dynamic target limits. Show `?` and `match` as existing safe `Result` alternatives, and distinguish `http_request_bytes` from the legacy JSON text response.
- [ ] Add a repository check that fails when agent target metadata contradicts the readiness evidence or status page. Validate the JSON/schema and documentation references using existing repository checks.
- [ ] **Gate:** an agent reading only the index and start-here page can identify the supported targets, the real permission boundary, and the existing error-handling syntax without an incorrect claim.

## Task 2 — make failure paths runnable and fix the unsafe recipe

**Files:** Modify `docs/agents/examples/sqlite_toml.gu` and `docs/agents/recipes.md`; create a focused negative-case recipe or runner under `docs/agents/examples/`; modify `docs/agents/index.json` to index it; extend the existing recipe validation test under `evaluation/tests/` or `crates/cli/tests/` without adding a new dependency.

- [ ] Replace each unguarded `unwrap` in `sqlite_toml.gu` with an explicit `match` or checked `Result` path. On failure, print the available stable error code/message, return nonzero, and roll back/close any opened SQLite transaction or handle. Guard empty query rows before indexing. Keep the successful output and persisted row behavior.
- [ ] Demonstrate four local negative cases separately: missing input file, omitted HTTP hostname grant, SQLite path outside its explicit writable root, and a `parallel_map` worker failure. Use a loopback server or a denied request that needs no external network; capture expected stable codes, exit status, and whether side effects occurred. The worker case must show `task_index`, `task_code`, and `task_message`; do not imply that completed worker side effects roll back.
- [ ] Add a deterministic test that runs the recipe cases against the current CLI, checks documented outputs/codes, and removes only its own repo-local scratch. Keep the examples readable enough for an agent to copy individually.
- [ ] **Gate:** all four failures are reproducible from the documented commands; the SQLite example exits cleanly on each injected read/parse/open/query failure, with no unguarded `unwrap` or index trap.

## Task 3 — make effect and grant inspection actionable

**Files:** Modify `crates/cli/src/main.rs` around `inspection_report` and project policy loading, `docs/agents/cli-reports.schema.json`, `docs/agents/index.json`, `docs/agents/start-here.md`, and `crates/cli/tests/cli.rs`.

- [ ] Extend `guion inspect --json SOURCE_OR_ARTIFACT` with additive fields that distinguish `required_effects` (transitive reachable effects), `manifest_grants` (what the adjacent project manifest declares, if one exists), and `grant_assessment` (`missing`, `present_but_target_dynamic`, `present`, or `unknown_external_runner`). Preserve existing report fields and schema version unless compatibility rules require a new version.
- [ ] Never label a runtime-supplied URL as statically approved merely because one or more hosts are granted. Never treat an absent manifest as proof that a later `guion run --allow-http-host` will not supply a grant. For `.gub` outside a project, report the artifact's effects and policy provenance separately; do not imply the artifact embeds the runner's policy.
- [ ] Test manifest host present/absent, CLI-only grant unknown at inspection time, dynamic URL, transitive function call, uncalled function, and packaged `.gub`. Confirm inspection itself does not execute code or contact a host. Keep `guion run` runtime enforcement unchanged.
- [ ] **Gate:** an agent can inspect a program and identify the effect categories and any manifest-backed grant gap before running it, while unknown dynamic targets remain explicitly unknown. Existing consumers of the version-1 JSON report continue to parse it.

## Decision after these gates

Revisit source-level `requires` only after the improved inspection and failure recipes are used on representative agent projects. A future syntax proposal must specify whether it declares effect categories, exact resources, or both; how transitive imports and dynamically supplied URLs behave; how declarations survive in `.gub`; compatibility for existing source/artifacts; and whether declarations are informational or compiler-enforced. A declaration must never create a grant. Named workers, typed SQLite row mapping, and changing the legacy text HTTP return type are separate API/language design projects, not prerequisites for this correction pass.

## Final verification and handoff

- [ ] Run the focused CLI/recipe tests, index/schema checks, formatting and strict Clippy for touched Rust code, and the relevant language conformance cases. Report exact pass/fail counts and any target not rerun; do not relabel earlier target evidence as a fresh run.
- [ ] Review Git scope by name, commit only intended source/docs/tests, verify no generated database, scratch directory, credential, or binary is staged, and check local/remote parity after any push.
- [ ] **Gate:** this plan's three task gates pass before starting `docs/superpowers/plans/2026-10-25-guion-source-run-and-launch-speed.md`. A failed or skipped gate remains visible rather than being treated as completion.

## Execution boundary

This document is a proposed plan for review. No example, CLI, language, or runtime behavior has been changed by writing it.
