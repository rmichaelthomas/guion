# Independent review v16

**Verdict: Revise before freeze**  
**Independence:** Fresh Codex task with no role in authoring the candidate language, briefs, fixtures, acceptance rules, implementation, or packet. The review was static and did not execute candidate code or tests.

| Area | Verdict | Review note |
|---|---|---|
| 01 — Support SLA report | Revise | Priority diagnostic conflicts with its fixture; timestamp rejection is weakly tested. |
| 02 — JSONL event summary | Approve | Fixtures distinguish strict RFC3339 from permissive ISO parsing. |
| 03 — Release note assembler | Revise | Acceptance promises validation of record versions, but records have no version field. |
| 04 — Calendar conflict report | Revise | Over-precision is tested, but another permissive timestamp form is not. |
| 05 — Webhook replay | Approve | No material specification or packaging issue found. |
| 06 — Service health digest | Revise | Coverage claims strict RFC3339, but fixtures do not reject malformed timestamps. |
| 07 — Configuration migrator | Approve | No material specification or packaging issue found. |
| 08 — Documentation link index | Revise | Normalized-path tie-break is tested for `check`, not `backlinks`. |
| 09 — Lockfile policy auditor | Approve | No material specification or packaging issue found. |

## Required revisions

1. **Refresh packaged documentation and remove private language docs from the candidate image.** Current protocol documentation says 125 cases and specifies the append-only stop log; the hash-locked archive contains stale copies saying 122 cases and older stop-log wording. Its `docs/case-runner.md` also says 122. The package hash agrees with the stale archive, so the hash does not detect source-to-package drift. The archive also contains `language/python-abi.md`, which differs from the shared product-information document, and the image recipe removes `language/examples` while leaving the other language Markdown files. See `docs/evaluation-protocol.md:63,89`, `docs/case-runner.md:40`, and `language/python-abi.md` in the archive.

2. **Align Task 01 diagnostics.** The brief requires diagnostic code `unknown_priority`, while the fixture checks for `unknown priority` and the Python reference emits the latter. Make the brief, fixture, and reference agree. See `evaluation/briefs/independent/01-support-sla-report.md:5`, `evaluation/fixtures/independent-v1/manifest.json:236`, and `evaluation/calibration/python-reference/main.py:113-115`.

3. **Remove or define Task 03 record versions.** The record schema has exactly `id`, `category`, and `text`, and version comes from the filename stem. Acceptance nevertheless says to reject malformed “record versions.” Remove the unsupported phrase or define the field and test it. See `evaluation/briefs/independent/03-release-notes.md:5`.

4. **Add exact RFC3339 rejection cases to Tasks 01, 04, and 06.** Task 01 uses only `not-a-timestamp`; Task 04 tests over-precision but not another permissive form; Task 06 lacks a malformed timestamp fixture despite the coverage-audit claim. A space separator such as `2025-01-02 00:00:00Z` should be rejected, with existing outputs preserved where applicable. Task 02 already demonstrates this distinction. See `manifest.json:266,2770` and `coverage-audit.md:14`.

5. **Exercise Task 08 backlink ordering.** The normalized UTF-8 tie fixture orders the colliding source paths for `check`, but does not run `backlinks`, although the brief specifies the same ordering for both. Add a backlink assertion with both paths linking to the same target. See `evaluation/briefs/independent/08-documentation-link-index.md:5` and the tie fixture near `manifest.json:7149`.

## Integrity and limitations

The ZIP passed CRC validation. All 63 declared hashes matched. The manifest and calibration agree on 125 unique cases, and the calibration reports 125/125. Campaign lock digests and product-information bundle hashes matched. The review confirmed the comparator type checks, completion-parity gate, direct-choice binding tests, source-free delivery checks, Task 08 symlink and normalization fixtures, and stop-log convention. Candidate behavior and author calibration were not independently executed.
