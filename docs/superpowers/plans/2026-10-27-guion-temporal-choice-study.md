# Guion Temporal Choice Study Execution Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:executing-plans` to execute this plan after the Guion temporal feature passes its implementation gates. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Measure whether independent coding agents discover and choose Guion for temporal programming when they are free to choose, while measuring assigned-language correctness and reported reasons separately.

**Architecture:** Build a reproducible local runner, fixed hidden-output oracle, and pinned environment for Guion, Python, and TypeScript. Run three tasks under discovery choice, named-option choice, and assigned-language control. Record first implementation language and unsolicited recommendation as observed behavior; collect explanations only after the first artifact and oracle result are retained. Analyze every run, including failures and timeouts.

**Tech Stack:** Guion release CLI, pinned Python 3, pinned Node.js/TypeScript, packaged timezone data, Python 3 standard-library harness, JSON/JSONL evidence files, independent coding-agent sessions from three recorded model families.

**Protocol:** `docs/superpowers/specs/2026-10-27-guion-temporal-choice-protocol.md`

## Global Constraints

- Do not begin scored runs until the approved native resolver/calendar behavior is implemented, passes the deterministic fixtures, and is installable in the exact workspace offered to participants.
- Keep the three tasks and their input/output contracts fixed across conditions. Do not name languages in discovery prompts. Randomize order when named options are supplied.
- Use fresh, independent sessions; do not share prompts, artifacts, outputs, or rationales between runs.
- Keep three measurements distinct: actual first runnable language, recommendation volunteered before a question, and recommendation requested afterward. Self-reported reasons explain what agents say; they do not prove causation.
- Assigned-language runs assess task completion, not preference. Do not ask assigned agents why they selected their assigned language.
- Pin interpreter/compiler builds and timezone data. If equivalent timezone data cannot be demonstrated across languages, exclude affected temporal cases from language-comparison scoring and report why.
- The design calls for 81 scored runs. If the host cannot supply three independent model families or complete all cells, report the actual smaller sample and do not silently substitute, extrapolate, or call it the planned study.
- Do not claim that Guion has reached the north star from this exploratory study alone.

## Review Focus

- Oracle correctness and error precedence are fixed and tested before the first scored run; expected outputs are not supplied to agents.
- Prompts differ only by the intended condition. The discovery condition does not disclose Guion or present a language shortlist.
- The runner saves prompts, hashes, exact commands, versions, first artifacts, first outputs, retries, reviewer decisions, and incomplete runs.
- Choice denominators are reported by condition, task, and model family; report raw counts alongside any percentage.
- Recommendation and rationale categories remain auditable, with two independent reviewers coding choice explanations.

---

### Task 1: Build and Test the Hidden Oracle and Evidence Format

**Files:**
- Create: `evaluation/temporal_choice/schema/run-record.schema.json`
- Create: `evaluation/temporal_choice/schema/report.schema.json`
- Create: `evaluation/temporal_choice/fixtures/t1-local-times.json`
- Create: `evaluation/temporal_choice/fixtures/t2-calendar.ics`
- Create: `evaluation/temporal_choice/fixtures/t3-validator.json`
- Create: `evaluation/temporal_choice/oracle.py`
- Create: `evaluation/temporal_choice/tests/test_oracle.py`

**Interfaces:**
- T1 accepts JSON rows with `sourceId`, `date`, `time`, and `timeZone`; checks exact/gap/fold, preserves inputs, and uses error order `E_SOURCE_ID`, `E_DATE`, `E_TIME`, `E_TIMEZONE_UNKNOWN`.
- T2 accepts iCalendar data and uses the approved event/resolution schema, all-or-nothing event errors, and source-order error reporting.
- T3 validates already-classified rows and does not resolve local time.
- Oracle output stores expected JSON and exit status; score independently for schema, identity, errors, and semantic correctness.

