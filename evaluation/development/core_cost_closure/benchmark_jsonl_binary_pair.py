#!/usr/bin/env python3
"""Compare two release binaries on the frozen JSONL workload and Python oracle."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "evaluation/development/core_north_star_sprint"))
import benchmark_core_workloads as benchmark  # noqa: E402


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def sha256_tree(root: Path) -> str:
    digest = hashlib.sha256()
    for path in sorted(root.rglob("*.gu")):
        digest.update(path.relative_to(root).as_posix().encode("utf-8"))
        digest.update(b"\0")
        digest.update(path.read_bytes())
        digest.update(b"\0")
    return digest.hexdigest()


def jsonl_input_hashes(scratch: Path, rows: list[int]) -> dict[str, Any]:
    stdlib_root = Path(os.environ.get("GUION_STDLIB_ROOT", ROOT / "language/stdlib"))
    workloads = {}
    for row_count in rows:
        project = scratch / f"jsonl-summary-data-{row_count}"
        workloads[str(row_count)] = {
            "application_source": sha256(project / "main.gu"),
            "events": sha256(project / "performance-inputs/events.jsonl"),
            "services": sha256(project / "performance-inputs/services.txt"),
            "python_reference": sha256(project / "data_reference.py"),
        }
    return {
        "benchmark_script": sha256(Path(__file__)),
        "performance_runner": sha256(Path(benchmark.runner.__file__)),
        "jsonl_generator": sha256(benchmark.runner.GENERATOR),
        "source_application": sha256(benchmark.runner.APP / "main.vr"),
        "python_reference": sha256(benchmark.runner.DATA_REFERENCE),
        "stdlib_tree": sha256_tree(stdlib_root),
        "workloads": workloads,
    }


def paired_ratio(samples: dict[str, dict[str, Any]], numerator: str, denominator: str) -> dict[str, Any]:
    ratios = [
        left / right
        for left, right in zip(
            samples[numerator]["samples_seconds"],
            samples[denominator]["samples_seconds"],
            strict=True,
        )
    ]
    return benchmark.runner.paired_median_ci(ratios)


def measure(
    baseline: Path, candidate: Path, python: Path, scratch: Path, rows: int, trials: int
) -> dict[str, Any]:
    result = benchmark.measure_jsonl_comparison(
        baseline, candidate, python, scratch / f"jsonl-summary-data-{rows}", rows, trials
    )
    result["binary_pair_comparisons"] = {
        "candidate_source_over_baseline_source": paired_ratio(
            result["samples"], "guion_candidate_source_uncached", "guion_before_source_uncached"
        ),
        "candidate_artifact_over_baseline_artifact": paired_ratio(
            result["samples"], "guion_candidate_artifact", "guion_before_artifact"
        ),
    }
    return result


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline-binary", required=True, type=Path)
    parser.add_argument("--candidate-binary", required=True, type=Path)
    parser.add_argument("--python", type=Path, default=Path(sys.executable))
    parser.add_argument("--scratch-dir", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--rows", type=int, nargs="+", default=[20_000, 200_000])
    parser.add_argument("--trials", type=int, default=15)
    args = parser.parse_args()
    if args.trials < 2:
        parser.error("--trials must be at least 2")
    baseline = args.baseline_binary.resolve(strict=True)
    candidate = args.candidate_binary.resolve(strict=True)
    python = args.python.resolve(strict=True)
    args.scratch_dir.mkdir(parents=True, exist_ok=True)
    toolchain = {
        "rustc": subprocess.run(["rustc", "--version"], capture_output=True, text=True, check=True).stdout.strip(),
        "cargo": subprocess.run(["cargo", "--version"], capture_output=True, text=True, check=True).stdout.strip(),
        "python": subprocess.run([str(python), "--version"], capture_output=True, text=True, check=True).stdout.strip(),
    }
    workloads = {
        str(rows): measure(baseline, candidate, python, args.scratch_dir, rows, args.trials)
        for rows in args.rows
    }
    report = {
        "schema_version": 1,
        "kind": "guion-jsonl-release-binary-pair",
        "created_at_utc": datetime.now(timezone.utc).isoformat(),
        "status": "passed",
        "correctness_status": "passed",
        "performance_status": {
            rows: {
                "source_candidate_win": workload["binary_pair_comparisons"]["candidate_source_over_baseline_source"]["win"],
                "artifact_candidate_win": workload["binary_pair_comparisons"]["candidate_artifact_over_baseline_artifact"]["win"],
            }
            for rows, workload in workloads.items()
        },
        "scope": "same frozen source and generated inputs; all timed Guion and Python outputs checked against the independent oracle; separate source and artifact lanes",
        "baseline_binary": {"path": str(baseline), "sha256": sha256(baseline)},
        "candidate_binary": {"path": str(candidate), "sha256": sha256(candidate)},
        "input_sha256": jsonl_input_hashes(args.scratch_dir, args.rows),
        "toolchain": {"platform": platform.platform(), "architecture": platform.machine(), **toolchain},
        "workloads": workloads,
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps({"status": "passed", "output": str(args.output), "rows": list(workloads)}))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
