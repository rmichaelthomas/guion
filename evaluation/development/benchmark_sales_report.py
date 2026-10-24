#!/usr/bin/env python3
"""Measure a generated JSON aggregation workload on Veritas and Python.

This author-run probe is exploratory evidence, not an independent language
recommendation study. It includes process startup, artifact loading, parsing,
and execution; fixture creation and compilation are recorded separately.
"""

from __future__ import annotations

import argparse
import json
import math
import os
import platform
import shutil
import statistics
import subprocess
import sys
import tempfile
import time
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
SOURCE = ROOT / "evaluation/development/sales_report/main.vr"
PYTHON_REFERENCE = ROOT / "evaluation/development/sales_report/reference.py"
DEFAULT_REPORT = ROOT / "evaluation/results/sales-report-runtime-benchmark.json"


def percentile(samples: list[float], quantile: float) -> float:
    if not samples:
        raise ValueError("at least one timing sample is required")
    if not 0 <= quantile <= 100:
        raise ValueError("quantile must be between 0 and 100")
    ordered = sorted(samples)
    position = (len(ordered) - 1) * quantile / 100
    lower = math.floor(position)
    upper = math.ceil(position)
    if lower == upper:
        return ordered[lower]
    fraction = position - lower
    return ordered[lower] * (1 - fraction) + ordered[upper] * fraction


def summarize(samples: list[float]) -> dict[str, object]:
    if not samples:
        raise ValueError("at least one timing sample is required")
    rounded = [round(sample, 9) for sample in samples]
    return {
        "samples_seconds": rounded,
        "min_seconds": min(rounded),
        "median_seconds": round(statistics.median(samples), 9),
        "p95_seconds": round(percentile(samples, 95), 9),
        "max_seconds": max(rounded),
    }


def runtime_commands(binary: Path, project: Path, artifact: Path) -> dict[str, list[str]]:
    return {
        "veritas_source": [str(binary), "run", str(project / "main.vr"), "sales.json"],
        "veritas_artifact": [str(binary), "run", str(artifact), "sales.json"],
        "python_standard_library": [sys.executable, str(project / "reference.py"), "sales.json"],
    }


def run_timed(
    command: list[str], cwd: Path, timeout_seconds: float = 30
) -> tuple[float, subprocess.CompletedProcess[str]]:
    start = time.perf_counter()
    try:
        completed = subprocess.run(
            command,
            cwd=cwd,
            text=True,
            capture_output=True,
            check=False,
            timeout=timeout_seconds,
        )
    except subprocess.TimeoutExpired as error:
        elapsed = time.perf_counter() - start
        raise RuntimeError(
            f"benchmark command exceeded {timeout_seconds:g}s: {command[0]} {command[1]}"
        ) from error
    elapsed = time.perf_counter() - start
    return elapsed, completed


def require_success(label: str, completed: subprocess.CompletedProcess[str], expected: str) -> None:
    if completed.returncode != 0:
        raise RuntimeError(
            f"{label} exited {completed.returncode}: {completed.stderr.strip()}"
        )
    if completed.stdout != expected:
        raise RuntimeError(
            f"{label} output mismatch: expected {expected!r}, got {completed.stdout!r}"
        )


def ensure_cli(binary: Path) -> tuple[float, bool]:
    if binary.is_file():
        return 0.0, False
    start = time.perf_counter()
    subprocess.run(
        ["cargo", "build", "--release", "--offline", "-p", "veritas-cli"],
        cwd=ROOT,
        check=True,
    )
    elapsed = time.perf_counter() - start
    return elapsed, True


