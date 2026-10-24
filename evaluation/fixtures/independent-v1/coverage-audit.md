# Case coverage audit

Status: author coverage audit frozen after independent review v17 and correction of its sole required packaging assertion. This maps promised behaviors to observably distinct fixture conditions; the v17 reviewer approved all nine task specifications and the comparison protocol but did not execute candidates.

The manifest contains 127 executable cases. Case count alone is not coverage. Each promised behavior must have a valid fixture and an assertion that distinguishes it from adjacent conditions.

| Task | Review correction and revised coverage |
|---|---|
| 01 Support SLA report | Policy values are nonnegative JSON integers, Booleans are rejected, unknown keys/statuses are rejected, future ticket timestamps and strict RFC3339 space-separator forms fail, and failures preserve the output. The zero-match team filter remains explicit. |
| 02 JSONL event summary | RFC3339 syntax is enforced instead of permissive ISO parsing; malformed timestamps are line-numbered in normal mode and preserve output in strict mode. Physical line numbers include blanks; follow-up covers a nonempty allow-list with no matches. |
| 03 Release notes | Config requires a non-empty category order. Config, change-record, date, filename SemVer, exact fields, and directory-wide ID uniqueness are defined. `--since` filters rendering only: excluded malformed records and duplicate IDs across the boundary still fail without changing either output. Punctuation escaping covers all 32 ASCII punctuation characters. |
| 04 Calendar conflicts | Timed events require a nonempty UID and timezone-aware instants; RFC3339 syntax is strict; floating and unknown-zone events are invalid. Three simultaneously overlapping events assert all pairwise conflicts. A TZID fixture verifies UTC conversion; distinct fixtures reject floating times, unknown TZIDs, missing UIDs, reversed intervals, over-precision windows, and a space separator; slow-trickle responses prove a total wall-clock deadline. |
| 05 Webhook replay | Init rejects empty and invalid-UTF-8 secrets without creating or mutating a database. Raw secrets and digests are absent from persistent state. Uppercase valid HMAC hex is accepted. Retry, attempt boundaries, and dead-letter privacy are defined; null payload is accepted. |
| 06 Service health digest | Duplicate names are rejected. `--now` is required when maintenance is configured. Invalid maintenance RFC3339 timestamps and space separators fail while preserving both outputs; cases also cover timeout preservation, invalid 2xx shapes inside maintenance, and slow-trickle responses that exceed a total wall-clock deadline. |
| 07 Config migrator | `check` validates schema 1, 2, and 3 requirements. Schema 2 requires positive integer `timeout_seconds`; `parallelism` is optional and valid when absent. If absent, `check` succeeds without changing source bytes and migration to schema 3 adds `parallelism = 4`. If present, it must be a positive integer; zero and Boolean `true` fail in both `check` and `migrate` without changing source bytes. Migration advances one version; defaults and explicit values are covered. |
| 08 Documentation link index | Backlinks use exact normalized target text even for ambiguous targets. The normalized-path UTF-8 tie-break is asserted in both `check` and `backlinks`. Same-page title/alias matches count once; unrelated front-matter fields are ignored. `aliases:` text is ignored when front matter is absent or unclosed. Valid front matter is excluded from heading and link extraction; output ordering uses normalized relative paths and titles. Rebuilds remove deleted documents and stale links. |
| 09 Lockfile auditor | Initial cases omit `--as-of`; an explicitly blank value fails without replacing the report. Inclusive minimum and maximum boundaries are asserted. Optional policy defaults, repeated per-entry diagnostics, valid, on-expiry, and expired exception reporting, duplicate-exception rejection without replacing the report, and invalid/missing dates remain covered. |

## Required pre-freeze work

1. Author calibration must pass all 127 cases with pinned CPython 3.14.8; preserve its manifest, candidate, and dependency hashes. This is calibration only.
2. Verify source-free delivery independently for both candidate configurations. The delivery runner must use the same acceptance cases, inputs, fixture network, and permissions after staging only declared artifacts and the dependency lock; reject lexical parent traversal in declared paths.
3. Recheck task overlap: `webhook-replay` shares persisted retry/restart behavior with `api_sync`; its scored distinction is signature validation, idempotent ingestion, attempt boundaries, and dead-letter privacy.
4. Recompute all packet hashes and the product-information bundle digest. Freeze only after an independent evaluator accepts the final hash identities; then create the randomized schedule, choose the primary practical-benefit measure, and append their timestamped identities before any formal session.

The matrix exercises major workflow paths, failure preservation, loopback timeouts, measured concurrency, symlink boundaries, and a 500-package traversal. Pilot findings prompted scope, output-shape, exit-status, backlink, diagnostic-oracle, privacy-state, and schema-validation clarifications. Those pilots are not formal campaign evidence. The independent reviewer must still confirm every acceptance claim and that delivery is equivalent.

## Author calibration

The Python reference is rerun against the revised manifest. Its iCalendar dependency is pinned in `requirements.txt`. The report binds the run to current manifest, candidate, and dependency hashes. This reference is authored in the implementation workspace and does not count as an independent solution or reviewer result.
