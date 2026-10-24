# Independent review — Veritas sprint packet v4

**Decision:** REVISE  
**Reviewer:** Codex AI, static packet reviewer  
**Independence:** The reviewer had no role in authoring the briefs, fixtures, acceptance rules, or Veritas implementation. Candidate outcomes and candidate tests were not run.

The reviewed packet SHA-256 was `e846c8908d56bdc6c7d7badf0b84bbbc1b2dd41ed5a0be83087a8c9c96807f7f`. All 56 source artifact hashes, the product bundle digest (`3984ca6b2603f91c05535e9d767c5adb4019610c3adc531a046977fdc84b9355`), and the included Linux/ARM64 archive digest (`65154eaa8892fad788e5511ae1fff91f714fe99a1a28a7016ffdaf3dae0d47ca`) matched. The packet contained 89 unique cases, and author calibration reported 89/89; these confirm consistency, not independent correctness.

## Decisions

| Area | Decision | Finding |
|---|---|---|
| 01 Support SLA | APPROVE | Unknown policy keys, Boolean and negative values, output preservation, and zero-match filters are covered. |
| 02 JSONL retention | APPROVE | Physical line numbering, strict-mode preservation, and empty/nonmatching allow-lists are specified and covered. |
| 03 Release notes | REVISE | The brief/reference define all ASCII punctuation, but the named all-punctuation case exercises only `?`, `@`, backslash, and `!` beyond existing `*` and `-` cases. Require a non-empty `category_order` explicitly and test empty-array rejection with preservation of both outputs. |
| 04 Calendar conflicts | APPROVE | The `evt-a`, `evt-b`, `evt-d` triple overlap and UTC normalization are asserted. |
| 05 Webhook replay | APPROVE | Binary assertions exclude both the raw secret and its SHA-256 digest; retry, idempotency, attempt boundaries, null payload, and repeated state are covered. |
| 06 Service health | APPROVE | Duplicate names, maintenance boundaries, required `--now`, invalid response shapes, and timeout behavior are covered. |
| 07 Config migrator | REVISE | Schema 1/2/3 checks and missing schema-2 timeout preservation are covered. Add invalid schema-2 `timeout_seconds` preservation; current invalid Boolean/zero cases target schema 3. |
| 08 Documentation link index | APPROVE | Alias normalization/collisions, same-page title/alias handling, metadata, and stale backlink removal are covered. |
| 09 Lockfile auditor | APPROVE | Duplicate package diagnostics are retained; exception suppression, expiry, duplicate names, invalid dates, and non-denied packages are covered. |
| Manifest and oracles | REVISE | Only the punctuation and invalid schema-2 timeout gaps remain among the reviewed corrections. |
| Comparison protocol | REVISE | The source package script stages `record_prerun.py`, campaign lock, and review results, but the pinned archive is stale: it lacks those items and the current product bundle; its embedded hash manifest has 30 entries versus 56 in the packet. Packaged documentation aliases do not match current source hashes. Rebuild and repin the archive, resolving the campaign-lock self-reference by keeping that lock outside the archive. The ledger must reject session-ID reuse across both pairs for one brief, not merely within each pair. Document `file_bytes_not_contains` in the runner guide. |
| Practical-benefit rule | APPROVE | The 20% median reduction, four-of-six direction requirement, guardrails, and limited task-class claim are reasonable for an exploratory assistant recommendation. The primary metric is preregistered and enforced; the result cannot establish that Veritas replaces Python generally. |

The packet remained review-pending, and no formal candidate runs were authorized by this review.
