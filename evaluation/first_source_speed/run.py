#!/usr/bin/env python3
"""Paired fresh-process source-run benchmarks for Guion and Python."""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
import platform
import random
import re
import shutil
import statistics
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Sequence


REPOSITORY = Path(__file__).resolve().parents[2]
WORKLOADS = Path(__file__).resolve().parent / "workloads"
DEFAULT_SCRATCH = REPOSITORY / "target" / "guion-first-source-speed"
EXPECTED_PYTHON = "3.14.7"
DEFAULT_TRIALS = 21
BOOTSTRAP_RESAMPLES = 10_000
BOOTSTRAP_SEED = 20261025


@dataclass(frozen=True)
class Workload:
    name: str
    expected_stdout: str
    arguments: tuple[str, ...] = ()
    needs_file_root: bool = False


WORKLOADS_BY_NAME = {
    "tiny": Workload("tiny", "Hello from Guion\n"),
    "arithmetic": Workload("arithmetic", "500500\n"),
    "json_file": Workload("json_file", "3\n", ("records.json",), True),
    "imported_helper": Workload("imported_helper", "42\n"),
    "sqlite_toml": Workload("sqlite_toml", "Guion agents\n", needs_file_root=True),
}


def nearest_rank(samples: Sequence[float], percentile: float = 0.95) -> float:
    if not samples:
        raise ValueError("nearest-rank percentile needs at least one sample")
    if not 0 < percentile <= 1:
        raise ValueError("percentile must be in (0, 1]")
    index = max(0, math.ceil(percentile * len(samples)) - 1)
    return sorted(samples)[index]


def paired_ratios(guion_samples: Sequence[float], python_samples: Sequence[float]) -> list[float]:
    if len(guion_samples) != len(python_samples) or not guion_samples:
        raise ValueError("paired samples must be non-empty and have equal lengths")
    if any(sample <= 0 for sample in (*guion_samples, *python_samples)):
        raise ValueError("paired timing samples must be positive")
    return [guion / python for guion, python in zip(guion_samples, python_samples)]


def bootstrap_median_interval(
    ratios: Sequence[float], resamples: int = BOOTSTRAP_RESAMPLES, seed: int = BOOTSTRAP_SEED
) -> tuple[float, float]:
    if not ratios or resamples < 1:
        raise ValueError("bootstrap needs samples and at least one resample")
    rng = random.Random(seed)
    samples = [
        statistics.median(rng.choices(ratios, k=len(ratios))) for _ in range(resamples)
    ]
    return nearest_rank(samples, 0.025), nearest_rank(samples, 0.975)


def verify_output(return_code: int, stdout: str, expected_stdout: str) -> None:
    if return_code != 0:
        raise RuntimeError(f"command exited with {return_code}; expected success")
    if stdout != expected_stdout:
        raise RuntimeError(
            f"output mismatch: expected {expected_stdout!r}, got {stdout!r}"
        )


def randomized_pair_order(rng: random.Random) -> tuple[str, str]:
    lanes = ["guion", "python"]
    rng.shuffle(lanes)
    return lanes[0], lanes[1]


def parse_guion_argument(value: str) -> tuple[str, Path]:
    label, separator, path = value.partition("=")
    if not separator or not label or not path:
        raise argparse.ArgumentTypeError("Guion binary must use LABEL=PATH")
    resolved = Path(path).expanduser().resolve()
    if not resolved.is_file():
        raise argparse.ArgumentTypeError(f"Guion executable does not exist: {resolved}")
    return label, resolved


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def prepare_project(workload: Workload, scratch_root: Path) -> Path:
    source = WORKLOADS / workload.name
    if not source.is_dir():
        raise FileNotFoundError(f"workload directory is missing: {source}")
    project = Path(tempfile.mkdtemp(prefix=f"{workload.name}-", dir=scratch_root))
    shutil.copytree(source, project, dirs_exist_ok=True)
    return project


