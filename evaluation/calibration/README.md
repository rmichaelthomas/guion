# Author Python reference calibration

From the foundation root, run `python3 evaluation/calibration/run_reference_calibration.py`. It executes every initial and follow-up case in the current independent-kit manifest against the dependency-matched Python reference and writes a hash-bound report to `evaluation/calibration/results/python-reference-calibration.json`.

This is author calibration for fixture and expected-result consistency. The reference, runner, and brief authors share the implementation workspace, so this result does not count as independent review, a matched build session, or an assistant recommendation decision.
