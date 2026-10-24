# Independent review result — Veritas packet v10

**Verdict:** Revise. **Reviewer:** Codex, independent technical reviewer. The reviewer reported no role in authoring the briefs, fixtures, acceptance rules, implementation, or packet. The review was read-only; it did not run builds, tests, candidate tasks, schedules, pre-runs, or formal sessions, and it did not inspect comparative candidate outcomes.

## Findings

- Tasks 01–04 and 08 were approved as written.
- Task 05 (webhook replay), Task 06 (service health), and Task 09 (lockfile auditor) needed explicit package-source restrictions: only the standard library or the shared offline wheelhouse may be used; downloads and installation from other sources must be forbidden. Veritas may implement directly or use its declared managed Python bridge with that same wheelhouse.
- Task 07 needed a follow-up proving schema 2 accepts a valid timeout with omitted `parallelism` during read-only `check`, and that Boolean `parallelism = true` fails both `check` and `migrate` without changing source bytes.
- Task 09 needed a valid single-exception boundary case where `expires` equals `--as-of`; the deny violation must remain and `expired_exception` must be reported. The existing expired and duplicate-exception cases should remain separate.
- The choice-safe product bundle needed an in-bundle README link and removal of known-workload and benchmark-result references. Its bundle digest and outer hashes then needed refresh.
- The operational recommendation rule needed to explicitly require both the direct no-build choice signal and matched implementation evidence, with disagreement reported and no class recommendation made.
- These cases would raise the manifest from 111 to 114, requiring the calibration report and all count references to be refreshed.
- Both campaign images needed rebuilding after the archive and runner inputs changed.

The reviewer approved the 20% practical-benefit threshold for the limited exploratory task-class verdict, subject to correctness, delivery, completion, and severe-regression guardrails. The 18-pair / 18-choice design was considered adequate only for bounded descriptive evidence, not a population-level or universal claim.

## v10 integrity observations

The ZIP passed integrity checking and all 58 listed artifact hashes matched. The campaign lock and archive digests matched; the protocol alias matched the locked v20 package. The reviewer confirmed that `case_count` matched computed cases, direct choices and host-ID checks were implemented, and the calibration report declared CPython 3.14.8 with 111 cases. Existing image IDs and labels were outside that packet’s evidence scope.

This report is included as a remediation checklist for v11. The new reviewer must independently verify the current packet; this prior revise verdict is not approval.
