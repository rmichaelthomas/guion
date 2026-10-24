#!/usr/bin/env python3
"""Run the author-owned Python reference over every independent-kit case.

This verifies fixture/oracle compatibility only. It is not independent review,
a matched implementation session, or an assistant recommendation result.
"""

from __future__ import annotations

import hashlib
import importlib.metadata
import json
import os
from pathlib import Path
import platform
import re
import sys

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT))

from evaluation.protocol.case_runner import run_case


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main() -> int:
    manifest_path = ROOT / "evaluation/fixtures/independent-v1/manifest.json"
    candidate_root = ROOT / "evaluation/calibration/python-reference"
    requirements_path = candidate_root / "requirements.txt"
    candidate_path = candidate_root / "main.py"
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    if platform.python_implementation() != "CPython" or platform.python_version() != "3.14.8":
        raise SystemExit(f"calibration requires CPython 3.14.8, found {platform.python_implementation()} {platform.python_version()}")
    # Use the explicitly staged campaign dependency tree, never an inherited path.
    dependency_root = Path(os.environ.get("VERITAS_CALIBRATION_DEPS", "/opt/campaign-dependencies/python-reference"))
    if not dependency_root.is_dir():
        raise SystemExit(f"campaign dependency tree is missing: {dependency_root}")
    inherited_pythonpath = os.environ.get("PYTHONPATH", "")
    os.environ.pop("PYTHONPATH", None)
    os.environ.pop("PYTHONHOME", None)
    os.environ["PYTHONNOUSERSITE"] = "1"
    for entry in filter(None, inherited_pythonpath.split(os.pathsep)):
        while entry in sys.path:
            sys.path.remove(entry)
    sys.path.insert(0, str(dependency_root))
    dependency_versions = {}
    for line in requirements_path.read_text(encoding="utf-8").splitlines():
        requirement = line.strip()
        if not requirement or requirement.startswith("#"):
            continue
        match = re.fullmatch(r"([A-Za-z0-9_.-]+)==([A-Za-z0-9_.+-]+)", requirement)
        if not match:
            raise SystemExit(f"calibration requirement is not exactly pinned: {requirement}")
        name, pinned = match.groups()
        actual = importlib.metadata.version(name)
        if actual != pinned:
            raise SystemExit(f"calibration dependency {name} must be {pinned}, found {actual}")
        dependency_versions[name] = actual
    candidate = {
        "command": [sys.executable, "${candidate}/main.py"],
        "metadata": {
            "language": "Python",
            "role": "author_calibration_reference",
            "python": platform.python_version(),
            "dependencies": "evaluation/calibration/python-reference/requirements.txt",
        },
        "env": {"PYTHONPATH": str(dependency_root), "PYTHONNOUSERSITE": "1"},
    }

    results = []
    for task in manifest["tasks"]:
        for stage in ("initial", "follow_up"):
            for case in task[stage].get("cases", []):
                result = run_case(case, candidate, candidate_root, timeout_seconds=60)
                result["task_id"] = task["id"]
                result["stage"] = stage
                results.append(result)

    passed = sum(result["passed"] for result in results)
    report = {
        "status": "author_calibration_only",
        "manifest_sha256": digest(manifest_path),
        "candidate_sha256": digest(candidate_path),
        "requirements_sha256": digest(requirements_path),
        "python": platform.python_version(),
        "python_executable": sys.executable,
        "dependency_versions": dependency_versions,
        "dependency_source": str(dependency_root),
        "environment_overrides": {"PYTHONPATH": str(dependency_root), "PYTHONHOME": "removed", "PYTHONNOUSERSITE": "1"},
        "summary": {"passed": passed, "total": len(results)},
        "cases": results,
    }
    output = ROOT / "evaluation/calibration/results/python-reference-calibration.json"
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(f"TOTAL {passed} / {len(results)}; report={output.relative_to(ROOT)}")
    return 0 if passed == len(results) else 1


if __name__ == "__main__":
    raise SystemExit(main())
