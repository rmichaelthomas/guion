#!/usr/bin/env python3
"""Compare two builds on 25 output-checked Richards executions per process."""

from __future__ import annotations

import argparse
import hashlib
import json
import shutil
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "evaluation/completion/performance"))
import runner  # noqa: E402


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline", required=True, type=Path)
    parser.add_argument("--candidate", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    baseline = args.baseline.resolve(strict=True)
    candidate = args.candidate.resolve(strict=True)
    scratch = ROOT / "target/perf-session"
    scratch.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="richards-repeat-", dir=scratch) as tmp:
        project = Path(tmp)
        source = project / "richards.gu"
        warm = project / "warm.gu"
        shutil.copy2(runner.WORKLOADS / "richards.vr", source)
        runner.make_warm_source(source, warm, 25)
        runner.write_manifest(project, "richards-repeated", 30_000_000 * 25)
        commands = {"guion_baseline_warm": [str(baseline), "run", "--json", "--no-cache", str(warm)],
                    "guion_candidate_warm": [str(candidate), "run", "--json", "--no-cache", str(warm)]}
        samples, metadata = runner.sample_candidates("richards-scheduler", commands, project,
                                                     ["9297", "23246"], repetitions=25, trials=15)
    ratios = [after / before for after, before in zip(samples["guion_candidate_warm"]["samples_seconds"],
                                                      samples["guion_baseline_warm"]["samples_seconds"], strict=True)]
    report = {"schema_version": 1, "repetitions_per_process": 25, "trials_per_arm": 15,
              "baseline_sha256": hashlib.sha256(baseline.read_bytes()).hexdigest(),
              "candidate_sha256": hashlib.sha256(candidate.read_bytes()).hexdigest(),
              "samples_per_execution": samples, "candidate_over_baseline": runner.paired_median_ci(ratios),
              "metadata": metadata, "output_checked": True, "os_cache": "uncontrolled"}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(report["candidate_over_baseline"]))


if __name__ == "__main__":
    main()
