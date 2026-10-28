#!/usr/bin/env python3
"""Measure fixed fresh-process Guion/Python bridge cost with a no-op Bytes call."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import random
import shutil
import statistics
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[3]
APP = ROOT / "evaluation/development/guion_python_bridge_probe"
sys.path.insert(0, str(Path(__file__).parent))
from runner import paired_median_ci  # noqa: E402


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def run_checked(
    command: list[str], cwd: Path, environment: dict[str, str]
) -> tuple[float, subprocess.CompletedProcess[bytes]]:
    started = time.perf_counter()
    result = subprocess.run(
        command,
        cwd=cwd,
        env=environment,
        capture_output=True,
        timeout=60,
        check=False,
    )
    return time.perf_counter() - started, result


def assert_expected(
    lane: str, result: subprocess.CompletedProcess[bytes], expected_stdout: bytes
) -> None:
    if result.returncode:
        raise RuntimeError(
            f"{lane} failed ({result.returncode}):\n"
            f"stdout={result.stdout!r}\nstderr={result.stderr!r}"
        )
    if result.stdout != expected_stdout or result.stderr:
        raise RuntimeError(
            f"{lane} output mismatch: stdout={result.stdout!r}, stderr={result.stderr!r}"
        )


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=ROOT / "target/release/guion")
    parser.add_argument(
        "--python", type=Path, default=Path(os.environ.get("GUION_PYTHON", sys.executable))
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=ROOT
        / "evaluation/completion/results/real-task-sprint/2026-10-28-python-bridge-startup-sizes.json",
    )
    parser.add_argument("--trials", type=int, default=15)
    parser.add_argument("--seed", type=int, default=20261033)
    args = parser.parse_args()
    if args.trials < 3:
        parser.error("at least three randomized pairs are required")

    binary = args.binary.resolve(strict=True)
    python = args.python.resolve(strict=True)
    scratch_parent = ROOT / "target/perf-session"
    scratch_parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="guion-python-bridge-startup-", dir=scratch_parent) as name:
        project = Path(name) / "bridge probe"
        shutil.copytree(APP, project)
        rows = [
            {
                "value": (index * 7919) % 50_003,
                "tag": f"é-{index % 13}",
                "department": f"dept-{index % 5}",
            }
            for index in range(100_000)
        ]
        dataset = project / "dataset.json"
        dataset.write_text(
            json.dumps({"rows": rows}, ensure_ascii=False, separators=(",", ":")),
            encoding="utf-8",
        )
        dataset_bytes = dataset.read_bytes()
        empty_input = project / "empty.bin"
        empty_input.write_bytes(b"")
        payloads = {
            "empty": (empty_input.name, b""),
            "5mb": (dataset.name, dataset_bytes),
        }
        expected_outputs = {
            f"{size}_{language}_noop": f"{len(payload)}\n".encode()
            for size, (_filename, payload) in payloads.items()
            for language in ("guion", "python")
        }

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
            timeout=15,
            check=False,
        )
        if versions.returncode:
            raise RuntimeError(f"Python dependency check failed: {versions.stdout!r}\n{versions.stderr!r}")
        python_version, numpy_version, orjson_version = versions.stdout.decode().splitlines()
        if (python_version, numpy_version, orjson_version) != ("3.14.7", "2.5.1", "3.11.9"):
            raise RuntimeError(
                "comparison requires CPython 3.14.7, NumPy 2.5.1, and orjson 3.11.9; "
                f"found {python_version}, {numpy_version}, and {orjson_version}"
            )

        commands: dict[str, list[str]] = {}
        for size, (filename, _payload) in payloads.items():
            commands[f"{size}_guion_noop"] = [
                str(binary),
                "run",
                "--no-cache",
                "--file-root",
                str(project),
                "main.gu",
                filename,
            ]
            commands[f"{size}_python_noop"] = [
                str(python),
                "-B",
                "reference.py",
                filename,
            ]
        for lane, command in commands.items():
            _, result = run_checked(command, project, environment)
            assert_expected(lane, result, expected_outputs[lane])

        rng = random.Random(args.seed)
        samples: dict[str, list[float]] = {lane: [] for lane in commands}
        orders: list[list[str]] = []
        for pair in range(args.trials):
            order = list(commands)
            rng.shuffle(order)
            orders.append(order)
            for lane in order:
                elapsed, result = run_checked(commands[lane], project, environment)
                assert_expected(lane, result, expected_outputs[lane])
                samples[lane].append(elapsed)

        paired_comparisons = {
            f"{size}_bytes_guion_over_python": paired_median_ci(
                [
                    guion / python
                    for guion, python in zip(
                        samples[f"{size}_guion_noop"],
                        samples[f"{size}_python_noop"],
                        strict=True,
                    )
                ]
            )
            for size in payloads
        }
        paired_comparisons["payload_penalty_guion_over_python"] = paired_median_ci(
            [
                (guion_5mb / guion_empty) / (python_5mb / python_empty)
                for guion_empty, python_empty, guion_5mb, python_5mb in zip(
                    samples["empty_guion_noop"],
                    samples["empty_python_noop"],
                    samples["5mb_guion_noop"],
                    samples["5mb_python_noop"],
                    strict=True,
                )
            ]
        )
        source_paths = (
            "crates/runtime/src/python_bridge.rs",
            "crates/runtime/python/sidecar.py",
            "evaluation/development/guion_python_bridge_probe/bridge_probe.py",
            "evaluation/development/guion_python_bridge_probe/main.gu",
            "evaluation/development/guion_python_bridge_probe/reference.py",
            "evaluation/completion/performance/benchmark_python_bridge_startup.py",
        )
        status = subprocess.run(
            ["git", "status", "--porcelain"], cwd=ROOT, capture_output=True, check=True
        ).stdout
        revision = subprocess.run(
            ["git", "rev-parse", "HEAD"], cwd=ROOT, capture_output=True, text=True, check=True
        ).stdout.strip()
        report: dict[str, Any] = {
            "schema_version": 1,
            "kind": "guion-python-bridge-startup-probe",
            "status": "passed",
            "commit": revision,
            "worktree_dirty_at_measurement": bool(status.strip()),
            "source_sha256": {
                path: sha256((ROOT / path).read_bytes()) for path in source_paths
            },
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
                "rows_in_large_input": len(rows),
                "input_bytes_by_size": {
                    size: len(payload) for size, (_filename, payload) in payloads.items()
                },
                "input_sha256_by_size": {
                    size: sha256(payload) for size, (_filename, payload) in payloads.items()
                },
                "operation": "Read an empty or 5.1 MB file as bytes, call Python len once, print the result.",
                "python_package_imports": ["numpy", "orjson"],
            },
            "measurement": {
                "trials": args.trials,
                "seed": args.seed,
                "paired_order": orders,
                "medians_seconds": {
                    lane: statistics.median(values) for lane, values in samples.items()
                },
                "samples_seconds": samples,
                "paired_comparisons": paired_comparisons,
                "all_timed_outputs_checked": True,
                "method": "Fresh process per sample; randomized order across four lanes; each stdout and stderr checked against the expected byte count; OS file cache uncontrolled.",
            },
            "correctness": {
                "all_outputs_match_expected": True,
                "expected_output_by_size": {
                    size: {
                        "byte_count": len(payload),
                        "stdout_sha256": sha256(expected_outputs[f"{size}_guion_noop"]),
                    }
                    for size, (_filename, payload) in payloads.items()
                },
            },
        }

    output = args.output if args.output.is_absolute() else ROOT / args.output
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(
        json.dumps(
            {
                "status": report["status"],
                "medians_seconds": report["measurement"]["medians_seconds"],
                "paired_comparisons": report["measurement"]["paired_comparisons"],
                "input_bytes_by_size": report["workload"]["input_bytes_by_size"],
                "report": str(output),
            },
            sort_keys=True,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
