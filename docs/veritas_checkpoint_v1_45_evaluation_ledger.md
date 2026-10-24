# Veritas checkpoint v1.45 — comparative evaluation ledger

## Change

Added `evaluation/protocol/ledger.py`, a Python standard-library tool for validating individual comparison records and summarizing matched Veritas/Python pairs. The record captures task origin and class, brief digest, candidate version, model and fresh-session identity, host and hardware, randomized pair order, equal budgets, timestamps, initial and follow-up results, guardrails, cold/warm costs, intervention descriptions, command list, evidence paths and digests, reviewer verdict, and exclusion reason.

The summarizer retains failed, incomplete, malformed, excluded, known, and calibration attempts in its output. Only complete independent pairs with matching task, model version, randomized order, hardware/network mode, and budget count toward independent sample size. A pair must have distinct session IDs and run IDs. It reports whether the prespecified minimum sample is available; it does not decide which language to recommend.

The installed package kit now includes the ledger module so `evaluation/tests/test_protocol_ledger.py` can import and exercise the same tool from the release.

## Verification

- Eight focused ledger tests pass, including malformed-field handling and rejects for a reused model session or mismatched hardware.
- The full source evaluation suite passes 31 tests with three installed-release-only checks skipped as intended.
- RC67 omitted `evaluation/protocol/ledger.py`; the installed import check caught `ModuleNotFoundError`. The packager was corrected. RC69 includes the ledger module and its full installed suite passes 31/31.

## Boundary

No independent recommendation runs have been collected. The protocol remains a preregistration draft pending the stated second-evaluator review. A structured ledger makes collection auditable; it does not create independence or evidence by itself.
