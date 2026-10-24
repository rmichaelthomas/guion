from __future__ import annotations
import json
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT))
from evaluation.protocol.case_runner import run_case

manifest = json.loads((ROOT / "evaluation/fixtures/independent-v1/manifest.json").read_text())
task = next(item for item in manifest["tasks"] if item["id"] == "06-service-health-digest")
candidate = ROOT / "evaluation/calibration/python-reference"
config = {"command": ["python3", "${candidate}/main.py"]}
outcomes = [
    run_case(case, config, candidate, timeout_seconds=60)
    for stage in ("initial", "follow_up")
    for case in task[stage]["cases"]
]
report = {
    "candidate": "author-calibrated-python-reference",
    "passed": sum(r["passed"] for r in outcomes),
    "total": len(outcomes),
    "cases": [
        {"id": r.get("case_id"), "passed": r.get("passed"), "elapsed_seconds": r.get("elapsed_seconds"), "failures": r.get("failures")}
        for r in outcomes
    ],
}
results = Path(__file__).with_name("results")
results.mkdir(exist_ok=True)
(results / "service-health-python.json").write_text(json.dumps(report, indent=2) + "\n")
print(json.dumps(report, indent=2))
