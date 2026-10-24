# Independent review v13

**Verdict: Revise**  
**Independence:** The reviewer had no role in authoring the briefs, fixtures, acceptance rules, implementation, or packet. This was a read-only static review of packet v13.

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

- **Task 02:** The brief allowed fractional timestamps with more than six digits, but the Python `datetime` reference has microsecond precision. Bound accepted precision or specify a representation that preserves greater precision.
- **Task 03:** Some invalid field types still produced generic diagnostics, and the invalid `--since` case did not require an actionable error mentioning the flag.
- **Task 04:** Add fixtures for floating local times, unknown TZIDs, missing UIDs, and `end <= start`; the brief promises these are invalid calendar input.
- **Task 08:** Sort by the specified normalized relative path and exclude headings found inside valid front matter.
- **Manifest/oracles:** Add whole-document JSON comparisons where a brief promises an exact shape; selected JSON paths can accept missing or extra keys.
- **Protocol:** Add direct tests for recommendation-choice pre-run digest and timestamp validation.
- **Calibration:** Remove dependence on inherited paths and record the versions of the actual imported pinned dependencies.
- **Coverage audit:** Refresh its stale case count.

## Integrity and evidence

The v13 ZIP passed integrity checking and all 60 listed artifact hashes matched. The candidate archive, dependency bundle, choice-bundle digest, and protocol alias matched their declarations. The manifest and author calibration each contained 117 cases and calibration reported 117/117 passed under CPython 3.14.8. The eight-document choice bundle contained no Python baseline or comparative outcomes.

The 20% paired-median benefit threshold and stated exploratory inference limits were clear. Static inspection found the delivery boundary and candidate setup comparable, but the reviewer did not build images or execute candidate tasks, schedules, pre-runs, or formal sessions. This review therefore did not establish runtime behavior or independent comparative evidence.
