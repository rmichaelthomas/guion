You are advising a developer choosing a language for the attached task.
Do not implement the task, browse, use tools, or infer results from code that has not been built.
Choose exactly one: Guion, Python, or either. Explain the decisive task-specific tradeoffs and what would change your choice.
In the product-information arm use only the supplied profiles. In the cold arm no product documentation is supplied; use only prior knowledge and the task brief.
Do not make a universal language-replacement claim.

Assignment ID: 04-calendar-conflict-report-choice-1
Schedule SHA-256: 0c7349093da3b7b1a4e60b415bac9db0f958e5f70175ddd51e48478e7edb6ce1
Schedule file SHA-256: 670e8b50a32dd0859e1e63e775ec383f176a5c7d71254929523f9224e6b4d2dd
Choice-safe bundle SHA-256: 4f53cda18c2baa0c0354bb5f9a3ecbe5ed12ab4d8e11ba873c2f11161202b945
Presentation order: Python, Guion
Configured assistant: Anthropic Claude claude-sonnet-5 (claude-sonnet-5); reasoning effort provider-default
Budget: 5 active minutes and 0 tool calls.
No implementation results, benchmarks, author calibration, or candidate runs have been shown.

# Task brief
# Multi-calendar conflict report

**Class:** concurrent network or package integration
**Origin:** independent-candidate brief draft; revised after overlap audit
**Initial request:** Build a CLI invoked as `calendar-conflicts --sources PATH --window-start RFC3339 --window-end RFC3339 --output PATH --concurrency N --timeout-ms N`. RFC3339 timestamps use the exact `YYYY-MM-DDTHH:MM:SS[.fraction](Z|±HH:MM)` form with a valid Gregorian date, seconds from 00 through 59, a timezone offset, and (when present) one to six fractional digits. Fractions longer than six digits are invalid. Require window-start < window-end, concurrency > 0, timeout-ms > 0, and unique non-empty calendar names. The sources JSON is `{"calendars":[{"name":"team","url":"..."}]}`. Fetch configured iCalendar feeds concurrently from supplied loopback endpoints. Accept timed VEVENTs with valid DTSTART and DTEND and a non-empty UID; ignore all-day events and do not expand RRULE recurrences. A timed event with a floating local time, an unknown TZID, a missing UID, or end <= start makes that source `invalid_calendar`. TZID values must resolve from a valid VTIMEZONE component or the installed IANA time-zone database. Absent STATUS means CONFIRMED; ignore CANCELLED; include TENTATIVE only with `--include-tentative`. Canonical output timestamp serialization is UTC with a trailing `Z`; omit the fractional part when it is zero, otherwise retain the exact fractional value, limited to six digits, and remove trailing zeroes (for example, `2025-01-02T00:00:00.125Z`). Normalize event instants to UTC and report each overlapping event pair from different calendars as a separate conflict, including when three or more events overlap. Each conflict contains exactly two event IDs and two calendar names, each pair sorted lexically. Bound concurrency and each request by one total wall-clock deadline measured from connection start through the complete response body; slow trickle data does not reset the deadline. Continue when one calendar fails. Clip each pairwise intersection to the requested half-open window [window-start, window-end); touching endpoints are not overlaps. Emit JSON shaped as `{"conflicts":[{"start":"RFC3339 UTC","end":"RFC3339 UTC","event_ids":["sorted IDs"],"calendars":["sorted names"]}],"source_errors":[{"calendar":"name","category":"http|timeout|invalid_calendar"}]}`. Sort conflicts by start, end, then event IDs, and source errors by calendar. Do not require public network access.

**Follow-up change:** Add `--include-tentative` to include tentative events in conflict detection while preserving the default confirmed-only behavior.

**Acceptance:** Multiple feeds; timezone-equivalent timestamps; true and non-overlap boundaries; cancelled events; malformed calendar; transient and permanent endpoint failures; concurrency bound; deadline; deterministic output; follow-up tentative-event handling; clean install and source-free launch. For a 10:00–11:00 event and a 10:30–11:30 event, report the intersection 10:30–11:00.

**Fairness notes:** Both candidates receive the same loopback iCalendar fixture, parser dependency allowance, network grant, and time budget. The non-Python candidate may use its managed Python bridge with the same parser. This replaces the earlier paginated incident export draft because the existing API-sync example already exercises pagination, cursor persistence, retries, and restart recovery.

# Option 1: Python
No product profile or documentation is supplied for this option.

# Option 2: Guion
No product profile or documentation is supplied for this option.
