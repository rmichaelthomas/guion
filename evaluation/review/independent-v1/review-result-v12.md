# Independent review v12

**Verdict: Revise**  
**Independence:** Codex, independent technical reviewer. The reviewer had no role in authoring the briefs, fixtures, acceptance rules, implementation, or packet.

| Area | Verdict |
|---|---|
| 01 — Support SLA report | Approve |
| 02 — JSONL event summary | Revise |
| 03 — Release note assembler | Revise |
| 04 — Calendar conflict report | Revise |
| 05 — Webhook replay | Approve |
| 06 — Service health digest | Approve |
| 07 — Configuration migrator | Approve |
| 08 — Documentation link index | Revise |
| 09 — Lockfile policy auditor | Approve |
| Manifest and oracles | Revise |
| Comparison protocol | Revise |
| Practical-benefit rule | Approve |

## Required revisions

- **Tasks 02 and 04 — UTC output format:** The briefs require UTC-normalized timestamps, while the oracles compare timestamp strings exactly. Specify one canonical serialization, including whether to use `Z` and how to represent fractional seconds; add a fractional-second case. This changes both languages’ expected behavior.
- **Task 03 — Error diagnostics:** The brief promises that invalid records identify the source file and field, but the relevant cases check only failure, output preservation, or a generic diagnostic. Add assertions for the offending filename and field. This applies to both languages.
- **Task 08 — Unclosed front matter:** Define what happens when a document starts with `---` but has no later exact `---` delimiter, and add a case that checks the chosen behavior. This applies to both languages.
- **Choice-safe bundle:** The bundle includes `python-baseline.md`, explicitly titled “Python evaluation baseline,” and links it from its README. It contains comparison setup guidance rather than prior result values, but the requested choice bundle must exclude a Python comparison baseline. Remove it from the bundle manifest and README links, then recalculate the bundle digest and outer hashes. The setup guide already describes both candidates’ campaign setup.

The reviewer found no named prior development workload, independent Python reference, acceptance result, or benchmark result in the choice-safe documents. The README, reference, architecture, and Python ABI docs describe product capabilities; performance-related statements were framed as capability descriptions, not measured comparative outcomes.

## Checks completed

The ZIP passed integrity checking. All 60 listed SHA-256 entries matched. The campaign archive and dependency digests matched the lock; the product bundle’s document hashes and bundle digest matched. The manifest declared and contained 114 cases. The author calibration also recorded 114 cases, pinned CPython 3.14.8, and matched the manifest, reference, and requirements hashes. The archive’s v23 `docs/evaluation-protocol.md` was byte-for-byte equal to the packet’s `evaluation/protocol/README.md`.

Reviewer instructions identified v11 as the latest prior review, and v10 and v11 reports were included. The v10 fixes were reflected in the briefs, cases, and protocol. The v11 leakage findings about named workloads and acceptance/calibration material appeared removed; the standalone Python baseline remained the exception noted above.

The protocol’s separate choice and implementation signals, 20% practical-benefit threshold, guardrails, and limits on inference were clear. Static inspection also found shared pinned libraries and offline setup, matching candidate limits and fixtures, and a delivery flow that stages declared artifacts and runs the relocated candidate without mounting the original project.

**Limits:** This was a read-only static review. The reviewer did not run tests, builds, image builds, candidate tasks, schedules, pre-runs, formal sessions, or inspect comparative candidate outcomes. Built-image contents and runtime behavior were not verified. The packet should not be frozen and comparative sessions should not start until the revisions are incorporated and the exact revised packet is reviewed.