def command_for(
    lane: str, executable: Path, workload: Workload, project: Path
) -> list[str]:
    if lane == "guion":
        command = [str(executable), "run", "--no-cache"]
        if workload.needs_file_root:
            command.extend(["--file-root", str(project)])
        command.extend([str(project / "main.gu"), *workload.arguments])
        return command
    if lane == "python":
        return [str(executable), str(project / "main.py"), *workload.arguments]
    raise ValueError(f"unknown benchmark lane: {lane}")


def invoke(command: Sequence[str], cwd: Path) -> tuple[float, subprocess.CompletedProcess[str]]:
    started = time.perf_counter_ns()
    completed = subprocess.run(
        command,
        cwd=cwd,
        check=False,
        capture_output=True,
        text=True,
        encoding="utf-8",
    )
    elapsed = (time.perf_counter_ns() - started) / 1_000_000_000
    return elapsed, completed


def summarize_pair(guion_samples: list[float], python_samples: list[float]) -> dict[str, object]:
    ratios = paired_ratios(guion_samples, python_samples)
    lower, upper = bootstrap_median_interval(ratios)
    return {
        "guion_median_seconds": statistics.median(guion_samples),
        "guion_p95_seconds_nearest_rank": nearest_rank(guion_samples),
        "python_median_seconds": statistics.median(python_samples),
        "python_p95_seconds_nearest_rank": nearest_rank(python_samples),
        "paired_median_ratio_guion_over_python": statistics.median(ratios),
        "paired_ratio_bootstrap_95_percent_interval": [lower, upper],
        "speed_gate_passed": statistics.median(ratios) < 1.0 and upper < 1.0,
    }


def run_cohort(
    python_executable: Path,
    guion_binaries: list[tuple[str, Path]],
    trials: int,
    seed: int,
    scratch_root: Path,
) -> dict[str, object]:
    rng = random.Random(seed)
    workloads: dict[str, object] = {}
    for workload in WORKLOADS_BY_NAME.values():
        lanes: dict[str, object] = {}
        for label, binary in guion_binaries:
            samples: dict[str, list[dict[str, object]]] = {"guion": [], "python": []}
            for trial in range(trials):
                order = randomized_pair_order(rng)
                for lane in order:
                    project = prepare_project(workload, scratch_root)
                    try:
                        executable = binary if lane == "guion" else python_executable
                        command = command_for(lane, executable, workload, project)
                        elapsed, completed = invoke(command, project)
                        if completed.returncode != 0:
                            raise RuntimeError(
                                f"{workload.name}/{label}/{lane} trial {trial + 1} failed "
                                f"({completed.returncode}): stdout={completed.stdout!r}; "
                                f"stderr={completed.stderr!r}"
                            )
                        try:
                            verify_output(
                                completed.returncode,
                                completed.stdout,
                                workload.expected_stdout,
                            )
                        except RuntimeError as error:
                            raise RuntimeError(
                                f"{workload.name}/{label}/{lane} trial {trial + 1}: {error}; "
                                f"stderr={completed.stderr!r}"
                            ) from error
                        samples[lane].append(
                            {
                                "seconds": elapsed,
                                "command": command,
                                "trial": trial + 1,
                                "paired_order": list(order),
                                "correct": True,
                            }
                        )
                    finally:
                        shutil.rmtree(project, ignore_errors=True)
            guion_seconds = [sample["seconds"] for sample in samples["guion"]]
            python_seconds = [sample["seconds"] for sample in samples["python"]]
            lanes[label] = {
                "guion_samples": samples["guion"],
                "python_samples": samples["python"],
                "summary": summarize_pair(guion_seconds, python_seconds),
            }
        workloads[workload.name] = {
            "expected_stdout": workload.expected_stdout,
            "cache_policy": "--no-cache; fresh process and fresh copied project per sample",
            "os_file_cache": "uncontrolled",
            "lanes": lanes,
        }

    return {
        "schema_version": 1,
        "kind": "guion-first-source-speed-cohort",
        "created_at_utc": datetime.now(timezone.utc).isoformat(),
        "source_revision": subprocess.run(
            ["git", "rev-parse", "HEAD"],
            cwd=REPOSITORY,
            check=True,
            capture_output=True,
            text=True,
        ).stdout.strip(),
        "platform": {
            "system": platform.system(),
            "release": platform.release(),
            "machine": platform.machine(),
            "python_platform": platform.platform(),
        },
        "python": {
            "executable": str(python_executable),
            "version": subprocess.run(
                [str(python_executable), "--version"],
                check=True,
                capture_output=True,
                text=True,
            ).stdout.strip()
            or subprocess.run(
                [str(python_executable), "--version"],
                check=True,
                capture_output=True,
                text=True,
            ).stderr.strip(),
        },
        "trial_count": trials,
        "random_seed": seed,
        "bootstrap_resamples": BOOTSTRAP_RESAMPLES,
        "peak_memory_observation": {
            "status": "not_measured",
            "peak_memory_bytes": None,
            "reason": "The operating system denied the available system memory sampler under this run's permissions.",
        },
        "guion_binaries": {
            label: {"path": str(path), "sha256": sha256_file(path)}
            for label, path in guion_binaries
        },
        "workloads": workloads,
    }