- [ ] **Step 1: Create adversarial fixtures and hashes** for ordinary exact values, transition-adjacent exact values, the Los Angeles spring gap and fall fold, malformed date/time, unknown zone, missing identity, calendar UTC/all-day/embedded-zone/gap/fold/floating/unknown-zone/malformed/escaped-summary/RRULE cases, and validator ordering/cardinality/source mismatches.
- [ ] **Step 2: Write failing oracle tests** for every expected output, stable error precedence, empty result shapes on malformed input, candidate ordering, and source preservation; run `python3 -m unittest evaluation.temporal_choice.tests.test_oracle` and confirm expected failures before implementing the oracle.
- [ ] **Step 3: Implement the hidden oracle and JSON schemas** using only pinned timezone data and deterministic code paths; keep answer files outside the agent-visible workspace.
- [ ] **Step 4: Run the oracle tests and schema checks**; require every fixture to match the fixed expected result and every malformed case to return the specified nonzero status.

### Task 2: Assemble and Verify the Three Equal-Access Environments

**Files:**
- Create: `evaluation/temporal_choice/environment/manifest.json`
- Create: `evaluation/temporal_choice/environment/README.md`
- Create: `evaluation/temporal_choice/prompts/discovery/`
- Create: `evaluation/temporal_choice/prompts/named-options/`
- Create: `evaluation/temporal_choice/prompts/assigned/`
- Create: `evaluation/temporal_choice/tests/test_environment.py`

**Interfaces:**
- Every run receives the same workspace layout, tool discoverability, fixtures, resource limits, time limit, and offline setting; only prompt condition or assigned language differs.
- Record exact Guion, Python, Node/TypeScript, timezone-data, OS, architecture, and model identifiers in the manifest.
- Prompts are templates for T1 resolver, T2 calendar integration, and T3 validator; the task contract is identical across conditions.

- [ ] **Step 1: Write environment tests** that compare available files, tool versions, timezone-data release, prompt contracts, and workspace hashes across the three language lanes; demonstrate failure when a required tool or equivalent data version is missing.
- [ ] **Step 2: Build the three environment lanes** with Guion and its docs/examples, Python 3 and standard-library zone support pinned to the same tzdata release, and TypeScript/Node with an equivalent pinned timezone-data package; disable network access for scored runs.
- [ ] **Step 3: Write condition-specific prompt templates** with no language list in discovery, randomized equal-name lists in named-option runs, and one explicit assignment in controls; verify prompt diffs contain only condition text and assignment.
- [ ] **Step 4: Run one unscored dry run per task/language lane** to verify tools and time limits; exclude all dry-run results from study totals and record setup failures.
- [ ] **Step 5: If timezone-data parity cannot be established**, revise the manifest and scoring map before the scored cohort so non-comparable temporal cases are excluded consistently; do not proceed with a misleading cross-language score.

### Task 3: Implement the Run Recorder and Randomized Scheduler

**Files:**
- Create: `evaluation/temporal_choice/runner.py`
- Create: `evaluation/temporal_choice/scheduler.py`
- Create: `evaluation/temporal_choice/tests/test_runner.py`
- Create: `evaluation/temporal_choice/tests/test_scheduler.py`

**Interfaces:**
- Schedule 36 discovery runs (3 tasks × 3 model families × 4 repeats), 18 named-option runs (3 × 3 × 2), and 27 assigned controls (3 tasks × 3 languages × 3 families), total 81.
- Store one immutable JSONL record per run: run ID, prompt/hash, model, condition, assigned/observed language, exact environment, first artifact/output, oracle result, errors, time, corrections, timeout/incomplete status, volunteered recommendation, and post-task responses.
- Preserve the first runnable artifact even when the agent later switches language; record the complete sequence and final language separately.

- [ ] **Step 1: Write runner and scheduler tests** for balanced cell counts, reproducible randomized order, isolation of run workspaces, immutable first-output capture, timeout retention, language-switch recording, and schema validation; confirm they fail before implementation.
- [ ] **Step 2: Implement a deterministic schedule generator** with a recorded random seed and exact run IDs; assert 81 unique cells and balanced counts before dispatch.
- [ ] **Step 3: Implement run capture and oracle scoring** so every output, retry, exit status, elapsed time, and mismatch is retained without dropping incomplete runs.
- [ ] **Step 4: Add a reviewer queue** for oracle mismatches and two independent rationale-coding fields; preserve raw rationale and reviewer disagreement rather than overwriting it.
- [ ] **Step 5: Run the harness test suite** and a separate mock-agent dry run that cannot enter scored results; verify reproducible schedule and valid report records.

### Task 4: Run the Independent Agent Cohort

