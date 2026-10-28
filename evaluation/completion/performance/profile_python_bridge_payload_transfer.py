#!/usr/bin/env python3
"""Attribute the first Python bridge call for empty and large byte attachments."""

from __future__ import annotations

import hashlib
import json
import os
import platform
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[3]
APP = ROOT / "evaluation/development/guion_python_bridge_probe"
SIZES = (0, 5_100_867)


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


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
        / "evaluation/completion/results/real-task-sprint/2026-10-28-python-bridge-payload-transfer-profile.json",
    )
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    python = args.python.resolve(strict=True)
    scratch = ROOT / "target/perf-session"
    scratch.mkdir(parents=True, exist_ok=True)

    with tempfile.TemporaryDirectory(prefix="guion-python-bridge-payload-profile-", dir=scratch) as name:
        project = Path(name) / "bridge payload profile"
        shutil.copytree(APP, project)
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

        package_check = subprocess.run(
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
        if package_check.returncode:
            raise RuntimeError(f"Python package check failed: {package_check.stdout!r}\n{package_check.stderr!r}")
        python_version, numpy_version, orjson_version = package_check.stdout.splitlines()
        if (python_version, numpy_version, orjson_version) != ("3.14.7", "2.5.1", "3.11.9"):
            raise RuntimeError(
                "profile requires CPython 3.14.7, NumPy 2.5.1, and orjson 3.11.9; "
                f"found {python_version}, {numpy_version}, and {orjson_version}"
            )

        profiles: dict[str, Any] = {}
        expected_output: dict[str, str] = {}
        elapsed_seconds: dict[str, float] = {}
        for size in SIZES:
            payload_name = f"payload-{size}.bin"
            (project / payload_name).write_bytes(b"x" * size)
            profile_path = project / f"profile-{size}.json"
            profile_environment = environment.copy()
            profile_environment["GUION_PYTHON_BRIDGE_PROFILE_OUT"] = str(profile_path)
            started = time.perf_counter()
            result = subprocess.run(
                [
                    str(binary),
                    "run",
                    "--no-cache",
                    "--file-root",
                    str(project),
                    "main.gu",
                    payload_name,
                ],
                cwd=project,
                env=profile_environment,
                capture_output=True,
                timeout=60,
                check=False,
            )
            elapsed_seconds[str(size)] = time.perf_counter() - started
            expected = f"{size}\n".encode()
            if result.returncode or result.stdout != expected or result.stderr:
                raise RuntimeError(
                    f"{size}-byte profile invocation failed: status={result.returncode}, "
                    f"stdout={result.stdout!r}, stderr={result.stderr!r}"
                )
            if not profile_path.is_file():
                raise RuntimeError(f"{size}-byte invocation did not write its bridge profile")
            profile = json.loads(profile_path.read_text(encoding="utf-8"))
            if (
                profile.get("kind") != "guion-python-bridge-phase-profile"
                or profile.get("python", {}).get("calls") != 1
            ):
                raise RuntimeError(f"unexpected {size}-byte bridge profile: {profile}")
            profiles[str(size)] = profile
            expected_output[str(size)] = expected.decode()

        status = subprocess.run(
            ["git", "status", "--porcelain"], cwd=ROOT, capture_output=True, check=True
        ).stdout
        revision = subprocess.run(
            ["git", "rev-parse", "HEAD"], cwd=ROOT, capture_output=True, text=True, check=True
        ).stdout.strip()
        source_paths = (
            "crates/runtime/src/python_bridge.rs",
            "crates/runtime/python/sidecar.py",
            "evaluation/development/guion_python_bridge_probe/main.gu",
            "evaluation/development/guion_python_bridge_probe/bridge_probe.py",
            "evaluation/completion/performance/profile_python_bridge_payload_transfer.py",
        )
        report: dict[str, Any] = {
            "schema_version": 1,
            "kind": "guion-python-bridge-payload-transfer-profile",
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
                "payload_sizes_bytes": list(SIZES),
                "calls_per_process": 1,
                "operation": "Read a Bytes file, send it to Python once, and return its length.",
            },
            "correctness": {
                "all_outputs_match_expected": True,
                "expected_stdout_by_size": expected_output,
            },
            "measurement": {
                "diagnostic_processes": len(SIZES),
                "wall_seconds_by_size": elapsed_seconds,
                "bridge_phase_profiles_by_size": profiles,
                "method": "One opt-in profile capture per payload size; no repeated timing cohort. Python frame-read and Rust request-write phases overlap while the pipe transfers data and must not be summed. The instrumentation adds work; use it to locate phases, not to compare wall speed.",
            },
        }
        output = args.output if args.output.is_absolute() else ROOT / args.output
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        print(
            json.dumps(
                {
                    "status": report["status"],
                    "wall_seconds_by_size": elapsed_seconds,
                    "profile_path": str(output.resolve()),
                },
                sort_keys=True,
            )
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
