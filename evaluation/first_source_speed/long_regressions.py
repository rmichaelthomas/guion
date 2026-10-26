#!/usr/bin/env python3
"""Run preserved Richards, N-body, and Mandelbrot source regressions."""

from __future__ import annotations

import argparse
import json
import math
import os
import platform
import random
import shutil
import statistics
import subprocess
import tempfile
import time
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Sequence

if __package__:
    from .run import EXPECTED_PYTHON, REPOSITORY, invoke, sha256_file, summarize_pair, verify_output
else:
    from run import EXPECTED_PYTHON, REPOSITORY, invoke, sha256_file, summarize_pair, verify_output


FAMOUS = REPOSITORY / "evaluation" / "development" / "famous_benchmarks"
DEFAULT_SCRATCH = REPOSITORY / "target" / "guion-first-source-speed"


@dataclass(frozen=True)
class LongWorkload:
    name: str
    source_path: Path
    python_path: Path
    expected_stdout: str


def long_workloads() -> tuple[LongWorkload, ...]:
    return (
        LongWorkload(
            "richards",
            FAMOUS / "richards.vr",
            FAMOUS / "richards_reference.py",
            "9297\n23246\n",
        ),
        LongWorkload(
            "nbody",
            FAMOUS / "nbody.vr",
            FAMOUS / "nbody.py",
            "-0.16907516382852447\n-0.16908926275527233\n",
        ),
        LongWorkload(
            "mandelbrot",
            FAMOUS / "mandelbrot.vr",
            FAMOUS / "mandelbrot.py",
            "155052\n1608\n",
        ),
    )


def verify_long_output(workload: LongWorkload, return_code: int, stdout: str) -> None:
    if return_code != 0:
        raise RuntimeError(f"command exited with {return_code}; expected success")
    if workload.name != "nbody":
        verify_output(return_code, stdout, workload.expected_stdout)
        return
    actual_lines = stdout.splitlines()
    expected_lines = workload.expected_stdout.splitlines()
    if len(actual_lines) != len(expected_lines):
        raise RuntimeError(
            f"output mismatch: expected {workload.expected_stdout!r}, got {stdout!r}"
        )
    try:
        matches = all(
            math.isclose(float(actual), float(expected), rel_tol=1e-12, abs_tol=1e-12)
            for actual, expected in zip(actual_lines, expected_lines)
        )
    except ValueError:
        matches = False
    if not matches:
        raise RuntimeError(
            f"output mismatch: expected {workload.expected_stdout!r} within 1e-12, got {stdout!r}"
        )


