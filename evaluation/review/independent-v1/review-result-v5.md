# Independent review v5 — Veritas independent workloads

**Reviewer:** Codex AI, independent static packet reviewer  
**Independence:** The reviewer had no role in authoring the briefs, fixtures, acceptance rules, or Veritas implementation, did not view candidate outcomes, and did not run tests or builds.  
**Reviewed packet:** v6, SHA-256 `4ad11edf1ec21cab8d7ea86ed34961008ac0953cedd1c82b4764510c3107795c`.

The packet digest matched. All 57 listed packet artifact hashes matched. The product-information bundle digest matched `3984ca6b2603f91c05535e9d767c5adb4019610c3adc531a046977fdc84b9355`; the Linux/ARM64 archive matched the campaign lock at `11b158f4339be6da450174e3cd44382dcb140e0a3207f4864747ff4b4d3954b0`. The archive's embedded artifact manifest had 55 entries, with the archive and campaign lock correctly packet-only.

| Area | Verdict |
|---|---|
| 01 Support SLA | APPROVE |
| 02 JSONL retention | APPROVE |
| 03 Release notes | REVISE |
| 04 Calendar conflicts | APPROVE |
| 05 Webhook replay | APPROVE |
| 06 Service health | APPROVE |
| 07 Config migrator | REVISE |
| 08 Documentation link index | APPROVE |
| 09 Lockfile auditor | APPROVE |
| Manifest and oracles | REVISE |
| Comparison protocol | REVISE |
| Practical-benefit rule | APPROVE |

## Required revisions

1. **Task 03 — Release notes:** `category_order` names and change text were only constrained to non-empty strings, although both appear in Markdown. CR/LF could alter document structure. Require category names and change text to be single-line strings, and add CR and LF rejection cases that preserve both prior outputs. Applies to both languages.
2. **Task 07 — Config migrator:** The follow-up contradicted the schema: schema 2 does not require `parallelism`, but migration adds its schema 3 default. Clarify that schema 2 accepts an absent field and migration adds `parallelism = 4`; a present invalid value fails without mutation. Applies to both languages.
3. **Task 07 — Python reference:** Migration rewrote the version line and dropped inline comments such as `version = 1 # owner note`, contrary to the comment-preservation promise. Preserve the comment and assert `version = 2 # owner note`, or narrow the promise. Applies to both languages' acceptance expectations.
4. **Comparison protocol — visibility parity:** The Veritas image removed `/opt/veritas/evaluation` and `/opt/veritas/examples` but retained `/opt/veritas/language/examples` and `/opt/veritas/docs`, including historical checkpoints and evaluation notes unavailable to Python. Remove those paths from the Veritas image or provide matching choice-safe material to Python. Add an assertion that neither candidate can read unmatched examples or evaluation material.
5. **Packet completeness:** Reviewer instructions named `evaluation/tests/test_recommendations.py`, which was absent from the packet. Include and hash it, or correct the instructions to identify the shipped implementation and tests.

The choice prerecord binding, delivery staging, source exclusion, candidate UID/permissions, offline runtime and dependency setup, and practical-benefit guardrails otherwise passed static review. The 20% task-class threshold supports only a limited recommendation for the sampled task classes, not a population-level claim that Veritas replaces Python. The exact v6 packet was **not ready to freeze**; revise it and obtain a fresh independent review before creating the schedule and pre-run record.
