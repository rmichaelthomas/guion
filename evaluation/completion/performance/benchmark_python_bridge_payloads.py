#!/usr/bin/env python3
"""Compare current Guion Text and Bytes Python-bridge payload paths."""

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
APP = ROOT / "evaluation/development/guion_package_analysis"
sys.path.insert(0, str(Path(__file__).parent))
from runner import paired_median_ci  # noqa: E402


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def run_checked(
    command: list[str], cwd: Path, environment: dict[str, str], timeout: int = 120
) -> tuple[float, subprocess.CompletedProcess[str]]:
    started = time.perf_counter()
    result = subprocess.run(
        command,
        cwd=cwd,
        env=environment,
        capture_output=True,
        text=True,
        timeout=timeout,
        check=False,
    )
    return time.perf_counter() - started, result


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
        / "evaluation/completion/results/real-task-sprint/2026-10-27-python-bridge-bytes.json",
    )
    parser.add_argument("--trials", type=int, default=15)
    parser.add_argument("--seed", type=int, default=20261031)
    args = parser.parse_args()
    if args.trials < 3:
        parser.error("at least three randomized pairs are required")

    binary = args.binary.resolve(strict=True)
    python = args.python.resolve(strict=True)
    scratch_parent = ROOT / "target/perf-session"
    scratch_parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(
        prefix="guion-python-bridge-payloads-", dir=scratch_parent
    ) as name:
        work = Path(name)
        project = work / "guion package analysis"
        shutil.copytree(APP, project)
        rows = [
            {
                "value": (index * 7919) % 50_003,
                "tag": f"é-{index % 13}",
                "department": f"dept-{index % 5}",
            }
            for index in range(100_000)
        ]
        dataset_path = project / "dataset.json"
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
            cwd=project,
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
            raise RuntimeError("selected Guion CLI does not enable Python interoperability")

        package_versions = subprocess.run(
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
        if package_versions.returncode:
            raise RuntimeError(
                "selected Python interpreter cannot import NumPy and orjson:\n"
                f"{package_versions.stdout}\n{package_versions.stderr}"
            )
        python_version, numpy_version, orjson_version = package_versions.stdout.splitlines()
        if (python_version, numpy_version, orjson_version) != ("3.14.7", "2.5.1", "3.11.9"):
            raise RuntimeError(
                "comparison requires CPython 3.14.7, NumPy 2.5.1, and orjson 3.11.9; "
                f"found {python_version}, {numpy_version}, and {orjson_version}"
            )

        python_script = str(project / "reference_packages.py")
        stdlib_script = str(project / "reference.py")
        commands = {
            "guion_text": [
                str(binary),
                "run",
                "--no-cache",
                "--file-root",
                str(project),
                "main-text.gu",
                "dataset.json",
                "guion-text-report.json",
                "tag",
            ],
            "guion_bytes": [
                str(binary),
                "run",
                "--no-cache",
                "--file-root",
                str(project),
                "main-bytes.gu",
                "dataset.json",
                "guion-bytes-report.json",
                "tag",
            ],
            "python_text": [
                str(python), "-B", python_script, "dataset.json", "python-text-report.json", "tag"
            ],
            "python_bytes": [
                str(python),
                "-B",
                python_script,
                "dataset.json",
                "python-bytes-report.json",
                "tag",
                "--bytes",
            ],
            "python_stdlib": [
                str(python), "-B", stdlib_script, "dataset.json", "python-stdlib-report.json", "tag"
            ],
        }

        def observe(name: str) -> tuple[float, bytes, str, str]:
            report_name = {
                "guion_text": "guion-text-report.json",
                "guion_bytes": "guion-bytes-report.json",
                "python_text": "python-text-report.json",
                "python_bytes": "python-bytes-report.json",
                "python_stdlib": "python-stdlib-report.json",
            }[name]
            report_path = project / report_name
            report_path.unlink(missing_ok=True)
            elapsed, completed = run_checked(commands[name], project, environment)
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
            raise RuntimeError("Python standard-library reference produced an invalid result")
        for lane in commands:
            _, report_bytes, stdout, stderr = observe(lane)
            if (report_bytes, stdout, stderr) != (
                expected_report,
                expected_stdout,
                expected_stderr,
            ):
                raise RuntimeError(f"{lane} output does not match the independent Python reference")

        rng = random.Random(args.seed)
        samples = {lane: [] for lane in commands}
        orders: list[list[str]] = []
        for _ in range(args.trials):
            order = list(commands)
            rng.shuffle(order)
            orders.append(order)
            for lane in order:
                elapsed, report_bytes, stdout, stderr = observe(lane)
                if (report_bytes, stdout, stderr) != (
                    expected_report,
                    expected_stdout,
                    expected_stderr,
                ):
                    raise RuntimeError(f"{lane} output mismatch in pair {len(orders)}")
                samples[lane].append(elapsed)

        profile_timings: dict[str, Any] = {}
        for mode in ("text", "bytes"):
            profile_environment = environment.copy()
            profile_environment["GUION_PHASE_TIMINGS"] = "1"
            profile_path = project / f"profile-{mode}-report.json"
            profile_command = [
                str(binary),
                "run",
                "--json",
                "--no-cache",
                "--file-root",
                str(project),
                f"main-{mode}.gu",
                "dataset.json",
                profile_path.name,
                "tag",
            ]
            _, result = run_checked(profile_command, project, profile_environment)
            if result.returncode:
                raise RuntimeError(f"{mode} phase profile failed:\n{result.stdout}\n{result.stderr}")
            envelope = json.loads(result.stdout)
            if (
                envelope.get("status") != "passed"
                or envelope.get("program_stdout") != expected_stdout
                or profile_path.read_bytes() != expected_report
            ):
                raise RuntimeError(f"{mode} phase-profile output does not match the reference")
            profile_timings[mode] = envelope.get("phase_timings_ns")

        ratios = {
            "guion_text_over_python_text": [
                guion / reference
                for guion, reference in zip(samples["guion_text"], samples["python_text"], strict=True)
            ],
            "guion_bytes_over_python_bytes": [
                guion / reference
                for guion, reference in zip(samples["guion_bytes"], samples["python_bytes"], strict=True)
            ],
            "guion_bytes_over_guion_text": [
                byte / text
                for byte, text in zip(samples["guion_bytes"], samples["guion_text"], strict=True)
            ],
            "guion_text_over_python_stdlib": [
                guion / reference
                for guion, reference in zip(samples["guion_text"], samples["python_stdlib"], strict=True)
            ],
            "guion_bytes_over_python_stdlib": [
                guion / reference
                for guion, reference in zip(samples["guion_bytes"], samples["python_stdlib"], strict=True)
            ],
        }
        paired = {name: paired_median_ci(values) for name, values in ratios.items()}
        dataset_bytes = dataset_path.read_bytes()
        revision = subprocess.run(
            ["git", "rev-parse", "HEAD"], cwd=ROOT, capture_output=True, text=True, check=True
        ).stdout.strip()
        source_paths = (
            "crates/runtime/src/python_bridge.rs",
            "crates/runtime/python/sidecar.py",
            "crates/runtime/tests/python_bridge.rs",
            "evaluation/development/guion_package_analysis/main-text.gu",
            "evaluation/development/guion_package_analysis/main-bytes.gu",
            "evaluation/development/guion_package_analysis/analysis.py",
            "evaluation/development/guion_package_analysis/reference.py",
            "evaluation/development/guion_package_analysis/reference_packages.py",
            "evaluation/completion/performance/benchmark_python_bridge_payloads.py",
            "evaluation/tests/test_python_bridge_frame.py",
        )
        status = subprocess.run(
            ["git", "status", "--porcelain"], cwd=ROOT, capture_output=True, text=True, check=True
        ).stdout
        report: dict[str, Any] = {
            "schema_version": 1,
            "kind": "guion-python-bridge-payload-comparison",
            "status": "passed",
            "commit": revision,
            "worktree_dirty_at_measurement": bool(status.strip()),
            "source_sha256": {
                relative: sha256((ROOT / relative).read_bytes()) for relative in source_paths
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
                "rows": len(rows),
                "groups": 13,
                "input_bytes": len(dataset_bytes),
                "input_sha256": sha256(dataset_bytes),
                "report_bytes": len(expected_report),
                "report_sha256": sha256(expected_report),
                "python_calls_per_guion_run": 1,
                "payload_modes": {
                    "text": "UTF-8 file read followed by JSON-string request serialization",
                    "bytes": "raw file read followed by private-frame raw-byte attachments",
                },
            },
            "measurement": {
                "trials": args.trials,
                "seed": args.seed,
                "paired_order": orders,
                "medians_seconds": {
                    lane: statistics.median(values) for lane, values in samples.items()
                },
                "samples_seconds": samples,
                "paired_ratios": paired,
                "untimed_guion_phase_profile": profile_timings,
                "all_timed_outputs_checked": True,
                "method": "Fresh process per sample; randomized lane order; each timed output matched stdout and report bytes from both Python references; OS file cache uncontrolled.",
                "host_cpu_profile": "unavailable; see 2026-10-27-python-bridge-host-profile.md",
            },
            "correctness": {
                "all_outputs_match_standard_library_reference": True,
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
