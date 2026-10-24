# Candidate workload overlap audit

Status: author revision after independent review; external evaluator review is still required.

The first brief set was compared against the known development workloads shipped in this repository before independent runs were proposed. Three drafts were replaced because of close semantic overlap:

| Replaced draft | Known workload overlap | Replacement |
|---|---|---|
| Inventory variance | `inventory_reconcile` already compares inventory records and tests validation and deterministic output. | Support ticket SLA report with explicit time-window and threshold semantics. |
| Paginated incident export | `api_sync` already covers pagination, retry, cursor persistence, and restart recovery. | Multi-calendar conflict report using iCalendar parsing and overlapping event intervals. |
| Local issue queue | `task_ledger` already covers persistent task creation, listing, completion, and removal. | Local documentation link index with title resolution, backlinks, aliases, and an SQLite migration. |

Residual similarities:

- Service health and calendar conflict tasks both issue concurrent HTTP requests, as does `feed_digest`; their parsing and decisions differ, but the external evaluator must decide whether both are useful distinct workloads.
- JSONL event summary uses local structured data and deterministic aggregation, which is common CLI work; UTC grouping, malformed-line behavior, and filtering are the specific distinctions.
- Release-note assembly and lockfile auditing have no close match in the current development workload inventory.
- `webhook-replay` overlaps `api_sync` in persisted retry state and restart recovery. Retain it only for its directly scored distinctions: exact-byte signature verification, idempotent ingestion, persisted attempt boundaries, and dead-letter privacy. Generic retry/restart is not counted as independent novelty.

This is a source-repository overlap check, not proof that no external task benchmark or prior prompt has used similar requirements. The external reviewer must judge whether the tasks remain sufficiently distinct and answerable.
