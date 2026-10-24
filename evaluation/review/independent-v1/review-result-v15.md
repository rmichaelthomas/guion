# Independent review v15

**Verdict: Revise before freeze**  
**Independence:** Codex, separate static reviewer; no role in authoring the briefs, fixtures, acceptance rules, implementation, or packet. This was not a human evaluation. The review did not run candidate code or tests, build images, install dependencies, create schedules or pre-run records, or start formal sessions.

| Area | Verdict |
|---|---|
| 01 — Support SLA report | Approve |
| 02 — JSONL event summary | Approve |
| 03 — Release note assembler | Approve |
| 04 — Calendar conflict report | Approve |
| 05 — Webhook replay | Approve |
| 06 — Service health digest | Approve |
| 07 — Configuration migrator | Approve |
| 08 — Documentation link index | Revise |
| 09 — Lockfile policy auditor | Approve |
| Manifest and oracles | Revise |
| Protocol and practical-benefit rule | Revise |

## Required revisions

- **Task 08:** Add fixtures for nested folders, symlinked directories, and two relative paths that collide under normalization so the UTF-8 byte tie-break is exercised. The acceptance list also promised database relocation without a corresponding case.
- **JSON oracle tests:** The recursive comparator is type-sensitive and path checks distinguish booleans, integers, and floats. Add a whole-document negative assertion for integer `1` versus float `1.0`.
- **Implementation gate:** The protocol said Veritas must not have a lower completion rate, but the ledger only checked five successful completions for each language. Add an explicit `Veritas completions >= Python completions` condition and an asymmetric-case test.
- **Stop logging:** The severe Veritas-only stop was reported, but the protocol lacked a logging convention for scheduled runs halted before launch. Specify how to preserve those events without changing the frozen schedule.

## Integrity and positive findings

The v15 ZIP passed CRC validation. All 62 declared artifact hashes matched, with no missing or extra paths. The v26 archive and dependency digests, choice-bundle hashes and digest, and packaged protocol aliases matched. The 122-case manifest matched the author-only CPython 3.14.8 calibration metadata. The reviewer confirmed that v14’s claim about absent direct-choice tests was incorrect: the packet contained `recommendations.validate_record` tests for valid binding, digest mismatch, prerecord timestamp mismatch, and a decision before the prerecord. Static inspection confirmed the offline candidate setup and source-free delivery design; built-image behavior remains unverified.
