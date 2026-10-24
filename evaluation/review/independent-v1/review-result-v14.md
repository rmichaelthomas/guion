# Independent review v14

**Verdict: Revise before freeze**  
**Reviewer:** Codex, separate static packet and oracle reviewer; no authorship role. This was not a human evaluation. The reviewer inspected the author calibration record but did not run candidate code, builds, tests, schedules, pre-run records, or comparative sessions.

| Area | Verdict | Finding |
|---|---|---|
| 01 — Support SLA report | Approve | Thirteen cases cover thresholds, malformed rows, output preservation, and team filtering. |
| 02 — JSONL retention | Approve | Fractional offset conversion and over-precision rejection are covered. |
| 03 — Release notes | Revise | The invalid `--since` case only required the flag name, not an explanatory diagnostic. |
| 04 — Calendar conflict report | Approve | Fractional window clipping, over-precision, and invalid calendar sources are covered. |
| 05 — Webhook replay | Revise | Two cases checked only `/accepted`; add complete-document assertions. |
| 06 — Service health digest | Approve | Slow-trickle timeouts, maintenance boundaries, invalid responses, and duplicates are covered. |
| 07 — Config migrator | Approve | No task-specific blocker found in the 21 cases. |
| 08 — Documentation link index | Revise | Alias-collision output lacked whole-document equality; define normalized path sorting and tie-breaks. |
| 09 — Lockfile auditor | Approve | No task-specific blocker found in the 17 cases. |
| Manifest and oracles | Revise | Python `==` treats JSON `true` as `1` and `1` as `1.0`; make both JSON checks type-sensitive. |
| Protocol | Revise | “Current stable Python” conflicts with pinned CPython 3.14.8. Reviewer reported missing direct-choice validation tests. |
| Practical-benefit rule | Revise | Add an absolute completion floor and a stop rule for severe Veritas-only failures. |

## Cross-cutting checks

The ZIP had 62 entries, passed CRC validation, and all 61 declared artifact hashes matched. The campaign archive and dependency digests, eight-document choice-bundle hashes and digest, and protocol alias matched. The bundle contained no identified benchmark, calibration, known-workload, or candidate outcomes. Source-free delivery and runtime fairness passed static inspection; the images were not built. Calibration recorded 122/122 under CPython 3.14.8, but remained author-only evidence.

## Author verification note

The v14 packet already included `test_choice_must_bind_pre_run_record_and_occur_after_it` in `evaluation/tests/test_recommendations.py`. It directly called `recommendations.validate_record` and covered a valid binding, a mismatched pre-run file digest, and a decision timestamp before the pre-run record. Thus the review’s statement that no such tests existed did not match the packet. Remediation adds an explicit mismatch between the choice’s recorded pre-run timestamp and the pre-run record timestamp, and the next review should verify all four cases in the exact revised packet.
