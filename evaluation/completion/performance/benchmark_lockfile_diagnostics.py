#!/usr/bin/env python3
"""Measure output-checked lockfile diagnostic sorting at increasing scales."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import random
import statistics
import subprocess
import sys
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT))
sys.path.insert(0, str(ROOT / "evaluation/completion"))
from compare_real_tasks import APPS, PYTHON_REFERENCE  # noqa: E402
from evaluation.protocol.case_runner import run_case  # noqa: E402


def interval(values: list[float], rng: random.Random) -> list[float]:
    medians = sorted(statistics.median(rng.choices(values, k=len(values))) for _ in range(5000))
    return [medians[124], medians[4874]]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--baseline-commit", default="8e1b43b")
    parser.add_argument("--sizes", nargs="+", type=int, default=[64, 128, 256, 512])
    parser.add_argument("--trials", type=int, default=7)
    args = parser.parse_args()
    if args.trials < 3 or any(size < 1 for size in args.sizes):
        parser.error("trials must be at least 3 and sizes must be positive")

    binary = args.binary.resolve(strict=True)
    scratch = ROOT / "target/perf-session/lockfile-scale"
    scratch.mkdir(parents=True, exist_ok=True)
    temp_root = ROOT / "target/perf-session/real-tasks-tmp"
    temp_root.mkdir(parents=True, exist_ok=True)
    os.environ["TMPDIR"] = str(temp_root)
    tempfile.tempdir = str(temp_root)

    old_source = subprocess.run(
        ["git", "show", f"{args.baseline_commit}:evaluation/completion/applications/lockfile-auditor/main.vr"],
        cwd=ROOT,
        text=True,
        capture_output=True,
        check=True,
    ).stdout
    baseline_root = scratch / "baseline-app"
    baseline_root.mkdir(parents=True, exist_ok=True)
    (baseline_root / "main.vr").write_text(old_source, encoding="utf-8")
    for filename in ("veritas.toml", "veritas.lock"):
        content = subprocess.run(
            [
                "git",
                "show",
                f"{args.baseline_commit}:evaluation/completion/applications/lockfile-auditor/{filename}",
            ],
            cwd=ROOT,
            text=True,
            capture_output=True,
            check=True,
        ).stdout
        (baseline_root / filename).write_text(content, encoding="utf-8")

    rng = random.Random(20261027)
    report = {
        "schema_version": 1,
        "kind": "lockfile-diagnostic-scaling",
        "baseline_commit": args.baseline_commit,
        "baseline_source_sha256": hashlib.sha256(old_source.encode()).hexdigest(),
        "candidate_source_sha256": hashlib.sha256(
            (APPS / "lockfile-auditor/main.vr").read_bytes()
        ).hexdigest(),
        "guion_binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
        "platform": platform.platform(),
        "python": sys.version,
        "trials": args.trials,
        "measurement": (
            "Fresh-process source runs with Guion source cache bypassed; fixture setup excluded. "
            "Every report is checked against the expected sorted JSON. OS file cache is uncontrolled."
        ),
        "workloads": {},
    }

    def observe(case: dict, config: dict, candidate_root: Path) -> dict:
        start = time.perf_counter()
        outcome = run_case(case, config, candidate_root, timeout_seconds=90)
        return {
            "passed": outcome["passed"],
            "elapsed_seconds": sum(step["elapsed_seconds"] for step in outcome["steps"]),
            "failures": outcome["failures"],
            "stderr": outcome["stderr"][:500],
            "runner_wall_seconds": time.perf_counter() - start,
        }

    for package_count in args.sizes:
        packages = [{"name": f"pkg-{index:04d}", "version": "1.0.0"} for index in range(package_count)]
        expected = {
            "violations": [
                {"name": f"pkg-{index:04d}", "rule": rule}
                for index in range(package_count)
                for rule in ("missing_checksum", "missing_source")
            ],
            "warnings": [],
        }
        command_args = ["--lockfile", "lock.json", "--policy", "policy.json", "--output", "report.json"]
        case = {
            "id": f"lockfile-diagnostics-{package_count}",
            "files": {
                "lock.json": json.dumps({"packages": packages}, separators=(",", ":")),
                "policy.json": json.dumps(
                    {"require_source": True, "require_checksum": True}, separators=(",", ":")
                ),
            },
            "command_args": command_args,
            "expected_exit_code": 0,
            "checks": [{"kind": "json_equals", "file": "report.json", "value": expected}],
        }
        candidates = {
            "baseline_source": (
                {
                    "command": [str(binary), "run", "--no-cache", "${candidate}", *command_args],
                    "workspace_root_project": True,
                    "env": {"GUION_FILE_ROOT": "${workspace}"},
                },
                baseline_root,
            ),
            "candidate_source": (
                {
                    "command": [str(binary), "run", "--no-cache", "${candidate}", *command_args],
                    "workspace_root_project": True,
                    "env": {"GUION_FILE_ROOT": "${workspace}"},
                },
                APPS / "lockfile-auditor",
            ),
            "python": ({"command": [sys.executable, "${candidate}/main.py", *command_args]}, PYTHON_REFERENCE),
        }

        preflight = {
            name: observe(case, config, candidate_root)
            for name, (config, candidate_root) in candidates.items()
        }
        active = ["candidate_source", "python"]
        if preflight["baseline_source"]["passed"]:
            active.insert(0, "baseline_source")
        if not preflight["candidate_source"]["passed"] or not preflight["python"]["passed"]:
            report["workloads"][str(package_count)] = {
                "packages": package_count,
                "diagnostics": len(expected["violations"]),
                "preflight": preflight,
                "status": "failed",
            }
            args.output.parent.mkdir(parents=True, exist_ok=True)
            args.output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
            return 1

        samples = {name: [] for name in active}
        orders = []
        for _ in range(args.trials):
            order = active[:]
            rng.shuffle(order)
            observed = {}
            for name in order:
                config, candidate_root = candidates[name]
                item = observe(case, config, candidate_root)
                if not item["passed"]:
                    raise RuntimeError(f"{package_count} packages: {name} failed: {item['failures']}")
                observed[name] = item["elapsed_seconds"]
            orders.append(order)
            for name in active:
                samples[name].append(observed[name])

        ratios = {
            "candidate_over_python": [
                candidate / python
                for candidate, python in zip(
                    samples["candidate_source"], samples["python"], strict=True
                )
            ]
        }
        if "baseline_source" in samples:
            ratios["candidate_over_baseline"] = [
                candidate / baseline
                for candidate, baseline in zip(
                    samples["candidate_source"], samples["baseline_source"], strict=True
                )
            ]
        workload = {
            "packages": package_count,
            "diagnostics": len(expected["violations"]),
            "preflight": preflight,
            "samples_seconds": samples,
            "paired_order": orders,
            "median_seconds": {name: statistics.median(value) for name, value in samples.items()},
            "ratios": {},
            "all_outputs_verified": True,
            "status": "passed",
        }
        for name, values in ratios.items():
            workload["ratios"][name] = {
                "median": statistics.median(values),
                "bootstrap_95_interval": interval(values, rng),
            }
        report["workloads"][str(package_count)] = workload
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
        print(json.dumps({"packages": package_count, "status": "passed", "ratios": workload["ratios"]}), flush=True)

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