def benchmark(records: int, repetitions: int, binary: Path) -> dict[str, object]:
    if records < 1 or repetitions < 2:
        raise ValueError("records must be positive and repetitions must be at least 2")
    binary_build_seconds, built_cli = ensure_cli(binary)
    with tempfile.TemporaryDirectory(prefix="veritas-sales-benchmark-") as temporary:
        project = Path(temporary)
        shutil.copy2(SOURCE, project / "main.vr")
        shutil.copy2(PYTHON_REFERENCE, project / "reference.py")
        sales = [{"amount": (index * 37) % 1001} for index in range(records)]
        fixture = project / "sales.json"
        fixture.write_text(json.dumps({"sales": sales}, separators=(",", ":")), encoding="utf-8")
        expected = f"{records}\n{sum(sale['amount'] for sale in sales)}\n"
        artifact = project / "sales_report.vrb"

        compile_seconds, compiled = run_timed(
            [str(binary), "build", str(project / "main.vr"), "--output", str(artifact)],
            project,
        )
        if compiled.returncode != 0:
            raise RuntimeError(f"Veritas artifact build failed: {compiled.stderr.strip()}")
        artifact_bytes = artifact.stat().st_size

        commands = runtime_commands(binary, project, artifact)
        first_run: dict[str, float] = {}
        samples: dict[str, list[float]] = {name: [] for name in commands}
        for name, command in commands.items():
            elapsed, completed = run_timed(command, project)
            require_success(name, completed, expected)
            first_run[name] = elapsed

        for iteration in range(repetitions):
            ordered = list(commands.items())
            if iteration % 2:
                ordered.reverse()
            for name, command in ordered:
                elapsed, completed = run_timed(command, project)
                require_success(name, completed, expected)
                samples[name].append(elapsed)

        return {
            "schema_version": 1,
            "classification": "author_run_synthetic_runtime_probe_not_independent_evidence",
            "host": {
                "system": platform.system(),
                "release": platform.release(),
                "machine": platform.machine(),
                "processor": platform.processor(),
                "cpu_count": os.cpu_count(),
                "python": sys.version.split()[0],
            },
            "veritas": {
                "version_output": subprocess.run(
                    [str(binary), "--version"], text=True, capture_output=True, check=True
                ).stdout.strip(),
                "binary_path": str(binary),
                "binary_built_for_probe": built_cli,
                "binary_build_seconds": round(binary_build_seconds, 9),
                "artifact_bytes": artifact_bytes,
                "artifact_compile_seconds": round(compile_seconds, 9),
            },
            "workload": {
                "name": "sales_report_generated_json_sum",
                "records": records,
                "input_bytes": fixture.stat().st_size,
                "expected_stdout": expected,
                "warmup_runs_per_implementation": 1,
                "measured_repetitions": repetitions,
                "measured_scope": "child process startup and the selected implementation's run path, including JSON parsing and aggregation",
                "per_implementation_scope": {
                    "veritas_source": "source read, parse, typecheck, compilation, VM load, JSON parsing, and execution on every invocation",
                    "veritas_artifact": "artifact load, JSON parsing, and execution; artifact compilation is excluded and reported separately",
                    "python_standard_library": "Python interpreter startup, source import/parse, JSON parsing, and execution",
                },
                "excluded_scope": "fixture generation, Veritas Rust CLI build, and the separate veritas build command (the source-run command still compiles each invocation)",
            },
            "first_run_seconds": first_run,
            "warm_runtime": {name: summarize(values) for name, values in samples.items()},
            "correctness_checked_on_every_run": True,
            "recommendation_claim": False,
        }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--records", type=int, default=1_000)
    parser.add_argument("--repetitions", type=int, default=5)
    parser.add_argument("--binary", type=Path, default=ROOT / "target/release/veritas")
    parser.add_argument("--report", type=Path, default=DEFAULT_REPORT)
    args = parser.parse_args()
    report = benchmark(args.records, args.repetitions, args.binary.resolve())
    args.report.parent.mkdir(parents=True, exist_ok=True)
    args.report.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    for name, result in report["warm_runtime"].items():
        print(f"{name}: median {result['median_seconds']:.6f}s; p95 {result['p95_seconds']:.6f}s")
    print(f"wrote {args.report}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