def parse_args(argv: Sequence[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--python", required=True, type=Path, help="pinned Python 3.14.7 executable")
    parser.add_argument(
        "--guion",
        required=True,
        action="append",
        type=parse_guion_argument,
        metavar="LABEL=PATH",
        help="Guion executable; repeat to compare baseline and candidate",
    )
    parser.add_argument("--trials", type=int, default=DEFAULT_TRIALS)
    parser.add_argument("--seed", type=int, default=BOOTSTRAP_SEED)
    parser.add_argument("--output", required=True, type=Path, help="JSON report path inside this repository")
    parser.add_argument("--scratch", type=Path, default=DEFAULT_SCRATCH)
    args = parser.parse_args(argv)
    args.python = args.python.expanduser().resolve()
    if not args.python.is_file():
        parser.error(f"Python executable does not exist: {args.python}")
    if args.trials < 1:
        parser.error("--trials must be at least 1")
    labels = [label for label, _ in args.guion]
    if len(labels) != len(set(labels)):
        parser.error("Guion binary labels must be unique")
    output = args.output.expanduser()
    if not output.is_absolute():
        output = REPOSITORY / output
    args.output = output.resolve()
    if not args.output.is_relative_to(REPOSITORY):
        parser.error("report output must stay inside the Guion repository")
    args.scratch = args.scratch.expanduser().resolve()
    if not args.scratch.is_relative_to(REPOSITORY):
        parser.error("benchmark scratch must stay inside the Guion repository")
    return args


def main(argv: Sequence[str] | None = None) -> int:
    args = parse_args(argv)
    version_result = subprocess.run(
        [str(args.python), "--version"], check=True, capture_output=True, text=True
    )
    python_version = version_result.stdout.strip() or version_result.stderr.strip()
    if not re.search(rf"\b{re.escape(EXPECTED_PYTHON)}\b", python_version):
        raise SystemExit(f"expected Python {EXPECTED_PYTHON}, got {python_version}")
    args.scratch.mkdir(parents=True, exist_ok=True)
    scratch_root = args.scratch / "tmp"
    scratch_root.mkdir(parents=True, exist_ok=True)
    os.environ["TMPDIR"] = str(scratch_root)
    tempfile.tempdir = None
    report = run_cohort(args.python, args.guion, args.trials, args.seed, scratch_root)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    for workload_name, workload in report["workloads"].items():
        for label, lane in workload["lanes"].items():
            summary = lane["summary"]
            print(
                f"{workload_name} {label}: ratio="
                f"{summary['paired_median_ratio_guion_over_python']:.3f}, "
                f"95% CI={summary['paired_ratio_bootstrap_95_percent_interval']}, "
                f"gate={'PASS' if summary['speed_gate_passed'] else 'MISS'}"
            )
    print(f"report: {args.output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
