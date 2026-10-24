# Veritas recommendation evaluation protocol

Status: final campaign protocol prepared for independent re-review. No formal independent runs are represented as completed. Pilot dry runs are stored under `evaluation/results/pilot/` and are excluded from the 18-pair sample and recommendation evidence.

## Direct recommendation decisions

The north star is that an assistant recommends Veritas when it is a better fit for a real task. Build success does not measure that choice, so record it separately from matched implementation runs.

All 18 choice sessions and 36 implementation sessions (54 sessions total) use the campaign model declared in `evaluation/protocol/campaign-config.json`: OpenAI Codex `gpt-6.1-sol`, reasoning effort `medium`. This is the host-exposed model selector alias; the host does not provide an immutable backend snapshot ID. A choice session has a five-minute active-time limit and zero tool calls. Each implementation session has a 45-minute active-time limit and 60 candidate-session tool calls, whichever comes first. The pre-run schedule and record bind the campaign configuration digest. Do not combine sessions from another model alias, reasoning effort, or budget into this cohort.

For each of the nine frozen briefs, collect two choices in distinct fresh assistant sessions (18 decisions total). Provide the same task brief and frozen choice-safe product-information bundle in each session. The choice bundle may contain only product documentation, declared tooling/version facts, and the task brief; it must exclude candidate-run, calibration, benchmark, and comparative outcome material. Record its manifest digest in every choice record and require an exact match. Randomize language order independently with `secrets.choice` and save the complete assignment file and its SHA-256 before opening any choice sessions. Do not show comparative pair results before the choice. Ask the assistant to choose `veritas`, `python`, or `either`, state its reasoning from the supplied evidence, and name any conditions that would change its choice. Do not ask it to implement the task in this session.

Generate and save the full randomized assignment before opening any candidate session:

```sh
python3 evaluation/protocol/schedule.py evaluation/fixtures/independent-v1/manifest.json evaluation/review/independent-v1/product-information/bundle-manifest.json evaluation/protocol/campaign-config.json evaluation/results/independent-v2/schedule.json
```

The schedule records the model, budgets, product-information digest, and randomized order for all 18 choices and 18 matched implementation pairs. It is immutable once generated. Render each choice prompt with the frozen schedule entry and verified bundle; the renderer places identical Python and Veritas profile content in the assigned order and saves the exact prompt transcript. Append the timestamped pre-run record before opening any choice session. Each choice record includes its own `recorded_at_utc`, prompt and response transcript paths and digests, the exact `prerun_records_sha256`, and the pre-run record's timestamp. The validator checks these bindings and requires the choice timestamp to be later than the pre-run record.

Save each decision as one JSON record under a new ID and keep its prompt transcript beside it. Validate and summarize with:

```sh
python3 evaluation/protocol/recommendations.py validate evaluation/results/independent-v2/recommendations/<decision-id>.json --schedule evaluation/results/independent-v2/schedule.json --prerun-records evaluation/results/independent-v2/prerun-records.jsonl --evidence-root evaluation/results/independent-v2/choice-evidence
python3 evaluation/protocol/recommendations.py summarize evaluation/results/independent-v2/recommendations --schedule evaluation/results/independent-v2/schedule.json --prerun-records evaluation/results/independent-v2/prerun-records.jsonl --evidence-root evaluation/results/independent-v2/choice-evidence
```

The record includes the brief digest, model and fresh-session ID, randomized presentation order, product versions, documentation-bundle digest, decision, rationale, pre-run-file digest, pre-run timestamp, choice timestamp, and whether comparative results had been shown. Save the complete `read_thread` JSON payload as `usage_snapshot_path`; set `turnLimit` high enough that `page.hasMore` is false and `page.nextCursor` is null. `usage_snapshot_sha256` binds the exact file. `usage.active_seconds` is the rounded-up sum of terminal turn `durationMs` values, and `usage.tool_calls` counts unique `commandExecution` and `functionCallOutput` item IDs, excluding coordinator `create_thread` calls. Missing turns/items, incomplete pagination, and unknown item types fail closed. The validator recomputes both values, checks the snapshot session ID, verifies all transcript and usage hashes under `--evidence-root`, and rejects a session that exceeds either cap. Reusing a session, recording a choice before the pre-run record, or exposing comparative outcomes invalidates the record. The summary reports per-class choices and requires six decisions across three briefs per class for the complete 18-decision sample. A Veritas majority in at least two classes is a direct recommendation signal, not a recommendation verdict by itself: the matched implementation cohort must independently pass its correctness, delivery, and practical-benefit rules.

