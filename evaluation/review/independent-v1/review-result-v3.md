# Independent review result — revised draft v3

- Reviewer task: `01a16abf-5ae6-7f93-b080-f2bd886916f7`
- Reviewed packet SHA-256: `e6f6d7460f1be1b442da03354adcacd9ead673005694d975828a211d2b9fcf71`
- Decision: **REVISE** before freezing or collecting comparative results.
- Independence: reviewer declared no role in authoring briefs, fixtures, acceptance rules, or the Veritas implementation; review was limited to the supplied packet and sidecar.
- Candidate outcomes: none run or inspected outside the packet; no runner test suite executed.

## Decisions

| Area | Decision |
|---|---|
| Support SLA report | Revise |
| JSONL retention | Approve |
| Release notes | Revise |
| Calendar conflict report | Approve |
| Webhook replay | Approve |
| Service health digest | Approve |
| Config migrator | Revise |
| Documentation link index | Approve |
| Lockfile auditor | Approve |
| Manifest and oracles | Revise |
| Comparison protocol | Revise |
| Practical-benefit rule | Approve |

## Required corrections

### Briefs

- **01 — Support SLA:** Define behavior for unknown keys in `response_minutes`. The reviewer recommends allowing only `high` and `low`, rejecting other keys, and preserving existing output on invalid policy.
- **03 — Release notes:** “Escape Markdown punctuation” is underspecified. Name the exact punctuation set and align the Python reference and expected outputs.
- **07 — Config migrator:** State whether `check` validates schema-specific requirements for versions 1, 2, and 3. Add coverage for the chosen rule; the reference and cases then focused mostly on version 3.

### Manifest and oracles

- **04 — Calendar conflicts:** Existing events had A–B and B–C overlap but no simultaneous three-event overlap. Add a fixture where all three overlap and assert all three pairwise conflicts, including ordered IDs and names.
- **05 — Webhook replay:** The brief prohibits persisting the secret or its digest, but no case checked database/state bytes. Check both secret and digest after initialization.
- **09 — Lockfile auditor:** Add two same-name package entries that trigger the same rule and assert the repeated diagnostics are retained.
- **01 — Support SLA:** Add Boolean and negative policy values; after clarifying unknown keys, add an unknown-key case.
- **03 — Release notes:** Add change text with punctuation beyond the currently tested `*` and `-`, with exact expected Markdown.
- **07 — Config migrator:** Add `check` cases for supported schemas according to the clarified rule. Verify schema 2 missing required `timeout_seconds` fails without rewriting the file.

### Comparison protocol

- `distribution/package-current-host.sh` omitted `evaluation/protocol/record_prerun.py`, `evaluation/sandbox/campaign-lock.json`, and `evaluation/review/independent-v1/review-result-v2.md`, despite listing them in `artifact-hashes.json`. Add them to staging and keep the packaging regression check aligned.
- `ledger.py` compared paired records with each other but did not bind a build to its scheduled slot. Include the assigned session slot plus schedule and pre-run digest identities in each run record, then validate them against the frozen schedule before counting the pair.
- The protocol requires choosing the primary practical-benefit measure before the first run, but the pre-run record did not store that choice. Add it to the pre-run record and enforce it during result summarization.

## Verified packet properties

The packet archive matched its sidecar; all packet artifact hashes and the product-information bundle hashes matched; the Linux/ARM64 archive digest matched `campaign-lock.json`. Static inspection found broadly equivalent offline runner setup: both images use the same OS base, CPython 3.14.8, uv 0.13.4, resource limits, offline wheelhouse, and loopback fixtures. The Veritas package is present and matches its pinned digest. Candidate configurations did not set campaign-global `VERITAS_PYTHON` or `PYTHONPATH`; relocated delivery stages declared artifacts without mounting the original source project.

The prior v2 findings were mostly reflected: zero-match SLA filtering, JSONL physical-line and allow-list rules, timezone normalization, webhook retry/null behavior, health-digest boundaries, migration defaults, documentation-index cases, lockfile policy defaults, runtime alignment, and removal of global Python overrides. The calendar still lacked true three-way overlap coverage; release escaping remained underspecified.

The 20% paired-reduction rule was approved for the explicitly limited task-class recommendation. It requires at least four of six pairs favoring Veritas, completion and safety guardrails, and no severe regression in the remaining class. It does not support a general claim that Veritas replaces Python.
