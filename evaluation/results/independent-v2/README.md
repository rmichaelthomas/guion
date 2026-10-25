# Independent campaign v2

Status: frozen for artifact format 30. Eight formal implementation sessions were started: six completed their candidate work and are pending the official runner, one has an initial implementation awaiting follow-up, and one is recorded as in progress. No formal choice sessions or official runner results are recorded. The retained schedule, freeze records, setup attempts, and partial session summaries are the available authority; the raw session archive has since been removed, so these sessions cannot be replayed byte for byte. They remain bound to the frozen format-30 package and cannot count for a later candidate build.

## Frozen inputs

- `schedule.json` assigns 18 language choices and 18 matched Python/Veritas build pairs for artifact format 30. Its SHA-256 is recorded in the file and freeze record. Do not change the schedule or reuse its sessions for a different package identity.
- `freeze-record.json` records the reviewed packet, pinned candidate package, images, and reviewer decision.
- `prerun-records.jsonl` binds the schedule and protocol before the first session.
- Follow the model, time limits, and tool-call limits in the frozen schedule. Run one candidate session at a time. Use the already-built, frozen images; do not rebuild them during the run. The Docker runner needs host socket access, so invoke it through the authorized host execution path on the first attempt rather than retrying once in a restricted shell.

## Where campaign records go

- Choice decisions: `recommendations/<decision-id>.json`. Save the exact prompt, response, and complete usage snapshot under `choice-evidence/<decision-id>/`.
- Historical build-evidence layout: `run-evidence/<pair-id>/<session-id>/`, which formerly held candidate sources, task inputs, outputs, runner reports, delivery artifacts, and usage snapshots. The raw evidence is no longer present; see the migration note below.
- Build ledger records: `../runs/<run-id>.json` under `evaluation/results/runs/`. Each record points to evidence by path relative to this campaign directory and includes its SHA-256.

Keep both languages in a pair on the same brief, model, host, budgets, and assigned order. Use distinct fresh assistant sessions. Keep choice sessions separate from build sessions. Record failed and incomplete attempts rather than dropping them.

## Evidence archive and workspace hygiene

At the frozen v2 checkpoint, `run-evidence/evidence.tar.zst` contained 52,067 tar entries (46,260 regular files, 2.39 GiB uncompressed), with SHA-256 `c458124b129e86075db573f015c693c773ddd4d2025655c4b66538ebf3b30a07`. The archive passed `zstd -t` before migration cleanup. On 2026-10-25, the 1.17 GB archive and 950 MB generated workspace were removed to reclaim disk space. They are not available to restore from this checkout, and the hash is an identifier, not a backup.

The small retained subset of prompts, schedules, setup records, source-information snapshots, and pinned inputs is in [`preserved-inputs/`](preserved-inputs/README.md). The cleanup removes exact byte-for-byte replay of the eight partial format-30 implementation sessions. No official choice outcomes or runner results were recorded. See [`../../docs/history/migration-2026-10-25.md`](../../docs/history/migration-2026-10-25.md) and the retention manifest for the complete disposition.