## Recording and checking runs

Save each attempt as one JSON file under a new run ID. Keep prompt, test-input, output, command-transcript, and complete raw `read_thread` usage files under the evidence root; store each relative path and SHA-256 digest in `evidence`. Each initial and follow-up stage also records its `run-case.sh` report path and digest. The report binds pair, stage, language, candidate version, session, task, manifest and schedule digests, Docker Engine ID, delivery manifest, and relocated artifact hash. The ledger verifies every file digest, rejects paths that escape the evidence root, recomputes usage from the thread snapshot, and recomputes the delivery tree hash against the runner report. Reported runner success must agree with both source/delivery outcomes and the stage status in the run record. A failed or incomplete attempt remains in the ledger. Mark an attempt excluded only with a concrete reason; summaries show it among valid records but do not count it as recommendation evidence.

Use the standard-library-only ledger tool:

```sh
python3 evaluation/protocol/ledger.py validate evaluation/results/runs/<run-id>.json \
  --schedule evaluation/results/independent-v2/schedule.json \
  --prerun-records evaluation/results/independent-v2/prerun-records.jsonl \
  --evidence-root evaluation/results/independent-v2
python3 evaluation/protocol/ledger.py summarize evaluation/results/independent-v2/runs \
  --schedule evaluation/results/independent-v2/schedule.json \
  --prerun-records evaluation/results/independent-v2/prerun-records.jsonl \
  --evidence-root evaluation/results/independent-v2
```

Each record captures the task origin and class, candidate language/version, model/version and unique fresh-session ID, host and hardware, network mode, randomized pair order, equal time/tool budgets, timestamps, initial and follow-up acceptance outcomes, clean-workspace and hidden-Python guardrails, actual active seconds and tool calls, setup/warm costs, interventions, commands, evidence paths/digests, reviewer verdict, and any exclusion reason. Independent run records also bind the assigned `session_slot`, frozen schedule and manifest digests, schedule-file digest, pre-run-records file digest, and predeclared primary measure; validation and summary require the matching schedule and pre-run record. `costs.active_seconds` is the rounded-up total of terminal Codex turn durations in the saved usage snapshot; `costs.tool_calls` is recomputed from the same snapshot. Both are checked against the frozen caps. A passed stage must have at least one acceptance case and all cases passing.

For each of the nine briefs, run exactly two independent matched implementation pairs. Each pair consists of one fresh Veritas session and one fresh Python session using the same model version, task digest, order, host, and budgets. All four sessions across the two pairs for a brief must have distinct session IDs; the ledger rejects reuse across either language or pair. This yields 18 pairs and 36 candidate runs. Separately collect two fresh no-build recommendation choices per brief, yielding 18 choice records. Do not combine choice records with implementation pairs. “Two fresh model sessions per language/brief pair” means two matched pairs, not two total sessions.

Generate the schedule using `secrets.choice` once per pair, copy that order to both sessions in the pair, and save the complete assignment file and its digest before runs begin. Count a human intervention whenever the user or evaluator supplies new task-specific information, edits candidate code/configuration, or repairs commands; routine assistant tool execution is not an intervention. Record each intervention with timestamp and description. Active time excludes only documented pauses where no candidate work occurs; retain start/end and pause intervals.

The summarizer pairs only one Veritas run with one Python run that share the task ID/class/brief digest, model version, randomized order, OS/architecture/CPU/network mode, and budget; the runs must have different session IDs and unique run IDs. It reports incomplete or mismatched pairs rather than dropping them. Known and calibration runs stay visible but are excluded from independent evidence. Per-pair reductions and per-class medians are calculated only when both candidates pass initial and follow-up acceptance, clean-workspace delivery, no-hidden-system-Python, and no-severe-regression guardrails. A reduction is undefined when Python's comparison value is zero; that pair cannot count as a 20% reduction for that metric. The `recommendation_gate` field reports only whether the minimum evidence sample is present (three classes, at least three distinct briefs and six pairs in each); it never decides which language to recommend.

The installed Veritas runtime distribution includes the evaluation protocol for reference, but it is not a standalone campaign kit. `evaluation/sandbox/build-images.sh` requires the campaign lock and pinned Linux archive held in the complete source checkout or review packet. The candidate archive omits the lock because the lock contains that archive's own digest.

