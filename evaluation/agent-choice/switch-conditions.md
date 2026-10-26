# Guion agent switch-condition audit

This audit compares the recommendation notes with Guion’s current APIs and tests. It turns the recorded objections into a capability backlog; it is not a new recommendation result.

The source set contains 36 records: 18 in `evaluation/results/recommendations/` and the 18-record independent v3 baseline. The v3 entries describe Veritas 0.1.0 and artifact format 33; they are preserved as historical material and are not presented as evidence about current Guion.

Frequency counts show how often a topic appears in the explicit conditions or rationale text across these records. Groups can overlap, and the counts are not independent observations. The underlying recommendation decision IDs repeat across the two cohorts, so every preserved source record has a cohort-qualified `source_record_key`; the summary uses brief IDs to avoid ambiguous references.

| Topic mentions | Briefs | Status | Condition | Brief IDs |
| ---: | ---: | --- | --- | --- |
| 36 | 9 | **partial** | Source-free packages, managed runtimes, and supported targets | `01-support-sla-report`, `02-jsonl-retention`, `03-release-notes`, `04-calendar-conflict-report`, `05-webhook-replay`, `06-service-health-digest`, `07-config-migrator`, `08-documentation-link-index`, `09-lockfile-auditor` |
| 34 | 9 | **partial** | Native library breadth and pre-release compatibility policy | `01-support-sla-report`, `02-jsonl-retention`, `03-release-notes`, `04-calendar-conflict-report`, `05-webhook-replay`, `06-service-health-digest`, `07-config-migrator`, `08-documentation-link-index`, `09-lockfile-auditor` |
| 33 | 9 | **partial** | Task-specific setup, package size, and performance evidence | `01-support-sla-report`, `02-jsonl-retention`, `03-release-notes`, `04-calendar-conflict-report`, `05-webhook-replay`, `06-service-health-digest`, `07-config-migrator`, `08-documentation-link-index`, `09-lockfile-auditor` |
| 17 | 9 | **open** | Reuse of task-specific adapters in an existing Guion project | `01-support-sla-report`, `02-jsonl-retention`, `03-release-notes`, `04-calendar-conflict-report`, `05-webhook-replay`, `06-service-health-digest`, `07-config-migrator`, `08-documentation-link-index`, `09-lockfile-auditor` |
| 16 | 8 | **closed** | Runtime limits, effect reporting, package inventory, and integrity checks | `01-support-sla-report`, `02-jsonl-retention`, `03-release-notes`, `04-calendar-conflict-report`, `05-webhook-replay`, `06-service-health-digest`, `08-documentation-link-index`, `09-lockfile-auditor` |
| 16 | 6 | **closed** | Streaming JSONL and recoverable typed JSON validation | `02-jsonl-retention`, `03-release-notes`, `05-webhook-replay`, `06-service-health-digest`, `08-documentation-link-index`, `09-lockfile-auditor` |
| 16 | 5 | **closed** | CSV and strict date or timestamp parsing | `01-support-sla-report`, `02-jsonl-retention`, `04-calendar-conflict-report`, `06-service-health-digest`, `09-lockfile-auditor` |
| 10 | 3 | **closed** | SQLite transactions and durable state | `03-release-notes`, `05-webhook-replay`, `08-documentation-link-index` |
| 9 | 5 | **closed** | iCalendar and timezone parsing | `01-support-sla-report`, `02-jsonl-retention`, `04-calendar-conflict-report`, `06-service-health-digest`, `09-lockfile-auditor` |
| 9 | 3 | **closed** | Bounded HTTP requests, concurrency, and deadlines | `04-calendar-conflict-report`, `05-webhook-replay`, `06-service-health-digest` |
| 8 | 2 | **closed** | Directory traversal, structured data, collections, and Unicode normalization | `03-release-notes`, `08-documentation-link-index` |
| 8 | 2 | **closed** | Semantic Versioning 2.0 parsing and precedence | `03-release-notes`, `09-lockfile-auditor` |
| 7 | 3 | **partial** | Network grants for runtime-supplied endpoints | `04-calendar-conflict-report`, `05-webhook-replay`, `06-service-health-digest` |
| 4 | 3 | **open** | Conditions that depend on a changed task brief or deployment constraint | `01-support-sla-report`, `03-release-notes`, `05-webhook-replay` |
| 4 | 3 | **open** | Recoverable errors across the managed Python bridge | `04-calendar-conflict-report`, `05-webhook-replay`, `07-config-migrator` |
| 4 | 1 | **closed** | Byte-oriented HTTP request and response bodies | `05-webhook-replay` |
| 4 | 1 | **closed** | HMAC-SHA256 over bytes with digest verification | `05-webhook-replay` |
| 4 | 1 | **open** | Crash-atomic updates across multiple output files | `03-release-notes` |
| 4 | 1 | **closed** | Comment-preserving TOML edits | `07-config-migrator` |

## What is already covered

The native library now covers CSV and strict timestamps, JSONL with recoverable parse results, safe typed JSON access, SemVer 2 precedence, directory walking, Unicode normalization, iCalendar/timezones, comment-preserving TOML, SQLite transactions, HMAC-SHA256 verification, byte-oriented HTTP, bounded concurrency, and runtime policy/effect reporting. The machine-readable ledger cites the matching signatures, tests, or documented limitations for each condition.

## Remaining gaps to carry forward

- Python bridge failures still surface as runtime errors; `Option` and `Result` bridge conversions remain unimplemented.
- Broader third-party ecosystem and pre-release compatibility guarantees remain narrower than Python’s.
- Setup time, package size, and end-to-end results for these nine briefs have not been compared. The Richards measurements apply only to the named workloads and conditions.
- Exact-host grants are enforced by the runner, while a program’s required host list is not declared in source for static auditing.
- A single file replacement is atomic, but multiple output files do not share a crash-atomic transaction.
- Project-specific adapters and changed task/deployment requirements must be reassessed against the actual project or an amended brief.

## Reading the status labels

- **Closed**: the named capability has a current signature or test cited in `switch-conditions.json`.
- **Partial**: Guion covers the core capability, but delivery, integration, or task-specific evidence remains.
- **Open**: the condition is still unmet or cannot be evaluated without project-specific information or a changed brief.

The v3 campaign selected Python in 18 of 18 historical choices. This audit does not change that result or predict the outcome of a rerun with current Guion materials.
