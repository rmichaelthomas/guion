#!/usr/bin/env python3
"""Measure health-digest after removing the fixture's deliberate timeout wait."""

from __future__ import annotations

import argparse
import copy
import hashlib
import json
import os
import platform
import random
import statistics
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT))
sys.path.insert(0, str(ROOT / "evaluation/completion"))
from compare_real_tasks import APPS, MANIFEST, PYTHON_REFERENCE  # noqa: E402
from evaluation.protocol.case_runner import run_case  # noqa: E402


def interval(values: list[float], rng: random.Random) -> list[float]:
    medians = sorted(statistics.median(rng.choices(values, k=len(values))) for _ in range(5000))
    return [medians[124], medians[4874]]


def make_case() -> dict:
    manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))
    task = next(item for item in manifest["tasks"] if item["id"] == "06-service-health-digest")
    case = copy.deepcopy(task["initial"]["cases"][0])
    case["id"] = "health-mixed-fast-response-diagnostic"
    case["http_fixtures"]["health"]["/slow"].pop("delay_ms", None)
    for check in case["checks"]:
        if check["kind"] in ("json_path_equals", "json_equals"):
            rows = check["value"] if isinstance(check["value"], list) else check["value"]["results"]
            for row in rows:
                if row["service"] == "slow":
                    row["status"] = "healthy"
                    row["raw_status"] = "ok"
        elif check["kind"] == "file_equals":
            check["value"] = check["value"].replace("slow timeout\n", "slow healthy\n")
    return case


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--trials", type=int, default=7)
    args = parser.parse_args()
    if args.trials < 3:
        parser.error("at least three randomized pairs are required")

    binary = args.binary.resolve(strict=True)
    scratch = ROOT / "target/perf-session/real-tasks-tmp"
    scratch.mkdir(parents=True, exist_ok=True)
    os.environ["TMPDIR"] = str(scratch)
    tempfile.tempdir = str(scratch)
    case = make_case()
    candidates = {
        "guion": (
            {
                "command": [str(binary), "run", "--no-cache", "${candidate}"],
                "workspace_root_project": True,
                "env": {"GUION_FILE_ROOT": "${workspace}"},
            },
            APPS / "health-digest",
        ),
        "python": ({"command": [sys.executable, "${candidate}/main.py"]}, PYTHON_REFERENCE),
    }

    rng = random.Random(20261027)
    samples = {name: [] for name in candidates}
    orders = []
    for pair in range(args.trials + 1):
        order = list(candidates)
        rng.shuffle(order)
        observed = {}
        for name in order:
            config, candidate_root = candidates[name]
            outcome = run_case(case, config, candidate_root, timeout_seconds=10)
            if not outcome["passed"]:
                raise RuntimeError(f"{name} failed output verification: {outcome['failures']}")
            observed[name] = sum(step["elapsed_seconds"] for step in outcome["steps"])
        if pair:
            orders.append(order)
            for name in candidates:
                samples[name].append(observed[name])

    ratios = [g / p for g, p in zip(samples["guion"], samples["python"], strict=True)]
    report = {
        "schema_version": 1,
        "kind": "health-fast-response-diagnostic",
        "status": "passed",
        "commit": subprocess.run(
            ["git", "rev-parse", "--short", "HEAD"], cwd=ROOT, text=True, capture_output=True, check=True
        ).stdout.strip(),
        "guion_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
        "python": sys.version,
        "platform": platform.platform(),
        "case_id": case["id"],
        "case_change": (
            "Removed the 700 ms delay from the slow endpoint and changed its expected result from timeout "
            "to healthy. Other endpoints, concurrency checks, and output checks are unchanged."
        ),
        "measurement": (
            "One unmeasured warmup pair and randomized paired trials; fresh-process source execution, "
            "Guion cache bypassed, every output checked, fixture setup excluded, OS cache uncontrolled. "
            "This diagnostic is not part of the frozen acceptance suite."
        ),
        "trials": args.trials,
        "samples_seconds": samples,
        "paired_order": orders,
        "median_seconds": {name: statistics.median(values) for name, values in samples.items()},
        "guion_over_python_median": statistics.median(ratios),
        "bootstrap_95_interval": interval(ratios, rng),
        "all_outputs_verified": True,
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(
        json.dumps(
            {
                "status": report["status"],
                "median_seconds": report["median_seconds"],
                "ratio": report["guion_over_python_median"],
                "bootstrap_95_interval": report["bootstrap_95_interval"],
            }
        ),
        flush=True,
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
