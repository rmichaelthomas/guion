# Python format-33 matched build session

Assignment ID: 01-support-sla-report-pair-2-python
Pair ID: 01-support-sla-report-pair-2
Task ID: 01-support-sla-report
Assigned language: Python
Pair order: python then veritas
Session slot: 01-support-sla-report-pair-2-python
Frozen model: OpenAI Codex gpt-6.1-sol
Reasoning effort: medium
Budget: 45 active minutes and 60 tool calls, initial plus follow-up combined
Manifest SHA-256: 2c326ad23d3d9ad94efc69b320d60dc3bb43ba2eac178bdfc581b5649d5bb949
Schedule SHA-256: abcc4bb175c51c22251846fc3609c94632123b196d549f1dd0a61de17eba76f5
Schedule file SHA-256: 04d198bcbe77295d2d0a2e413a4ecc24e3b48e12ae28ed4f5741b70b5c6cfd5a
Pre-run records SHA-256: a4d6ecff8b4840d60dfa8a881e7c34978c49979147b3f20b2ce398abed004d50
Predeclared primary measure: active_seconds
Candidate version: CPython 3.14.8 with uv 0.13.4

## Shared campaign workspace
Use only this fresh candidate project directory for all source, tests, logs, and delivery files:
/Users/rmichaelthomas/Documents/Codex/2026-10-22/referenced-chatgpt-conversation-this-is-an/work/veritas-foundation/evaluation/results/independent-v2/workspace/candidate/format33/01-support-sla-report-pair-2-python

The directory is empty. Your task thread's own working folder is separate; do not create the project there. Work under the shared candidate directory with every command's working directory set to this path. Do not inspect any sibling candidate, any other campaign session, or historical implementation/result.

## Runner contract
The frozen Python runner launches `uv run --locked --offline --project ${candidate} python3 ${candidate}/main.py`. Put the project manifest, lockfile, and `main.py` at the candidate root so the pinned runner can launch it. The same root-relative entrypoint must work for source and relocated runs. The container has the pinned Python/uv and offline wheelhouse; do not download dependencies. The coordinator will run official acceptance after each stage.

## Work
Complete only the initial request in the attached task brief. Use the supplied Python baseline and setup guide. You may use the standard library and locked offline packages. Do not inspect another language's candidate or any pair result. Do not use the official hidden-case evaluator yourself. Build a runnable, relocatable project with a locked environment, tests, concise setup/launch instructions, and delivery.json. Record every command, dependency, edit after initial completion, human intervention, and time spent.

The task brief includes the follow-up so you can see the complete requirement, but do not implement it until the coordinator sends it. Preserve the initial implementation, then stop and report the saved candidate path, commands and tests run, dependencies/lockfiles, package sizes, unresolved issues, and anything needed for official evaluation. No parallel or background work.

## Task brief
# Support ticket SLA report

**Class:** small automation or data CLI  
**Origin:** independent-candidate brief draft; revised after overlap audit  
**Initial request:** Build a CLI invoked as `sla-report --tickets PATH --policy PATH --as-of RFC3339 --output PATH`. RFC3339 timestamps use the exact `YYYY-MM-DDTHH:MM:SS[.fraction](Z|±HH:MM)` form with a valid Gregorian date, seconds from 00 through 59, a timezone offset, and (when present) one to six fractional digits; fractions longer than six digits are invalid. Apply the same timestamp syntax to `created_at`. RFC3339 timestamps use the exact `YYYY-MM-DDTHH:MM:SS[.fraction](Z|±HH:MM)` form with a valid Gregorian date, seconds from 00 through 59, a timezone offset, and (when present) one to six fractional digits. Fractions longer than six digits are invalid. The CSV header must be exactly `id,team,priority,status,created_at`; every data row must contain exactly five non-empty fields, and blank rows are invalid. The policy JSON must be an object containing a `response_minutes` object. Its only allowed keys are `high` and `low`; either may be omitted. Values that are present must be nonnegative JSON integers (Booleans are not integers). An unknown key, invalid value, or malformed policy is an actionable error and must leave an existing output untouched. The example policy is `{"response_minutes":{"high":60,"low":120}}`. The only accepted priorities are `high` and `low`; reject another value as `unknown_priority`. If a recognized priority has no `response_minutes` entry, reject it with the diagnostic code `missing_policy_entry`. Accept only `open` and `closed` statuses. Convert both timestamps to UTC and compute `age_minutes` as floor of elapsed seconds divided by 60; a `created_at` later than `--as-of` is invalid. Emit UTF-8 JSON with `as_of`, a `tickets` array sorted by team then ID, and a `summary` object containing total and breached counts. Each ticket has its id/team/priority/status, integer `age_minutes`, integer `limit_minutes`, and Boolean `breached`. An open ticket is breached only when its age is strictly greater than its priority limit; closed tickets are never breached. Reject malformed rows, duplicate IDs, unknown statuses, invalid timestamps, and missing policy entries without replacing an existing output.

