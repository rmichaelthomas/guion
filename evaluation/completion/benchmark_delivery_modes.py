#!/usr/bin/env python3
"""Compare uncached, cached, packaged Guion and Python on matching cases."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import random
import shutil
import statistics
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT))
from evaluation.completion.compare_real_tasks import APP_IDS, APPS, MANIFEST, PYTHON_REFERENCE  # noqa: E402
from evaluation.protocol.case_runner import run_case  # noqa: E402


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--trials", type=int, default=7)
    parser.add_argument("--application", action="append", choices=sorted(APP_IDS))
    args = parser.parse_args()
    if args.trials < 3:
        parser.error("at least three trials are required")
    binary = args.binary.resolve(strict=True)
    scratch = ROOT / "target/perf-session/real-tasks-tmp"
    scratch.mkdir(parents=True, exist_ok=True)
    os.environ["TMPDIR"] = str(scratch)
    tempfile.tempdir = str(scratch)
    manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))
    tasks = {item["id"]: item for item in manifest["tasks"]}
    rng = random.Random(20261027)
    report = {"schema_version": 1, "kind": "real-task-delivery-modes", "trials": args.trials,
              "guion_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
              "measurement": "randomized pairs; process plus oracle-check time; setup excluded; "
                             "cached source copied from a verified warm cache; OS cache uncontrolled",
              "applications": {}}
    for name in args.application or list(APP_IDS):
        case = tasks[APP_IDS[name]]["initial"]["cases"][0]
        with tempfile.TemporaryDirectory(prefix=f"delivery-{name}-", dir=scratch) as temp:
            work = Path(temp)
            cached = work / "cached"
            shutil.copytree(APPS / name, cached, ignore=shutil.ignore_patterns("dist", ".guion"))
            environment = os.environ.copy()
            environment["GUION_FILE_ROOT"] = str(cached)
            command = [str(binary), "run", "--json", str(cached)]
            subprocess.run(command, env=environment, capture_output=True, text=True, timeout=90)
            warm = subprocess.run(command, env=environment, capture_output=True, text=True, timeout=90)
            try:
                cache_state = json.loads(warm.stdout).get("source_cache")
            except json.JSONDecodeError as error:
                raise RuntimeError(f"{name} cache preflight failed: {warm.stderr}") from error
            if cache_state != "hit":
                raise RuntimeError(f"{name} did not produce a verified warm source cache: {cache_state}")
            package = work / "package"
            subprocess.run([str(binary), "package", str(cached), "--output", str(package)],
                           capture_output=True, text=True, check=True, timeout=120)
            subprocess.run([str(binary), "verify-package", str(package)],
                           capture_output=True, text=True, check=True, timeout=120)
            empty = work / "empty"
            empty.mkdir()
            candidates = {
                "guion_uncached": ({"command": [str(binary), "run", "--no-cache", "${candidate}"],
                                    "workspace_root_project": True, "env": {"GUION_FILE_ROOT": "${workspace}"}}, APPS / name),
                "guion_cached": ({"command": [str(binary), "run", "${candidate}"],
                                  "workspace_root_project": True, "env": {"GUION_FILE_ROOT": "${workspace}"}}, cached),
                "guion_package": ({"command": [str(package / "run")],
                                   "env": {"GUION_FILE_ROOT": "${workspace}"}}, empty),
                "python": ({"command": [sys.executable, "${candidate}/main.py"]}, PYTHON_REFERENCE),
            }
            samples = {key: [] for key in candidates}
            orders = []
            for pair in range(args.trials + 1):
                order = list(candidates)
                rng.shuffle(order)
                observation = {}
                for key in order:
                    config, root = candidates[key]
                    result = run_case(case, config, root, timeout_seconds=90)
                    if not result["passed"]:
                        raise RuntimeError(f"{name} {key} failed acceptance: {result['failures']}")
                    observation[key] = sum(step["elapsed_seconds"] for step in result["steps"])
                if pair:
                    orders.append(order)
                    for key in candidates:
                        samples[key].append(observation[key])
            report["applications"][name] = {
                "case_id": case["id"], "cache_preflight": cache_state,
                "package_file_bytes": sum(p.stat().st_size for p in package.rglob("*") if p.is_file()),
                "samples_seconds": samples, "paired_order": orders,
                "median_seconds": {key: statistics.median(values) for key, values in samples.items()},
                "median_over_python": {key: statistics.median(a / b for a, b in zip(values, samples["python"], strict=True))
                                       for key, values in samples.items() if key != "python"},
            }
            args.output.parent.mkdir(parents=True, exist_ok=True)
            args.output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
            print(json.dumps({"application": name, "medians": report["applications"][name]["median_seconds"]}), flush=True)


if __name__ == "__main__":
    main()
