# Independent review v18

**Verdict: Revise before formal sessions**  
**Independence:** Fresh Codex static reviewer with no role in authoring the briefs, fixtures, rules, Veritas implementation, or packet.

## Findings

1. **Campaign model and budgets were not frozen.** The build prompt retained time/tool placeholders; schedule and pre-run records did not bind a cohort-wide model or budgets, and choice summaries could mix model versions.
2. **Choice-order randomization was not operationalized.** The prompt recorded an order but did not define how to render the two product profiles, and choice records did not bind a saved prompt transcript.
3. **The Python relocation example was impossible.** It launched `${candidate}/main.py` while omitting `main.py` and project metadata from the staged artifacts and excluding `main.py` as source.
4. **Task 06’s oracle exceeded its brief.** The invalid maintenance-timestamp fixture required both existing outputs to remain byte-for-byte unchanged, a guarantee absent from the brief.

## Review areas

Tasks 01–05 and 07–09 were approved. Task 06 and its oracle required revision. The practical-benefit rule was approved for the exploratory scope. The review confirmed the corrected 127-case assertion, all 67 packet artifact hashes, the 68-entry ZIP structure, the campaign archive/dependency hashes, the product bundle digest, and safe nested archive paths. Static inspection supported the source-free staging and candidate-image cleanup claims.

## Limits

Static review only. The reviewer did not run candidates, tests, or choice sessions and did not treat author calibration as independent candidate evidence. Formal sessions were not approved by this review.