**Follow-up change:** Add `--team NAME`, filtering the ticket array and summary to that team without changing the default result.

**Acceptance:** Exact output shape and stable order; a ticket exactly at its threshold is not breached; closed-ticket behavior; malformed/duplicate inputs; unknown priority; missing policy; output remains intact on failure; team match/miss; clean-workspace rerun.

**Fairness notes:** Python may use its standard library. All timestamps in the initial fixture use UTC so the task tests age and policy behavior rather than locale configuration. This brief replaces the earlier inventory-variance draft because the existing Veritas inventory-reconciliation example made that task too familiar.


## Python profile
# Python campaign profile

This campaign evaluates CPython 3.14.8 on Linux ARM64. The available launcher is `python3`; no `python` command is assumed. Candidates may use the standard library and the exact third-party distributions listed in the campaign dependency lock. Those dependencies are available only through the pinned offline wheelhouse, managed by uv 0.13.4. Network downloads and undeclared system interpreters are outside the campaign environment.

The Python candidate is a project with a lockfile and a runnable entry point. The evaluator first runs it from the candidate workspace, then copies only the declared delivery artifacts into a clean directory and runs it there. The task brief, fixture inputs, network permissions, container resource limits, active-time budget, and tool-call budget are the same as for Veritas.

This profile describes the pinned campaign baseline, not every Python implementation or deployment option. The broader language reference is [Python 3.14 documentation](https://docs.python.org/3.14/).


## Setup and delivery guide
# Task setup and delivery guide

This sheet defines the supported campaign setup paths for both candidates. The campaign runs on Debian-compatible Linux ARM64 with CPython 3.14.8 and uv 0.13.4. The same inputs, loopback network fixtures, CPU/memory limits, workspace rules, and offline wheelhouse apply to both languages. Python interop in a Veritas candidate must use that candidate project’s declared `[python]` version and lock; campaign-wide `VERITAS_PYTHON` and `PYTHONPATH` overrides are prohibited. Resolve only the shared pinned wheelhouse packages through the candidate project setup.

## Python

Create a project with `pyproject.toml`, a committed `uv.lock`, and a normal application entry point. Invoke setup with `uv sync --locked --offline`; use the shared offline wheelhouse for third-party dependencies. The user-facing run command must use `python3` or `uv run`, never an assumed `python` launcher. Build a wheel or zipapp before the task is reported complete. The delivered artifact must run from a new path with the original source tree absent. Record the exact sync/build/run commands, lock digest, package/dependency sizes, cold sync time, and warm run time. Dependencies not present in the wheelhouse are unavailable to both candidates.

## Veritas

Create a normal Veritas project with its declared `veritas.lock` and optional managed Python bridge lock. Use the documented `veritas sync`, `veritas test`, `veritas build`, and `veritas package` workflows as appropriate. The final package must launch from a new path with the original source tree absent. If Python interop is used, declare it through the managed project configuration and lockfile; do not call an undeclared system interpreter. Record exact sync/build/package/run commands, lock digests, artifact/dependency sizes, cold sync time, and warm run time. Any third-party dependency must come from the shared offline wheelhouse or be unavailable to both candidates.

## Required delivery manifest

Place `delivery.json` at the project root:

```json
{
  "launch_command": ["${candidate}/dist/app", "--help"],
  "artifact_paths": ["dist/app", "veritas.lock"],
  "source_paths": ["src", "main.vr"],
  "dependency_lock_path": "veritas.lock"
}
```

The launch command is an argument array executed without a shell. `${candidate}` names the relocated artifact directory and `${workspace}` names the clean directory containing task inputs and outputs. `artifact_paths` must include the lock named by `dependency_lock_path`. `source_paths` describe source trees/files that are excluded from the relocated run; they cannot overlap any delivered artifact. The evaluator hashes the delivered artifact set, stages task files in a fresh workspace, and runs the same cases against the relocated launch command. If a task's fixtures require application-generated state, that state must be created by the application from the fixture inputs.

The evaluator runs source acceptance and relocated delivery in separate restricted containers. The relocated container receives the same task inputs and fixtures but mounts only the declared delivery artifacts; the original source tree is absent. The package session has a fixed budget, so environment setup, packaging, and runtime costs remain visible. A task-class recommendation is exploratory and specific to this model/tooling/host sample; it is not a universal claim about either language.