The run ledger and direct-choice ledger check record structure and their respective pairing/session rules. Evaluators must preserve the referenced files and review that hashes match their contents; neither tool cryptographically signs records or certifies reviewer independence.

## Decision being tested

When should an assistant recommend Veritas rather than Python to a builder who wants generated programs to be easy to run, maintain and deliver? The test must allow a Python recommendation. “The Veritas app ran” is not the decision criterion.

## Comparison unit

Use a task brief plus one initial implementation and one changed-requirement follow-up. A pair is run in fresh, equivalent contexts by the same model version and tool budget. Randomize which language is attempted first. Freeze Veritas version, bundled docs and Python tooling before recruiting runs. Both candidates use the campaign-pinned CPython 3.14.8 runtime and uv 0.13.4 on the same Debian 12 ARM64 image; Python uses its locked `uv` project and documented entry command, with equivalent HTTP/file permissions. Veritas pays its sync and compile/build costs; Python pays its environment/setup costs. Do not charge Veritas compiler development to the task.

Keep one known development case (feed digest) as an integration regression. Do not count it toward independent recommendation evidence. Include previously known sales aggregation only as a calibration case, not as unseen task evidence.

The independent-kit manifest carries the exact file contents, argv arrays, setup/mutation sequence, expected exit codes and output predicates for each scored stage. `case_runner.py` executes one candidate command without a shell in a fresh working directory and can start deterministic loopback HTTP fixtures. Project-scoped runtimes may opt into workspace-root candidate staging so their file-access root matches the workspace used for fixtures. For scored delivery, each candidate project must include `delivery.json` with documented launch argv, artifact paths, source paths, and dependency-lock path. The campaign launcher first runs the source project, then stages declared artifacts on the host and starts a second restricted container that mounts only those artifacts. That second container receives the same manifest, case inputs, fixture network, candidate UID, and resource limits; the original project is not mounted or accessible. It executes the declared launch argv from the relocated directory. The same acceptance cases apply to both languages. Python uses `uv run --locked --offline` with its `pyproject.toml` and `uv.lock`; Veritas uses its project-aware `veritas run` command and declared `veritas.lock`. Environment synchronization and cold/warm costs are recorded separately. Dependencies must resolve from the shared offline wheelhouse or be explicitly disallowed for both candidates. The runner records artifact identity, relocation result, per-step/overall elapsed time, checks, fixture requests and concurrency. The runner is not an operating-system sandbox: campaign candidates must execute in equivalent restricted containers with matching filesystem, network and resource permissions. The current 127 cases and expected values are frozen after independent review; runner acceptance proves only that the case is executable, not that the task or oracle is fair or correct.

## Task classes and sampling

The first independent cohort should use three task classes that exercise distinct reasons to choose a language:

1. **Small automation or data CLI:** structured input, validation, transformation, useful diagnostics, and a changed input or output rule.
2. **Concurrent network or package integration:** multiple remote or mocked requests, rate/host limits, a real ecosystem library, partial failure, and a follow-up API change.
3. **Maintainable application change:** a multi-module program with a changed requirement, explicit error behavior, and a deliverable another person can run.

For each of the nine briefs, run exactly two independent matched implementation pairs. Each pair contains one fresh Veritas session and one fresh Python session using the same model version, brief digest, randomized order, Docker engine host ID, and budgets; the two sessions have distinct IDs. The campaign launcher records the Docker Engine ID from `docker info` in each host report. Copy that value into `environment.host_id` in the run ledger; the ledger requires it and rejects a pair if the IDs differ. The two pairs for a brief use four distinct session IDs. This yields 18 pairs and 36 candidate runs. Separately collect two fresh no-build language choices per brief, yielding 18 choices; do not combine choice sessions with implementation pairs. This exploratory cohort cannot justify a universal claim. The evaluator must not have authored the candidate task or implementation. If there are insufficient builders or sessions, publish the kit and leave the evidence gate open rather than replace independent samples with more known fixtures.

## Primary and guardrail measures

Record per candidate and per run:

- correct acceptance cases on the first complete implementation and after the follow-up;
- elapsed active time, cold setup time, warm rerun time, and human interventions separately;
- setup/build/run/test commands, dependency and artifact sizes, and whether another clean workspace can run the result;
- maintenance edits, workaround code, generated-code repairs, and unresolved issues;
- model name/version, prompt, tools, tool budget, attempts, machine/runtime, and exclusions.

