# Independent campaign v2

Status: preregistered; no formal choice or implementation sessions have been recorded. Pilot runs remain separate and do not count toward this campaign.

## Frozen inputs

- `schedule.json` assigns 18 language choices and 18 matched Python/Veritas build pairs. Its SHA-256 is recorded in the file and freeze record.
- `freeze-record.json` records the reviewed packet, pinned candidate package, images, and reviewer decision.
- `prerun-records.jsonl` binds the schedule and protocol before the first session.
- Follow the model, time limits, and tool-call limits in the frozen schedule. Run one candidate session at a time. Use the already-built, frozen images; do not rebuild them during the run. The Docker runner needs host socket access, so invoke it through the authorized host execution path on the first attempt rather than retrying once in a restricted shell.

## Where campaign records go

- Choice decisions: `recommendations/<decision-id>.json`. Save the exact prompt, response, and complete usage snapshot under `choice-evidence/<decision-id>/`.
- Build evidence: `run-evidence/<pair-id>/<session-id>/`, including a preserved candidate source tree, task inputs, outputs, runner reports, delivery artifact, and usage snapshot.
- Build ledger records: `../runs/<run-id>.json` under `evaluation/results/runs/`. Each record points to evidence by path relative to this campaign directory and includes its SHA-256.

Keep both languages in a pair on the same brief, model, host, budgets, and assigned order. Use distinct fresh assistant sessions. Keep choice sessions separate from build sessions. Record failed and incomplete attempts rather than dropping them.
