# Independent review result — revised draft v2

- Reviewer task: `01a16aa8-0072-7ed3-b424-347a48367dc8`
- Reviewed packet SHA-256: `a59d16b46bd5976a1a8d5b0df4bf9d644e6d5267f442293ebc158457f32e5fc3`
- Decision: **REVISE**; reviewer declared no role in authoring and did not inspect pilot reports.
- Practical-benefit rule: approved.
- Formal runs and choices: remain prohibited until blocking items are corrected and the exact revised packet is independently re-reviewed.

## Blocking areas recorded

1. SLA: define CSV and policy validation; add zero-match team filter oracle.
2. JSONL: remove unsupported policy arguments; use physical line numbers; add nonmatching allow-list.
3. Release notes: define input schemas, date and filename/version rules, and duplicate-ID scope; add invalid date/version preservation cases.
4. Calendar: define timezone/UID requirements and pairwise output for 3+ overlaps; add non-UTC normalization.
5. Webhook: define secret initialization and result schemas, positive retry limits and late limit application; cover null payload.
6. Health: define omitted `--now` and unique service names; cover timeout and invalid response shapes during maintenance.
7. Migrator: state schema-2 default and version range; test explicit schema-3 parallelism.
8. Documentation index: define ambiguous exact-term backlinks and same-page duplicate aliases; cover stale outgoing links after deletion and unknown front matter.
9. Lockfile: remove premature `--as-of`; define optional policy fields and duplicate package behavior; test non-denied exceptions.
10. Environment fairness: remove campaign-global `VERITAS_PYTHON` and `PYTHONPATH`; require project-declared managed Python and lock for Veritas interop.
11. Calibration identity: align calibration CPython with campaign CPython and rerun after oracle corrections.

## Nonblocking recommendations carried forward

- Keep novelty claims limited to the specific differentiated features documented in the overlap audit.
- The next packet implements this with `evaluation/protocol/record_prerun.py` and `evaluation/sandbox/campaign-lock.json`; the record must be created after the next approval and schedule generation, before sessions begin.
- Report results as descriptive evidence for these briefs, model, and host, not a universal recommendation.

The reviewer verified the packet's 46 source hashes and the nine-document choice bundle. Its local loopback test run could not bind sockets in the restricted review shell; the review project itself was not modified.