Primary outcome is **successful completion of both the initial brief and its follow-up under the same task budget**. Before the first independent run, designate active seconds as the primary practical-benefit measure and human interventions as secondary, or designate interventions as primary and active seconds as secondary; do not switch after seeing outcomes. Correctness, source-free relocated delivery, and no hidden system-Python dependency are guardrails. Report every run and paired differences; do not average away a failure.

## Recommendation rule

An assistant may recommend Veritas for a task class only when both independent evidence signals qualify for that class. The direct-choice signal requires at least four of six valid no-build choices in that class to select Veritas. The implementation signal requires all six assigned pairs across three briefs (two pairs per brief) to be reported, at least five of six initial-plus-follow-up primary successes for **each** language, and at least five of six pairs passing correctness, source-free delivery, and no-hidden-runtime guardrails. On those qualifying pairs, Veritas must have a median reduction of at least 20% in the predeclared primary measure and at least four of the six assigned pairs must favor Veritas by 20% or more; no lower Veritas completion rate is allowed. A guardrail-failing pair cannot count as favorable. When interventions are primary and Python has zero interventions in a pair, that pair has no percentage reduction and cannot count as favorable. A severe Veritas-only correctness, data-loss, privacy, or hidden-runtime failure triggers a stop: do not launch further Veritas runs in that class. Append a `not_started` event for every halted session to `campaign-stop-log.jsonl`, recording pair ID, session slot, UTC timestamp, triggering run ID, and reason; preserve the frozen schedule and do not synthesize a run record. Disqualify that class for this campaign. A class where the choice and implementation signals disagree does not qualify. A cross-class Veritas recommendation requires both signals to qualify in at least two of the three classes and no severe regression in the remaining class. A severe regression is a critical correctness, data-loss, privacy, or hidden-runtime failure; lower Veritas completion; or a remaining-class median at least 20% worse on the primary measure. Publish all pair values, spreads, failures, exclusions, direct choices, and the secondary measure. Otherwise report Python or insufficient evidence with the task-specific reason. One class-specific recommendation never becomes a claim that Veritas replaces Python generally.

The initial sample supports only an exploratory, task-class verdict for the tested assistant/model version, ARM64 Linux campaign image, three selected briefs per class, and supplied product bundle. It cannot support a universal recommendation across models, hosts, workloads, or developers. Two no-build choices per brief are a small, model-specific descriptive sample; a majority is a direction signal, not a population estimate. Before independent runs begin, a second evaluator must review the final task materials, outcome definitions, and meaningful-effect rule, and record approval before unblinding results. If task budgets or machine availability force changes, amend the protocol before collecting those results.

The exact Python base, Veritas archive, dependency bundle, uv version, and platform are pinned in `evaluation/sandbox/campaign-lock.json` in the review kit. The candidate archive deliberately omits this lock because it contains the archive's own digest; embedding it would create an impossible self-referential hash. The reviewer packet keeps the lock beside the archive and verifies their digest relationship. After freezing the reviewed inputs and generating the randomized schedule, append a timestamped pre-run record before starting any choice or build session:

```sh
python3 evaluation/protocol/record_prerun.py \
  --freeze evaluation/results/independent-v2/freeze-record.json \
  --schedule evaluation/results/independent-v2/schedule.json \
  --campaign-lock evaluation/sandbox/campaign-lock.json \
  --primary-measure active_seconds \
  --output evaluation/results/independent-v2/prerun-records.jsonl
```

The record binds the schedule, freeze, campaign configuration and digest, product bundle, campaign lock, expected Veritas archive digest, and the primary practical-benefit measure chosen before any run. The tool refuses a stale or tampered schedule, mismatched freeze or campaign digests, an invalid primary measure, or a duplicate schedule record. Each build record must carry its assigned `session_slot`, frozen model and budget, schedule digest, schedule-file digest, pre-run-records digest, primary measure, and the `docker_engine_id` copied as `environment.host_id` from its host run report; the ledger validates these before counting a pair and requires both members to share the same host ID.

## Required artifacts per run

Store an immutable task brief, exact candidate versions/tooling, raw timestamps and command transcript, test inputs/outputs, human interventions, reviewer verdict, and a machine-readable summary under a new run ID. Preserve failed and excluded attempts with reasons. Keep known development fixtures under `evaluation/development/` and never relabel them as independent.
