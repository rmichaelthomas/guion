You are advising a developer choosing a language for the attached task.
Do not implement the task, browse, use tools, or infer results from code that has not been built.
Choose exactly one: Guion, Python, or either. Explain the decisive task-specific tradeoffs and what would change your choice.
In the product-information arm use only the supplied profiles. In the cold arm no product documentation is supplied; use only prior knowledge and the task brief.
Do not make a universal language-replacement claim.

Assignment ID: 01-support-sla-report-choice-1
Schedule SHA-256: 6a070a9801ffb2479d16ab2b3ddd9769cd74235050d8c48a03a09bf77dc9c8dd
Schedule file SHA-256: d3ff11c45c77160349cc998ba9db7ca85f28734a1137ec5cf9a7e78cb9fdee9a
Choice-safe bundle SHA-256: 4f53cda18c2baa0c0354bb5f9a3ecbe5ed12ab4d8e11ba873c2f11161202b945
Presentation order: Guion, Python
Configured assistant: Google Gemini gemini-4-argon (gemini-4-argon); reasoning effort provider-default
Budget: 5 active minutes and 0 tool calls.
No implementation results, benchmarks, author calibration, or candidate runs have been shown.

# Task brief
# Support ticket SLA report

**Class:** small automation or data CLI
**Origin:** independent-candidate brief draft; revised after overlap audit
**Initial request:** Build a CLI invoked as `sla-report --tickets PATH --policy PATH --as-of RFC3339 --output PATH`. RFC3339 timestamps use the exact `YYYY-MM-DDTHH:MM:SS[.fraction](Z|±HH:MM)` form with a valid Gregorian date, seconds from 00 through 59, a timezone offset, and (when present) one to six fractional digits; fractions longer than six digits are invalid. Apply the same timestamp syntax to `created_at`. RFC3339 timestamps use the exact `YYYY-MM-DDTHH:MM:SS[.fraction](Z|±HH:MM)` form with a valid Gregorian date, seconds from 00 through 59, a timezone offset, and (when present) one to six fractional digits. Fractions longer than six digits are invalid. The CSV header must be exactly `id,team,priority,status,created_at`; every data row must contain exactly five non-empty fields, and blank rows are invalid. The policy JSON must be an object containing a `response_minutes` object. Its only allowed keys are `high` and `low`; either may be omitted. Values that are present must be nonnegative JSON integers (Booleans are not integers). An unknown key, invalid value, or malformed policy is an actionable error and must leave an existing output untouched. The example policy is `{"response_minutes":{"high":60,"low":120}}`. The only accepted priorities are `high` and `low`; reject another value as `unknown_priority`. If a recognized priority has no `response_minutes` entry, reject it with the diagnostic code `missing_policy_entry`. Accept only `open` and `closed` statuses. Convert both timestamps to UTC and compute `age_minutes` as floor of elapsed seconds divided by 60; a `created_at` later than `--as-of` is invalid. Emit UTF-8 JSON with `as_of`, a `tickets` array sorted by team then ID, and a `summary` object containing total and breached counts. Each ticket has its id/team/priority/status, integer `age_minutes`, integer `limit_minutes`, and Boolean `breached`. An open ticket is breached only when its age is strictly greater than its priority limit; closed tickets are never breached. Reject malformed rows, duplicate IDs, unknown statuses, invalid timestamps, and missing policy entries without replacing an existing output.

**Follow-up change:** Add `--team NAME`, filtering the ticket array and summary to that team without changing the default result.

**Acceptance:** Exact output shape and stable order; a ticket exactly at its threshold is not breached; closed-ticket behavior; malformed/duplicate inputs; unknown priority; missing policy; output remains intact on failure; team match/miss; clean-workspace rerun.

**Fairness notes:** Python may use its standard library. All timestamps in the initial fixture use UTC so the task tests age and policy behavior rather than locale configuration. This brief replaces the earlier inventory-variance draft because the existing inventory-reconciliation example made that task too familiar.

# Option 1: Guion
No product profile or documentation is supplied for this option.

# Option 2: Python
No product profile or documentation is supplied for this option.
