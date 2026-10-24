from __future__ import annotations
import json
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT))
from evaluation.protocol.case_runner import run_case

manifest = json.loads((ROOT / "evaluation/fixtures/independent-v1/manifest.json").read_text())
task = next(item for item in manifest["tasks"] if item["id"] == "07-config-migrator")
candidate = Path(__file__).with_name("config_migrator")
config = {
    "binary": str(ROOT / "target/debug/veritas"),
    "command": ["${binary}", "run", "${candidate}"],
    "workspace_root_project": True,
    "env": {
        "VERITAS_PYTHON": "/private/tmp/veritas-final-uv-python/cpython-3.14.8-macos-aarch64-none/bin/python3.14",
        "VERITAS_UV": str(ROOT / "distribution/out-north-star-sprint-final-macos-v9/veritas-0.1.0-darwin-arm64/toolchains/uv"),
        "UV_CACHE_DIR": "/private/tmp/veritas-configpilot-uv-cache",
    },
}
outcomes = [
    run_case(case, config, candidate, timeout_seconds=60)
    for stage in ("initial", "follow_up")
    for case in task[stage]["cases"]
]
report = {
    "candidate": "author-self-review-veritas-config-migrator",
    "passed": sum(r["passed"] for r in outcomes),
    "total": len(outcomes),
    "cases": [{"id": r.get("case_id"), "passed": r.get("passed"), "elapsed_seconds": r.get("elapsed_seconds"), "failures": r.get("failures")} for r in outcomes],
}
results = Path(__file__).with_name("results")
results.mkdir(exist_ok=True)
(results / "config-migrator-veritas.json").write_text(json.dumps(report, indent=2) + "\n")
print(json.dumps(report, indent=2))
