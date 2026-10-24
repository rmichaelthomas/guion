# Independent review v17

**Verdict: Revise before freeze**  
**Independence:** Fresh Codex task with no role in authoring the language, briefs, fixtures, protocol, implementation, or packet. Static review only; no candidate image, test suite, implementation, or evaluation session was run.

## Finding

- **Packaging verification:** `evaluation/tests/test_shipped_docs_repair.py:213` expected `125 cases total`, while `evaluation/protocol/case-runner.md:40` specifies `127 cases total`. Update the installed-package assertion and refresh the package hashes before freezing.

## Task verdicts

| Task | Verdict |
|---|---|
| 01 — Support SLA report | Approve |
| 02 — JSONL retention | Approve |
| 03 — Release notes | Approve |
| 04 — Calendar conflict report | Approve |
| 05 — Webhook replay | Approve |
| 06 — Service health digest | Approve |
| 07 — Config migrator | Approve |
| 08 — Documentation link index | Approve |
| 09 — Lockfile policy auditor | Approve |

The v16 corrections were present in v17: aligned Task 01 priority diagnostics; exact RFC3339 rejection and output preservation in Tasks 01, 04, and 06; Task 03 acceptance matches its filename-derived version schema; Task 08 checks normalized-path tie ordering for both `check` and `backlinks`; and the packaged docs match their sources. The Veritas image removes `evaluation`, examples, language documentation, and docs. Source-free delivery stages only declared artifacts and locks.

## Protocol and fairness

The comparison protocol and practical-benefit rule were approved for the stated exploratory scope. The two recommendation signals are separate; completion parity and guardrail floors are enforced; the severe Veritas-only stop rule records an append-only stop event without synthesizing a run. Claims are scoped to the sampled tasks, model, host, and bundle. Task overlap is disclosed and bounded.

## Integrity and limits

The v17 ZIP passed CRC validation with 67 entries, 66 declared artifacts, no duplicate or undeclared payload entries, and 66/66 matching SHA-256 hashes. The manifest has 127 unique cases across nine tasks. The author calibration report records 127/127 with the current manifest digest. The v17 packet remains draft until the packaging assertion is corrected and the hashes are refreshed.
