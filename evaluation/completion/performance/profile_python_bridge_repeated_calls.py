#!/usr/bin/env python3
"""Collect opt-in phase timings for repeated Python bridge calls."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import shutil
import statistics
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[3]
APP = ROOT / "evaluation/development/guion_python_bridge_repeat_probe"
PHASES = (
    "request_serialize",
    "request_write",
    "response_wait",
    "response_first_byte_wait",
    "reader_frame_after_first_byte",
    "channel_delivery",
    "response_json_parse",
    "result_decode",
    "request_frame_read",
    "request_setup",
    "request_parse",
    "request_validation_setup",
    "request_value_decode",
    "module_import_fallback",
    "module_dispatch",
    "function_call",
    "stream_restore",
    "result_encode",
    "response_object_setup",
    "output_capture",
    "response_native_prepare",
    "native_image_check",
    "native_image_snapshot",
    "response_envelope",
    "profile_payload_build",
    "response_json_encode",
    "request_to_response_first_write",
    "response_write",
)


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def profile_run_summaries(profiles: list[dict[str, Any]]) -> dict[str, Any]:
    summary: dict[str, Any] = {}
    for side, first_key, repeated_key in (
        ("rust", "rust_first_call_ns", "rust_repeated_calls_ns"),
        ("python", "first_call_ns", "repeated_calls_ns"),
    ):
        first_runs = [
            profile[first_key] if side == "rust" else profile["python"][first_key]
            for profile in profiles
        ]
        repeated_runs = [
            profile[repeated_key] if side == "rust" else profile["python"][repeated_key]
            for profile in profiles
        ]
        first_summary = {
            phase: statistics.median(run.get(phase, 0) for run in first_runs)
            for phase in PHASES
        }
        repeated_summary = {
            phase: statistics.median(
                run.get(phase, 0) / max(profile["calls"] - 1, 1)
                for run, profile in zip(repeated_runs, profiles, strict=True)
            )
            for phase in PHASES
        }
        summary[side] = {
            "first_call_median_ns": first_summary,
            "repeated_call_median_ns": repeated_summary,
        }
    return summary


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=ROOT / "target/release/guion")
    parser.add_argument(
        "--python", type=Path, default=Path(os.environ.get("GUION_PYTHON", sys.executable))
    )
    parser.add_argument("--calls", type=int, default=1_000)
    parser.add_argument("--runs", type=int, default=3)
    parser.add_argument(
        "--output",
        type=Path,
        default=ROOT
        / "evaluation/completion/results/real-task-sprint/2026-10-28-python-bridge-phase-profile.json",
    )
    args = parser.parse_args()
    if args.calls < 2 or args.runs < 1:
        parser.error("--calls must be at least 2 and --runs must be positive")

    binary = args.binary.resolve(strict=True)
    python = args.python.resolve(strict=True)
    scratch_parent = ROOT / "target/perf-session"
    scratch_parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="guion-bridge-profile-", dir=scratch_parent) as name:
        scratch = Path(name)
        project = scratch / "project"
        shutil.copytree(APP, project)
        source = (project / "main.template.gu").read_text(encoding="utf-8")
        source = source.replace("__CALL_COUNT__", str(args.calls))
        if "__CALL_COUNT__" in source:
            raise RuntimeError("failed to substitute the call count")
        (project / "main.gu").write_text(source, encoding="utf-8")
        payload = b"x" * 64
        (project / "payload.bin").write_bytes(payload)

        environment = os.environ.copy()
        environment.update(
            {
                "GUION_PYTHON": str(python),
                "PYTHONDONTWRITEBYTECODE": "1",
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
            raise RuntimeError(f"Guion doctor failed: {doctor.stdout!r}\n{doctor.stderr!r}")
        if not json.loads(doctor.stdout).get("capabilities", {}).get("python_calls"):
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
                "profiling requires CPython 3.14.7, NumPy 2.5.1, and orjson 3.11.9; "
                f"found {python_version}, {numpy_version}, and {orjson_version}"
            )

        profiles: list[dict[str, Any]] = []
        wall_times_seconds: list[float] = []
        expected = f"{len(payload) * args.calls}\n"
        for run_index in range(args.runs):
            profile_path = scratch / f"profile-{run_index}.json"
            run_environment = environment.copy()
            run_environment["GUION_PYTHON_BRIDGE_PROFILE_OUT"] = str(profile_path)
            started = time.perf_counter()
            result = subprocess.run(
                [
                    str(binary),
                    "run",
                    "--no-cache",
                    "--file-root",
                    str(project),
                    "main.gu",
                    "payload.bin",
                ],
                cwd=project,
                env=run_environment,
                capture_output=True,
                text=True,
                timeout=120,
                check=False,
            )
            wall_times_seconds.append(time.perf_counter() - started)
            if result.returncode:
                raise RuntimeError(f"Guion run {run_index} failed: {result.stderr!r}")
            if result.stdout != expected or result.stderr:
                raise RuntimeError(
                    f"Guion run {run_index} returned unexpected output: "
                    f"stdout={result.stdout!r}, stderr={result.stderr!r}"
                )
            if not profile_path.is_file():
                raise RuntimeError(f"Guion run {run_index} did not write its phase profile")
            profile = json.loads(profile_path.read_text(encoding="utf-8"))
            if (
                profile.get("status", "passed") != "passed"
                or profile.get("calls") != args.calls
                or profile.get("python", {}).get("calls") != args.calls
            ):
                raise RuntimeError(f"Guion run {run_index} wrote an incomplete profile: {profile!r}")
            profiles.append(profile)

        source_paths = (
            "crates/runtime/src/python_bridge.rs",
            "crates/runtime/python/sidecar.py",
            "evaluation/development/guion_python_bridge_repeat_probe/main.template.gu",
            "evaluation/development/guion_python_bridge_repeat_probe/bridge_probe.py",
            "evaluation/completion/performance/profile_python_bridge_repeated_calls.py",
            "evaluation/tests/test_python_bridge_profile.py",
        )
        status = subprocess.run(
            ["git", "status", "--porcelain"], cwd=ROOT, capture_output=True, check=True
        ).stdout
        revision = subprocess.run(
            ["git", "rev-parse", "HEAD"], cwd=ROOT, capture_output=True, text=True, check=True
        ).stdout.strip()
        report = {
            "schema_version": 1,
            "kind": "guion-python-bridge-phase-profile",
            "status": "passed",
            "commit": revision,
            "worktree_dirty_at_measurement": bool(status.strip()),
            "source_sha256": {path: sha256((ROOT / path).read_bytes()) for path in source_paths},
            "guion_binary_sha256": sha256(binary.read_bytes()),
            "host": platform.platform(),
            "python": {
                "executable": str(python),
                "version": python_version,
                "numpy": numpy_version,
                "orjson": orjson_version,
            },
            "workload": {
                "calls_per_run": args.calls,
                "runs": args.runs,
                "payload_bytes": len(payload),
                "operation": "Read a 64-byte file once, call payload_length repeatedly in one process, and print the sum.",
            },
            "correctness": {
                "all_outputs_match_expected": True,
                "expected_output": expected,
            },
            "measurement": {
                "wall_times_seconds": wall_times_seconds,
                "profiles": profiles,
                "phase_medians": profile_run_summaries(profiles),
                "method": (
                    f"{args.runs} opt-in diagnostic process runs of {args.calls} calls each. "
                    "First-call phases are separated from calls 2..N. Rust response_wait "
                    "runs from completed request write through main-thread receipt. "
                    "response_first_byte_wait, reader_frame_after_first_byte, and "
                    "channel_delivery partition that interval. Python request_frame_read "
                    "includes waiting for the next Guion request after a response, so it "
                    "does not align to the same Rust response interval. Python "
                    "response_json_encode and response_write include the diagnostic profile "
                    "payload and are not representative of normal unprofiled responses."
                ),
                "all_profiled_outputs_checked": True,
            },
        }
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        print(
            json.dumps(
                {
                    "status": report["status"],
                    "calls_per_run": args.calls,
                    "runs": args.runs,
                    "phase_medians": report["measurement"]["phase_medians"],
                    "report": str(args.output.resolve()),
                },
                sort_keys=True,
            )
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
