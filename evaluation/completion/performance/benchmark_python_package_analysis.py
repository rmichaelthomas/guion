#!/usr/bin/env python3
"""Measure Guion's typed Python bridge on the 100,000-row NumPy/orjson task."""

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
APP = ROOT / "evaluation/development/package_analysis"
sys.path.insert(0, str(Path(__file__).parent))
from runner import paired_median_ci  # noqa: E402


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def run_checked(
    command: list[str], cwd: Path, environment: dict[str, str]
) -> tuple[float, subprocess.CompletedProcess[str]]:
    started = time.perf_counter()
    result = subprocess.run(
        command,
        cwd=cwd,
        env=environment,
        capture_output=True,
        text=True,
        timeout=120,
        check=False,
    )
    return time.perf_counter() - started, result


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=ROOT / "target/release/guion")
    parser.add_argument("--python", type=Path, default=Path(os.environ.get("GUION_PYTHON", sys.executable)))
    parser.add_argument(
        "--output",
        type=Path,
        default=ROOT / "evaluation/completion/results/real-task-sprint/2026-10-27-python-bridge-package-analysis.json",
    )
    parser.add_argument("--trials", type=int, default=7)
    parser.add_argument("--seed", type=int, default=20261027)
    args = parser.parse_args()
    if args.trials < 3:
        parser.error("at least three randomized pairs are required")

    binary = args.binary.resolve(strict=True)
    python = args.python.resolve(strict=True)
    scratch_parent = ROOT / "target/perf-session"
    scratch_parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="guion-python-package-analysis-", dir=scratch_parent) as name:
        work = Path(name)
        shutil.copyfile(APP / "main.vr", work / "main.gu")
        shutil.copyfile(APP / "analysis.py", work / "analysis.py")
        shutil.copyfile(APP / "reference.py", work / "reference.py")
        shutil.copyfile(APP / "reference_packages.py", work / "reference_packages.py")
        rows = [
            {
                "value": (index * 7919) % 50_003,
                "tag": f"é-{index % 13}",
                "department": f"dept-{index % 5}",
            }
            for index in range(100_000)
        ]
        dataset_path = work / "dataset.json"
        dataset_path.write_text(
            json.dumps({"rows": rows}, ensure_ascii=False, separators=(",", ":")),
            encoding="utf-8",
        )

        environment = os.environ.copy()
        environment.update(
            {
                "GUION_PYTHON": str(python),
                "PYTHONDONTWRITEBYTECODE": "1",
                "TMPDIR": str(work),
            }
        )
        doctor = subprocess.run(
            [str(binary), "doctor", "--json"],
            cwd=work,
            env=environment,
            capture_output=True,
            text=True,
            timeout=15,
            check=False,
        )
        if doctor.returncode:
            raise RuntimeError(f"Guion doctor failed: {doctor.stdout}\n{doctor.stderr}")
        doctor_report = json.loads(doctor.stdout)
        if not doctor_report.get("capabilities", {}).get("python_calls"):
            raise RuntimeError("selected Guion binary does not have Python interoperability enabled")

        package_versions = subprocess.run(
            [
                str(python),
                "-c",
                "import numpy, orjson, sys; print(sys.version.split()[0]); print(numpy.__version__); print(orjson.__version__)",
            ],
            cwd=work,
            env=environment,
            capture_output=True,
            text=True,
            timeout=15,
            check=False,
        )
        if package_versions.returncode:
            raise RuntimeError(
                "selected Python interpreter cannot import NumPy and orjson:\n"
                f"{package_versions.stdout}\n{package_versions.stderr}"
            )
        python_version, numpy_version, orjson_version = package_versions.stdout.splitlines()

        commands = {
            "guion": [
                str(binary),
                "run",
                "--no-cache",
                str(work / "main.gu"),
                "dataset.json",
                "guion-report.json",
                "tag",
            ],
            "python_packages": [
                str(python),
                "-B",
                str(work / "reference_packages.py"),
                "dataset.json",
                "python-packages-report.json",
                "tag",
            ],
            "python_stdlib": [
                str(python),
                "-B",
                str(work / "reference.py"),
                "dataset.json",
                "python-stdlib-report.json",
                "tag",
            ],
        }

        def observe(name: str) -> tuple[float, bytes, str, str]:
            report_path = work / f"{name.replace('_', '-')}-report.json"
            report_path.unlink(missing_ok=True)
            elapsed, completed = run_checked(commands[name], work, environment)
            if completed.returncode:
                raise RuntimeError(
                    f"{name} failed ({completed.returncode}):\n"
                    f"{completed.stdout}\n{completed.stderr}"
                )
            if not report_path.is_file():
                raise RuntimeError(f"{name} completed without writing its report")
            return elapsed, report_path.read_bytes(), completed.stdout, completed.stderr

        _, expected_report, expected_stdout, expected_stderr = observe("python_stdlib")
        expected = json.loads(expected_report)
        if expected.get("count") != 100_000 or len(expected.get("groups", {})) != 13:
            raise RuntimeError("Python reference produced an invalid 100,000-row result")
        _, guion_preflight, guion_stdout, guion_stderr = observe("guion")
        if (guion_preflight, guion_stdout, guion_stderr) != (
            expected_report,
            expected_stdout,
            expected_stderr,
        ):
            raise RuntimeError("Guion preflight output does not match the Python reference")
        _, package_report, package_stdout, package_stderr = observe("python_packages")
        if (package_report, package_stdout, package_stderr) != (
            expected_report,
            expected_stdout,
            expected_stderr,
        ):
            raise RuntimeError("NumPy/orjson preflight output does not match the Python reference")

        rng = random.Random(args.seed)
        samples = {name: [] for name in commands}
        orders: list[list[str]] = []
        for _ in range(args.trials):
            order = list(commands)
            rng.shuffle(order)
            orders.append(order)
            for name in order:
                elapsed, report_bytes, stdout, stderr = observe(name)
                if (report_bytes, stdout, stderr) != (
                    expected_report,
                    expected_stdout,
                    expected_stderr,
                ):
                    raise RuntimeError(f"{name} output mismatch in paired trial {len(orders)}")
                samples[name].append(elapsed)

        profile_environment = environment.copy()
        profile_environment["GUION_PHASE_TIMINGS"] = "1"
        profile_command = [
            str(binary),
            "run",
            "--json",
            "--no-cache",
            str(work / "main.gu"),
            "dataset.json",
            "profile-report.json",
            "tag",
        ]
        _, profile_result = run_checked(profile_command, work, profile_environment)
        if profile_result.returncode:
            raise RuntimeError(
                f"Guion phase profile failed ({profile_result.returncode}):\n"
                f"{profile_result.stdout}\n{profile_result.stderr}"
            )
        profile_envelope = json.loads(profile_result.stdout)
        profile_report_path = work / "profile-report.json"
        if (
            profile_envelope.get("status") != "passed"
            or profile_envelope.get("program_stdout") != expected_stdout
            or profile_report_path.read_bytes() != expected_report
        ):
            raise RuntimeError("Guion phase-profile output does not match the Python reference")

        ratios = {
            "guion_over_python_stdlib": [
                guion / reference
                for guion, reference in zip(
                    samples["guion"], samples["python_stdlib"], strict=True
                )
            ],
            "guion_over_python_packages": [
                guion / direct
                for guion, direct in zip(
                    samples["guion"], samples["python_packages"], strict=True
                )
            ],
            "python_packages_over_python_stdlib": [
                direct / reference
                for direct, reference in zip(
                    samples["python_packages"], samples["python_stdlib"], strict=True
                )
            ],
        }
        paired = {name: paired_median_ci(values) for name, values in ratios.items()}
        dataset_bytes = dataset_path.read_bytes()
        revision = subprocess.run(
            ["git", "rev-parse", "HEAD"],
            cwd=ROOT,
            capture_output=True,
            text=True,
            check=True,
        ).stdout.strip()
        source_paths = (
            "crates/runtime/src/python_bridge.rs",
            "crates/runtime/tests/python_bridge.rs",
            "evaluation/development/package_analysis/main.vr",
            "evaluation/development/package_analysis/analysis.py",
            "evaluation/development/package_analysis/reference.py",
            "evaluation/development/package_analysis/reference_packages.py",
            "evaluation/completion/performance/benchmark_python_package_analysis.py",
        )
        worktree_status = subprocess.run(
            ["git", "status", "--porcelain"],
            cwd=ROOT,
            capture_output=True,
            text=True,
            check=True,
        ).stdout
        report: dict[str, Any] = {
            "schema_version": 1,
            "kind": "guion-python-bridge-package-analysis",
            "status": "passed",
            "commit": revision,
            "worktree_dirty_at_measurement": bool(worktree_status.strip()),
            "source_sha256": {
                relative: sha256((ROOT / relative).read_bytes()) for relative in source_paths
            },
            "guion_binary_sha256": sha256(binary.read_bytes()),
            "guion_version": doctor_report["guion"],
            "host": platform.platform(),
            "python_executable": str(python),
            "python_version": python_version,
            "native_packages": {"numpy": numpy_version, "orjson": orjson_version},
            "workload": {
                "rows": len(rows),
                "groups": 13,
                "input_bytes": len(dataset_bytes),
                "input_sha256": sha256(dataset_bytes),
                "report_bytes": len(expected_report),
                "report_sha256": sha256(expected_report),
                "python_calls_per_guion_run": 1,
                "semantic_transfer_bytes_per_call": {
                    "input_text": dataset_path.stat().st_size,
                    "group_by_text": len("tag".encode("utf-8")),
                    "returned_json": len(expected_report),
                    "note": "Payload sizes are measured; protocol framing and serialization bytes are not currently exposed by the runtime.",
                },
            },
            "measurement": {
                "trials": args.trials,
                "seed": args.seed,
                "paired_order": orders,
                "medians_seconds": {name: statistics.median(values) for name, values in samples.items()},
                "samples_seconds": samples,
                "paired_ratios": paired,
                "untimed_guion_phase_profile": profile_envelope.get("phase_timings_ns"),
                "untimed_phase_profile_output_checked": True,
                "method": "Fresh process per sample; Guion source cache bypassed; one untimed output-checked preflight per lane; each timed output checked byte-for-byte; OS file cache uncontrolled.",
            },
            "correctness": {
                "all_preflight_report_bytes_match": True,
                "preflight_stdout_match": True,
                "all_timed_outputs_match": True,
                "expected_stdout": expected_stdout,
                "result_count": expected["count"],
                "result_total": expected["total"],
                "group_count": len(expected["groups"]),
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
                "paired_ratios": report["measurement"]["paired_ratios"],
                "input_bytes": report["workload"]["input_bytes"],
                "report": str(output),
            },
            sort_keys=True,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
