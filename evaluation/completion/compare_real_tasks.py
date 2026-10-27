#!/usr/bin/env python3
"""Run frozen acceptance cases on current Guion and the Python reference."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT))
from evaluation.completion.run_applications import APP_IDS  # noqa: E402
from evaluation.protocol.case_runner import run_case  # noqa: E402

MANIFEST = ROOT / "evaluation/fixtures/independent-v1/manifest.json"
APPS = ROOT / "evaluation/completion/applications"
PYTHON_REFERENCE = ROOT / "evaluation/calibration/python-reference"


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--application", action="append", choices=sorted(APP_IDS))
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    scratch = ROOT / "target/perf-session/real-tasks-tmp"
    scratch.mkdir(parents=True, exist_ok=True)
    os.environ["TMPDIR"] = str(scratch)
    tempfile.tempdir = str(scratch)
    manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))
    tasks = {item["id"]: item for item in manifest["tasks"]}
    report = {
        "schema_version": 1,
        "kind": "matched-frozen-case-acceptance",
        "platform": platform.platform(),
        "python": sys.version,
        "guion_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
        "manifest_sha256": hashlib.sha256(MANIFEST.read_bytes()).hexdigest(),
        "note": "Per-step times are acceptance observations, not randomized benchmark results.",
        "applications": {},
    }
    for name in args.application or list(APP_IDS):
        task = tasks[APP_IDS[name]]
        candidates = {
            "guion": (
                {"command": [str(binary), "run", "--no-cache", "${candidate}"],
                 "workspace_root_project": True, "env": {"GUION_FILE_ROOT": "${workspace}"}},
                APPS / name,
            ),
            "python": ({"command": [sys.executable, "${candidate}/main.py"]}, PYTHON_REFERENCE),
        }
        cases = []
        for stage in ("initial", "follow_up"):
            for case in task[stage]["cases"]:
                outcomes = {key: run_case(case, config, path, timeout_seconds=90)
                            for key, (config, path) in candidates.items()}
                cases.append({"stage": stage, "case_id": case["id"], "outcomes": outcomes})
        report["applications"][name] = {"cases": cases, "passed": all(
            result["passed"] for case in cases for result in case["outcomes"].values()
        )}
        summary = {key: sum(case["outcomes"][key]["passed"] for case in cases) for key in candidates}
        print(json.dumps({"application": name, "total": len(cases), **summary}), flush=True)
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    if not all(app["passed"] for app in report["applications"].values()):
        raise SystemExit(1)


if __name__ == "__main__":
    main()
