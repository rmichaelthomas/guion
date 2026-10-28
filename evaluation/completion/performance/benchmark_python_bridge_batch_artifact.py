#!/usr/bin/env python3
"""Separate source compilation from runtime cost for the batched bridge probe."""

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

CALL_COUNT = 1_000


def paired_median_difference_ci(values: list[float], seed: int) -> dict[str, Any]:
    rng = random.Random(seed)
    medians = [
        statistics.median(values[rng.randrange(len(values))] for _ in values)
        for _ in range(20_000)
    ]
    medians.sort()
    return {
        "median_ms": statistics.median(values) * 1_000,
        "ci95_percentile_ms": [
            medians[int(0.025 * (len(medians) - 1))] * 1_000,
            medians[int(0.975 * (len(medians) - 1))] * 1_000,
        ],
        "positive_in_all_pairs": all(value > 0 for value in values),
    }


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
        / "evaluation/completion/results/real-task-sprint/2026-10-28-python-bridge-batch-artifact.json",
    )
    parser.add_argument("--trials", type=int, default=30)
    parser.add_argument("--seed", type=int, default=20261050)
    args = parser.parse_args()
    if args.trials < 3:
        parser.error("at least three randomized triplets are required")

    binary = args.binary.resolve(strict=True)
    python = args.python.resolve(strict=True)
    scratch_parent = ROOT / "target/perf-session"
    scratch_parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="guion-python-bridge-batch-artifact-", dir=scratch_parent) as name:
        project = Path(name) / "bridge batch artifact probe"
        shutil.copytree(APP, project)
        template = (project / "main.template.gu").read_text(encoding="utf-8")
        source = project / f"main-{CALL_COUNT}.gu"
        source_text = template.replace("__CALL_COUNT__", str(CALL_COUNT))
        if "__CALL_COUNT__" in source_text:
            raise RuntimeError("failed to substitute the batch call count")
        source.write_text(source_text, encoding="utf-8")

        payload = b"x" * 64
        payload_path = project / "payload.bin"
        payload_path.write_bytes(payload)
        artifact = project / f"main-{CALL_COUNT}.gub"
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

        build_seconds, build_result = run_checked(
            [str(binary), "build", str(source), "--output", str(artifact)], project, environment
        )
        if build_result.returncode or not artifact.is_file():
            raise RuntimeError(
                f"artifact build failed ({build_result.returncode}):\n"
                f"stdout={build_result.stdout!r}\nstderr={build_result.stderr!r}"
            )

        expected = f"{len(payload) * CALL_COUNT}\n".encode()
        commands = {
            "source_uncached": [
                str(binary),
                "run",
                "--no-cache",
                "--file-root",
                str(project),
                str(source),
                "payload.bin",
            ],
            "prebuilt_artifact": [
                str(binary),
                "run",
                "--file-root",
                str(project),
                str(artifact),
                "payload.bin",
            ],
            "direct_python": [
                str(python),
                "-B",
                "reference.py",
                str(payload_path),
                str(CALL_COUNT),
            ],
        }
        for lane, command in commands.items():
            _, result = run_checked(command, project, environment)
            assert_expected(lane, result, expected)

        rng = random.Random(args.seed)
        samples: dict[str, list[float]] = {lane: [] for lane in commands}
        orders: list[list[str]] = []
        for _trial in range(args.trials):
            order = list(commands)
            rng.shuffle(order)
            orders.append(order)
            for lane in order:
                elapsed, result = run_checked(commands[lane], project, environment)
                assert_expected(lane, result, expected)
                samples[lane].append(elapsed)

        medians = {lane: statistics.median(values) for lane, values in samples.items()}
        comparisons = {
            f"{candidate}_over_direct_python": paired_median_ci(
                [
                    candidate_time / python_time
                    for candidate_time, python_time in zip(
                        samples[candidate], samples["direct_python"], strict=True
                    )
                ]
            )
            for candidate in ("source_uncached", "prebuilt_artifact")
        }
        source_artifact_differences = [
            source_time - artifact_time
            for source_time, artifact_time in zip(
                samples["source_uncached"], samples["prebuilt_artifact"], strict=True
            )
        ]
        source_artifact_comparison = {
            "source_over_artifact": paired_median_ci(
                [
                    source_time / artifact_time
                    for source_time, artifact_time in zip(
                        samples["source_uncached"], samples["prebuilt_artifact"], strict=True
                    )
                ]
            ),
            "source_minus_artifact_ms": paired_median_difference_ci(
                source_artifact_differences, args.seed + 1
            ),
        }
        source_paths = (
            "crates/runtime/src/python_bridge.rs",
            "crates/runtime/python/sidecar.py",
            "evaluation/development/guion_python_bridge_batch_probe/main.template.gu",
            "evaluation/development/guion_python_bridge_batch_probe/bridge_batch.py",
            "evaluation/development/guion_python_bridge_batch_probe/reference.py",
            "evaluation/completion/performance/benchmark_python_bridge_batch_artifact.py",
        )
        status = subprocess.run(
            ["git", "status", "--porcelain"], cwd=ROOT, capture_output=True, check=True
        ).stdout
        revision = subprocess.run(
            ["git", "rev-parse", "HEAD"], cwd=ROOT, capture_output=True, text=True, check=True
        ).stdout.strip()
        report: dict[str, Any] = {
            "schema_version": 1,
            "kind": "guion-python-bridge-batched-source-artifact",
            "status": "passed",
            "commit": revision,
            "worktree_dirty_at_measurement": bool(status.strip()),
            "source_sha256": {path: sha256((ROOT / path).read_bytes()) for path in source_paths},
            "guion_binary_sha256": sha256(binary.read_bytes()),
            "artifact_sha256": sha256(artifact.read_bytes()),
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
                "iteration_count": CALL_COUNT,
                "bridge_calls_per_process": 1,
                "operation": "Read a 64-byte file, call the same Python payload_length helper 1,000 times, sum its results, and print the total.",
                "python_package_imports": ["numpy", "orjson"],
            },
            "correctness": {
                "all_outputs_match_expected": True,
                "expected_output": len(payload) * CALL_COUNT,
                "all_timed_outputs_checked": True,
            },
            "measurement": {
                "trials": args.trials,
                "seed": args.seed,
                "paired_order": orders,
                "medians_seconds": medians,
                "samples_seconds": samples,
                "paired_comparisons": comparisons,
                "source_vs_artifact": source_artifact_comparison,
                "artifact_build_seconds_once_outside_timing": build_seconds,
                "method": "Fresh process per sample; 30 randomized triplets; uncached Guion source, prebuilt Guion artifact, and direct Python run the same operation and import the same packages. Artifact compilation occurs once outside the timed samples. Every stdout and stderr is checked. OS file-cache state is uncontrolled.",
            },
        }
        output = args.output if args.output.is_absolute() else ROOT / args.output
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        print(
            json.dumps(
                {
                    "status": report["status"],
                    "medians_seconds": medians,
                    "paired_comparisons": comparisons,
                    "artifact_build_seconds_once_outside_timing": build_seconds,
                    "report": str(output.resolve()),
                },
                sort_keys=True,
            )
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
