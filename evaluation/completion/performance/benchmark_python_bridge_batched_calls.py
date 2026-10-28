#!/usr/bin/env python3
"""Compare one batched Python bridge call with direct Python for repeated work."""

from __future__ import annotations

import json
import os
import platform
import random
import shutil
import statistics
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[3]
APP = ROOT / "evaluation/development/guion_python_bridge_batch_probe"
sys.path.insert(0, str(Path(__file__).parent))
from benchmark_python_bridge_startup import assert_expected, run_checked, sha256  # noqa: E402
from runner import paired_median_ci  # noqa: E402

CALL_COUNTS = (1, 10, 100, 1_000)


def main() -> int:
    import argparse

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=ROOT / "target/release/guion")
    parser.add_argument(
        "--python", type=Path, default=Path(os.environ.get("GUION_PYTHON", sys.executable))
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=ROOT
        / "evaluation/completion/results/real-task-sprint/2026-10-28-python-bridge-batched-calls.json",
    )
    parser.add_argument("--trials", type=int, default=30)
    parser.add_argument("--seed", type=int, default=20261049)
    args = parser.parse_args()
    if args.trials < 3:
        parser.error("at least three randomized pairs are required")

    binary = args.binary.resolve(strict=True)
    python = args.python.resolve(strict=True)
    scratch_parent = ROOT / "target/perf-session"
    scratch_parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="guion-python-bridge-batch-", dir=scratch_parent) as name:
        project = Path(name) / "bridge batch probe"
        shutil.copytree(APP, project)
        template = (project / "main.template.gu").read_text(encoding="utf-8")
        for count in CALL_COUNTS:
            source = template.replace("__CALL_COUNT__", str(count))
            if "__CALL_COUNT__" in source:
                raise RuntimeError("failed to substitute the batch call count")
            (project / f"main-{count}.gu").write_text(source, encoding="utf-8")

        payload = b"x" * 64
        payload_path = project / "payload.bin"
        payload_path.write_bytes(payload)
        environment = os.environ.copy()
        environment.update(
            {
                "GUION_PYTHON": str(python),
                "PYTHONDONTWRITEBYTECODE": "1",
                "TMPDIR": str(project),
            }
        )

        doctor = subprocess.run(
            [str(binary), "doctor", "--json"],
            cwd=project,
            env=environment,
            capture_output=True,
            timeout=15,
            check=False,
        )
        if doctor.returncode:
            raise RuntimeError(f"Guion doctor failed: {doctor.stdout!r}\n{doctor.stderr!r}")
        doctor_report = json.loads(doctor.stdout)
        if not doctor_report.get("capabilities", {}).get("python_calls"):
            raise RuntimeError("selected Guion CLI does not enable Python interoperability")

        versions = subprocess.run(
            [
                str(python),
                "-B",
                "-c",
                "import numpy, orjson, sys; print(sys.version.split()[0]); print(numpy.__version__); print(orjson.__version__)",
            ],
            cwd=project,
            env=environment,
            capture_output=True,
            text=True,
            timeout=15,
            check=False,
        )
        if versions.returncode:
            raise RuntimeError(f"Python package check failed: {versions.stdout!r}\n{versions.stderr!r}")
        python_version, numpy_version, orjson_version = versions.stdout.splitlines()
        if (python_version, numpy_version, orjson_version) != ("3.14.7", "2.5.1", "3.11.9"):
            raise RuntimeError(
                "comparison requires CPython 3.14.7, NumPy 2.5.1, and orjson 3.11.9; "
                f"found {python_version}, {numpy_version}, and {orjson_version}"
            )

        commands: dict[str, list[str]] = {}
        expected_outputs: dict[str, bytes] = {}
        for count in CALL_COUNTS:
            expected = f"{len(payload) * count}\n".encode()
            guion_lane = f"batch_{count}_guion"
            python_lane = f"batch_{count}_python"
            commands[guion_lane] = [
                str(binary),
                "run",
                "--no-cache",
                "--file-root",
                str(project),
                f"main-{count}.gu",
                "payload.bin",
            ]
            commands[python_lane] = [
                str(python),
                "-B",
                "reference.py",
                str(payload_path),
                str(count),
            ]
            expected_outputs[guion_lane] = expected
            expected_outputs[python_lane] = expected

        for lane, command in commands.items():
            _, result = run_checked(command, project, environment)
            assert_expected(lane, result, expected_outputs[lane])

        rng = random.Random(args.seed)
        samples: dict[str, list[float]] = {lane: [] for lane in commands}
        orders: list[list[str]] = []
        for _pair in range(args.trials):
            order = list(commands)
            rng.shuffle(order)
            orders.append(order)
            for lane in order:
                elapsed, result = run_checked(commands[lane], project, environment)
                assert_expected(lane, result, expected_outputs[lane])
                samples[lane].append(elapsed)

        medians = {lane: statistics.median(values) for lane, values in samples.items()}
        comparisons = {
            f"calls_{count}_guion_over_python": paired_median_ci(
                [
                    guion / reference
                    for guion, reference in zip(
                        samples[f"batch_{count}_guion"],
                        samples[f"batch_{count}_python"],
                        strict=True,
                    )
                ]
            )
            for count in CALL_COUNTS
        }
        guion_increment = [
            (many - one) / (CALL_COUNTS[-1] - CALL_COUNTS[0])
            for one, many in zip(
                samples[f"batch_{CALL_COUNTS[0]}_guion"],
                samples[f"batch_{CALL_COUNTS[-1]}_guion"],
                strict=True,
            )
        ]
        python_increment = [
            (many - one) / (CALL_COUNTS[-1] - CALL_COUNTS[0])
            for one, many in zip(
                samples[f"batch_{CALL_COUNTS[0]}_python"],
                samples[f"batch_{CALL_COUNTS[-1]}_python"],
                strict=True,
            )
        ]
        source_paths = (
            "crates/runtime/src/python_bridge.rs",
            "crates/runtime/python/sidecar.py",
            "evaluation/development/guion_python_bridge_batch_probe/main.template.gu",
            "evaluation/development/guion_python_bridge_batch_probe/bridge_batch.py",
            "evaluation/development/guion_python_bridge_batch_probe/reference.py",
            "evaluation/completion/performance/benchmark_python_bridge_batched_calls.py",
        )
        status = subprocess.run(
            ["git", "status", "--porcelain"], cwd=ROOT, capture_output=True, check=True
        ).stdout
        revision = subprocess.run(
            ["git", "rev-parse", "HEAD"], cwd=ROOT, capture_output=True, text=True, check=True
        ).stdout.strip()
        report: dict[str, Any] = {
            "schema_version": 1,
            "kind": "guion-python-bridge-batched-calls",
            "status": "passed",
            "commit": revision,
            "worktree_dirty_at_measurement": bool(status.strip()),
            "source_sha256": {path: sha256((ROOT / path).read_bytes()) for path in source_paths},
            "guion_binary_sha256": sha256(binary.read_bytes()),
            "guion_version": doctor_report["guion"],
            "host": platform.platform(),
            "python": {
                "executable": str(python),
                "version": python_version,
                "numpy": numpy_version,
                "orjson": orjson_version,
            },
            "workload": {
                "payload_bytes": len(payload),
                "iteration_counts": list(CALL_COUNTS),
                "bridge_calls_per_process": 1,
                "operation": "Pass one 64-byte input and a repeat count in one bridge request; sum its length count times in Python.",
                "python_package_imports": ["numpy", "orjson"],
            },
            "correctness": {
                "all_outputs_match_expected": True,
                "expected_output_by_iteration_count": {
                    str(count): len(payload) * count for count in CALL_COUNTS
                },
            },
            "measurement": {
                "trials": args.trials,
                "seed": args.seed,
                "paired_order": orders,
                "medians_seconds": medians,
                "samples_seconds": samples,
                "paired_comparisons": comparisons,
                "estimated_loop_increment_seconds_per_iteration": {
                    "guion_bridge": statistics.median(guion_increment),
                    "direct_python": statistics.median(python_increment),
                },
                "all_timed_outputs_checked": True,
                "method": "Fresh process per sample; randomized order across eight lanes; each stdout and stderr checked against expected output; both lanes call the same operation once; package imports identical; OS file-cache state uncontrolled.",
            },
        }
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        print(
            json.dumps(
                {
                    "status": report["status"],
                    "trials": args.trials,
                    "medians_seconds": medians,
                    "paired_comparisons": comparisons,
                    "report": str(args.output.resolve()),
                },
                sort_keys=True,
            )
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