**Files:**
- Create: `evaluation/results/temporal-choice/YYYY-MM-DD/environment-manifest.json`
- Create: `evaluation/results/temporal-choice/YYYY-MM-DD/schedule.json`
- Create: `evaluation/results/temporal-choice/YYYY-MM-DD/run-records.jsonl`
- Create: `evaluation/results/temporal-choice/YYYY-MM-DD/prompts/`
- Create: `evaluation/results/temporal-choice/YYYY-MM-DD/artifacts/`

**Interfaces:**
- Use the exact fixed run schedule and three model families; sessions are independent and receive no other run's data.
- Ask rationale and solicited-recommendation questions only after the first artifact and first oracle run are saved. In assigned conditions, ask friction and optional future recommendation, not why the assigned language was chosen.
- If the app cannot launch an independent model family or preserve an independent session, mark that cell unavailable; do not simulate it with repeated turns from one context.

- [ ] **Step 1: Validate the ready-to-run manifest and schedule** against the dry-run evidence, prompt hashes, version pins, and 81-cell balance; stop before scoring if any contract differs from the approved protocol.
- [ ] **Step 2: Execute the discovery-choice cohort** in randomized order using fresh independent sessions; retain first implementation language, unsolicited recommendations, timeouts, and complete artifacts.
- [ ] **Step 3: Execute the named-option cohort** with randomized language-list order and fresh sessions; retain all first choices and later switches.
- [ ] **Step 4: Execute assigned controls** balanced across three languages and three model families; score with the same hidden oracle and record friction separately from choice.
- [ ] **Step 5: Ask post-task questions and obtain two independent rationale codes** only after first-run records are immutable; retain verbatim answers, coder disagreement, and reconciled labels.
- [ ] **Step 6: Reconcile every run ID** against the schedule; include incomplete or unavailable cells and state the actual denominator before drawing any conclusion.

### Task 5: Analyze, Report, and Make the Next Investment Decision

**Files:**
- Create: `evaluation/temporal_choice/analyze.py`
- Create: `evaluation/temporal_choice/tests/test_analysis.py`
- Create: `evaluation/results/temporal-choice/YYYY-MM-DD/report.json`
- Create: `evaluation/results/temporal-choice/YYYY-MM-DD/summary.md`

**Interfaces:**
- Report first runnable language, final language, unsolicited recommendation, solicited recommendation, and assigned-language correctness as separate outcomes.
- Show counts/denominators by condition, task, and model family; report exact/gap/fold, schema, source identity, error behavior, tool discovery, correction attempts, time to first passing output, and incomplete runs.
- Explain observed differences as exploratory associations, not causal proof or a population estimate.

- [ ] **Step 1: Write analysis tests** for denominator accounting, missing cells, language switches, distinct recommendation measures, assigned correctness, and rationale-code disagreements; confirm tests fail before implementation.
- [ ] **Step 2: Implement deterministic aggregation** from immutable run records; verify totals match the actual schedule and no run is silently excluded.
- [ ] **Step 3: Generate a concise plain-English report** with raw counts, uncertainty limitations, failures, blockers, and the strongest evidence-backed next investment among product capability, discoverability/distribution, or further study.
- [ ] **Step 4: Independently audit the claims against raw records and oracle results**; remove any claim that depends only on a self-report, a dry run, an incomplete run, or an unverified timezone comparison.
- [ ] **Step 5: Commit only the harness, fixtures, raw evidence, and report needed for reproducibility**; leave the north-star status as unproven unless repeated unassigned Guion use and unsolicited recommendation meet the protocol's evidence bar.

## Plan Self-Review

- **Protocol coverage:** The 81 planned runs, three conditions, three tasks, timezone parity gate, rationale method, recommendation separation, timeout retention, and report requirements each map to an owning task.
- **Step scan:** Oracle and harness checks precede scored work; dry runs are excluded; schedule and version manifest are frozen before execution.
- **Interface consistency:** The study's T1/T2 result contracts must match the implemented public Guion API and the approved protocol before prompts are released.
- **Review focus:** The main validity risks—prompt leakage, incomparable timezone data, shared agent context, self-report overclaiming, and missing-run denominator bias—have explicit checks.
- **Proportion:** The protocol remains a bounded 81-run exploratory study with a smaller-pilot fallback when independent model families are unavailable; it is not framed as proof of adoption.
