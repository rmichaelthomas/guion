#!/usr/bin/env python3
"""Measure matched, output-checked real task cases in randomized pairs."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import random
import statistics
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT))
from evaluation.completion.compare_real_tasks import APP_IDS, APPS, MANIFEST, PYTHON_REFERENCE  # noqa: E402
from evaluation.protocol.case_runner import run_case  # noqa: E402


def interval(values: list[float], rng: random.Random) -> list[float]:
    medians = sorted(statistics.median(rng.choices(values, k=len(values))) for _ in range(2000))
    return [medians[49], medians[1949]]


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--application", action="append", choices=sorted(APP_IDS))
    parser.add_argument("--trials", type=int, default=7)
    args = parser.parse_args()
    if args.trials < 3:
        parser.error("at least three pairs are required")
    binary = args.binary.resolve(strict=True)
    tmp = ROOT / "target/perf-session/real-tasks-tmp"
    tmp.mkdir(parents=True, exist_ok=True)
    os.environ["TMPDIR"] = str(tmp)
    tempfile.tempdir = str(tmp)
    manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))
    tasks = {item["id"]: item for item in manifest["tasks"]}
    rng = random.Random(20261026)
    report = {"schema_version": 1, "kind": "real-task-randomized-pairs", "trials": args.trials,
              "python": sys.version, "platform": platform.platform(),
              "guion_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
              "text_stdlib_sha256": hashlib.sha256((ROOT / "language/stdlib/text.gu").read_bytes()).hexdigest(),
              "manifest_sha256": hashlib.sha256(MANIFEST.read_bytes()).hexdigest(),
              "measurement": "Sum of process and oracle-check time per case step; fixture setup excluded. "
                             "Guion source cache bypassed, OS cache uncontrolled. Each output verified.",
              "applications": {}}
    for name in args.application or list(APP_IDS):
        case = tasks[APP_IDS[name]]["initial"]["cases"][0]
        candidates = {
            "guion": ({"command": [str(binary), "run", "--no-cache", "${candidate}"],
                       "workspace_root_project": True, "env": {"GUION_FILE_ROOT": "${workspace}"}}, APPS / name),
            "python": ({"command": [sys.executable, "${candidate}/main.py"]}, PYTHON_REFERENCE),
        }
        samples = {"guion": [], "python": []}
        orders = []
        for pair in range(args.trials + 1):
            order = list(candidates)
            rng.shuffle(order)
            observations = {}
            for key in order:
                config, root = candidates[key]
                outcome = run_case(case, config, root, timeout_seconds=90)
                if not outcome["passed"]:
                    raise RuntimeError(f"{name} {key} failed acceptance: {outcome['failures']}")
                observations[key] = sum(step["elapsed_seconds"] for step in outcome["steps"])
            if pair:
                orders.append(order)
                for key in candidates:
                    samples[key].append(observations[key])
        ratios = [g / p for g, p in zip(samples["guion"], samples["python"], strict=True)]
        report["applications"][name] = {"case_id": case["id"], "samples_seconds": samples,
            "paired_order": orders, "median_seconds": {key: statistics.median(value) for key, value in samples.items()},
            "guion_over_python_median": statistics.median(ratios), "bootstrap_95_interval": interval(ratios, rng)}
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
        print(json.dumps({"application": name, "ratio": statistics.median(ratios)}), flush=True)


if __name__ == "__main__":
    main()
