# Independent campaign v2

Status: frozen for artifact format 30. Eight formal implementation sessions were started: six completed their candidate work and are pending the official runner, one has an initial implementation awaiting follow-up, and one is recorded as in progress. No formal choice sessions or official runner results are recorded. The registry and raw session evidence are authoritative; these sessions remain bound to the frozen format-30 package and cannot count for a later candidate build.

## Frozen inputs

- `schedule.json` assigns 18 language choices and 18 matched Python/Veritas build pairs for artifact format 30. Its SHA-256 is recorded in the file and freeze record. Do not change the schedule or reuse its sessions for a different package identity.
- `freeze-record.json` records the reviewed packet, pinned candidate package, images, and reviewer decision.
- `prerun-records.jsonl` binds the schedule and protocol before the first session.
- Follow the model, time limits, and tool-call limits in the frozen schedule. Run one candidate session at a time. Use the already-built, frozen images; do not rebuild them during the run. The Docker runner needs host socket access, so invoke it through the authorized host execution path on the first attempt rather than retrying once in a restricted shell.

## Where campaign records go

- Choice decisions: `recommendations/<decision-id>.json`. Save the exact prompt, response, and complete usage snapshot under `choice-evidence/<decision-id>/`.
- Build evidence: `run-evidence/<pair-id>/<session-id>/`, including a preserved candidate source tree, task inputs, outputs, runner reports, delivery artifact, and usage snapshot.
- Build ledger records: `../runs/<run-id>.json` under `evaluation/results/runs/`. Each record points to evidence by path relative to this campaign directory and includes its SHA-256.

Keep both languages in a pair on the same brief, model, host, budgets, and assigned order. Use distinct fresh assistant sessions. Keep choice sessions separate from build sessions. Record failed and incomplete attempts rather than dropping them.

## Evidence archive and workspace hygiene

The preserved raw `run-evidence/` tree is stored in `run-evidence/evidence.tar.zst` to keep the active workspace smaller. The archive contains the complete original tree: 52,067 tar entries, including 46,260 regular files, with 2.39 GiB uncompressed. Its SHA-256 is `c458124b129e86075db573f015c693c773ddd4d2025655c4b66538ebf3b30a07`. Validate it with `zstd -t run-evidence/evidence.tar.zst`. From this directory, restore the original tree with `zstd -dc run-evidence/evidence.tar.zst | tar -xf -`. The archive remains under the existing Git-ignored evidence path. New campaign outputs may be added beside it; do not overwrite the archive.
