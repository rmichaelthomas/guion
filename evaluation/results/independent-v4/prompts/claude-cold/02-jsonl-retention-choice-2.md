You are advising a developer choosing a language for the attached task.
Do not implement the task, browse, use tools, or infer results from code that has not been built.
Choose exactly one: Guion, Python, or either. Explain the decisive task-specific tradeoffs and what would change your choice.
In the product-information arm use only the supplied profiles. In the cold arm no product documentation is supplied; use only prior knowledge and the task brief.
Do not make a universal language-replacement claim.

Assignment ID: 02-jsonl-retention-choice-2
Schedule SHA-256: 0c7349093da3b7b1a4e60b415bac9db0f958e5f70175ddd51e48478e7edb6ce1
Schedule file SHA-256: 670e8b50a32dd0859e1e63e775ec383f176a5c7d71254929523f9224e6b4d2dd
Choice-safe bundle SHA-256: 4f53cda18c2baa0c0354bb5f9a3ecbe5ed12ab4d8e11ba873c2f11161202b945
Presentation order: Guion, Python
Configured assistant: Anthropic Claude claude-sonnet-5 (claude-sonnet-5); reasoning effort provider-default
Budget: 5 active minutes and 0 tool calls.
No implementation results, benchmarks, author calibration, or candidate runs have been shown.

# Task brief
# JSONL event summary

**Class:** small automation or data CLI
**Origin:** independent-candidate brief draft
**Initial request:** Build a CLI invoked as `jsonl-summary --events PATH --output PATH [--strict]`. Read JSON Lines events with non-empty string `service` and RFC3339 `at` fields. RFC3339 timestamps use the exact `YYYY-MM-DDTHH:MM:SS[.fraction](Z|±HH:MM)` form with a valid Gregorian date, seconds from 00 through 59, a timezone offset, and (when present) one to six fractional digits. Fractions longer than six digits are invalid. Python's broader ISO 8601 forms are not accepted. Ignore blank or whitespace-only lines. Each other malformed line, including an invalid timestamp, is reported by its one-based physical line number in the original file, including blank lines; normal mode continues and strict mode fails without replacing an existing output. Duplicate valid events count independently. Canonical output timestamp serialization is UTC with a trailing `Z`; omit the fractional part when it is zero, otherwise retain the exact fractional value, limited to six digits, and remove trailing zeroes (for example, `2025-01-02T00:00:00.125Z`). Normalize timestamps to UTC and group by service/date. Emit JSON shaped as `{"groups":[{"service":"api","date":"2025-01-02","count":2,"oldest":"2025-01-02T00:00:00Z","newest":"2025-01-02T00:00:00Z"}],"invalid":[{"line":5}],"ignored_count":0}`. Sort groups by service then date.

**Follow-up change:** Add `--services PATH`, a newline-delimited service allow-list. When present, summarize only listed services and separately report valid events ignored by the filter. A nonempty list with no matching services produces no groups and counts all valid events as ignored. Without it, preserve the initial result schema and behavior.

**Acceptance:** Empty input; multiple dates/services; equivalent timestamps with offsets; blank lines; malformed nonblank JSON and missing fields; strict and continuing modes; duplicate events remain counted; stable UTC normalization and output ordering; filter matches, misses, empty list; normal and strict validation failures preserve an existing output; clean rerun.

**Fairness notes:** Python may use the standard library. Record cold dependency setup separately from warm execution. Do not grant either language a prewritten parser or event model.

# Option 1: Guion
No product profile or documentation is supplied for this option.

# Option 2: Python
No product profile or documentation is supplied for this option.