def parse_args(argv: Sequence[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--guion", required=True, type=Path)
    parser.add_argument("--python", required=True, type=Path)
    parser.add_argument("--trials", type=int, default=3)
    parser.add_argument("--seed", type=int, default=20261026)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--scratch", type=Path, default=DEFAULT_SCRATCH)
    args = parser.parse_args(argv)
    args.guion = args.guion.expanduser().resolve()
    args.python = args.python.expanduser().resolve()
    if not args.guion.is_file():
        parser.error(f"Guion executable does not exist: {args.guion}")
    if not args.python.is_file():
        parser.error(f"Python executable does not exist: {args.python}")
    if args.trials < 1:
        parser.error("--trials must be at least 1")
    args.output = args.output.expanduser()
    if not args.output.is_absolute():
        args.output = REPOSITORY / args.output
    args.output = args.output.resolve()
    args.scratch = args.scratch.expanduser().resolve()
    if not args.output.is_relative_to(REPOSITORY):
        parser.error("report output must stay inside the Guion repository")
    if not args.scratch.is_relative_to(REPOSITORY):
        parser.error("benchmark scratch must stay inside the Guion repository")
    return args


def prepare_project(workload: LongWorkload, scratch_root: Path) -> tuple[Path, Path, Path]:
    project = Path(tempfile.mkdtemp(prefix=f"{workload.name}-long-", dir=scratch_root))
    source = project / "main.vr"
    artifact = project / "main.gub"
    shutil.copy2(workload.source_path, source)
    instruction_limit = 300_000_000 if workload.name == "nbody" else 100_000_000
    manifest = (
        "[project]\n"
        f'name = "{workload.name}-first-source-speed"\n'
        'version = "0.1.0"\n'
        'entry = "main.vr"\n\n'
        "[runtime]\n"
        f"instruction_limit = {instruction_limit}\n"
    )
    (project / "guion.toml").write_text(manifest, encoding="utf-8")
    return project, source, artifact


def run_report(
    guion: Path,
    python: Path,
    trials: int,
    seed: int,
    scratch_root: Path,
) -> dict[str, object]:
    rng = random.Random(seed)
    reports: dict[str, object] = {}
    python_env = dict(os.environ)
    python_env["PYTHONDONTWRITEBYTECODE"] = "1"

    for workload in long_workloads():
        project, source, artifact = prepare_project(workload, scratch_root)
        try:
            build_command = [str(guion), "build", str(source), "--output", str(artifact)]
            build_seconds, built = invoke(build_command, project)
            if built.returncode != 0 or not artifact.is_file():
                raise RuntimeError(
                    f"{workload.name} artifact build failed: "
                    f"stdout={built.stdout!r}; stderr={built.stderr!r}"
                )
            commands = {
                "source": [str(guion), "run", "--no-cache", str(source)],
                "artifact": [str(guion), "run", str(artifact)],
                "python": [str(python), str(workload.python_path)],
            }
            working_directories = {
                "source": project,
                "artifact": project,
                "python": FAMOUS,
            }
            samples: dict[str, list[dict[str, object]]] = {
                "source": [],
                "artifact": [],
                "python": [],
            }
            for trial in range(trials):
                order = ["source", "artifact", "python"]
                rng.shuffle(order)
                for lane in order:
                    command = commands[lane]
                    started = time.perf_counter_ns()
                    completed = subprocess.run(
                        command,
                        cwd=working_directories[lane],
                        check=False,
                        capture_output=True,
                        text=True,
                        encoding="utf-8",
                        env=python_env,
                    )
                    elapsed = (time.perf_counter_ns() - started) / 1_000_000_000
                    if completed.returncode != 0:
                        raise RuntimeError(
                            f"{workload.name}/{lane} trial {trial + 1} failed "
                            f"({completed.returncode}): stdout={completed.stdout!r}; "
                            f"stderr={completed.stderr!r}"
                        )
                    verify_long_output(workload, completed.returncode, completed.stdout)
                    samples[lane].append(
                        {
                            "seconds": elapsed,
                            "command": command,
                            "trial": trial + 1,
                            "paired_order": order,
                            "correct": True,
                        }
                    )
            source_seconds = [sample["seconds"] for sample in samples["source"]]
            artifact_seconds = [sample["seconds"] for sample in samples["artifact"]]
            python_seconds = [sample["seconds"] for sample in samples["python"]]
            reports[workload.name] = {
                "expected_stdout": workload.expected_stdout,
                "source_sha256": sha256_file(workload.source_path),
                "python_reference_sha256": sha256_file(workload.python_path),
                "artifact_build_seconds_outside_timed_lane": build_seconds,
                "artifact_bytes": artifact.stat().st_size,
                "samples": samples,
                "source_vs_python": summarize_pair(source_seconds, python_seconds),
                "artifact_vs_python": summarize_pair(artifact_seconds, python_seconds),
                "os_file_cache": "uncontrolled",
            }
        finally:
            shutil.rmtree(project, ignore_errors=True)

    return {
        "schema_version": 1,
        "kind": "guion-long-source-regressions",
        "created_at_utc": datetime.now(timezone.utc).isoformat(),
        "source_revision": subprocess.run(
            ["git", "rev-parse", "HEAD"],
            cwd=REPOSITORY,
            check=True,
            capture_output=True,
            text=True,
        ).stdout.strip(),
        "platform": platform.platform(),
        "python": {
            "executable": str(python),
            "version": subprocess.run(
                [str(python), "--version"],
                check=True,
                capture_output=True,
                text=True,
                env=python_env,
            ).stdout.strip(),
        },
        "guion_binary": {"path": str(guion), "sha256": sha256_file(guion)},
        "trial_count_per_lane": trials,
        "random_seed": seed,
        "workloads": reports,
    }


def main(argv: Sequence[str] | None = None) -> int:
    args = parse_args(argv)
    version = subprocess.run(
        [str(args.python), "--version"], check=True, capture_output=True, text=True
    ).stdout.strip()
    if EXPECTED_PYTHON not in version:
        raise SystemExit(f"expected Python {EXPECTED_PYTHON}, got {version}")
    args.scratch.mkdir(parents=True, exist_ok=True)
    scratch_root = args.scratch / "tmp"
    scratch_root.mkdir(parents=True, exist_ok=True)
    os.environ["TMPDIR"] = str(scratch_root)
    report = run_report(args.guion, args.python, args.trials, args.seed, scratch_root)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    for name, result in report["workloads"].items():
        source_ratio = result["source_vs_python"]["paired_median_ratio_guion_over_python"]
        artifact_ratio = result["artifact_vs_python"]["paired_median_ratio_guion_over_python"]
        print(f"{name}: source/Python={source_ratio:.3f}; artifact/Python={artifact_ratio:.3f}")
    print(f"report: {args.output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
