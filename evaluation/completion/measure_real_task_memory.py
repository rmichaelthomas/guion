#!/usr/bin/env python3
"""Measure each matching case's child high-water RSS, separately from timings."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import resource
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT))
from evaluation.completion.compare_real_tasks import APP_IDS, APPS, MANIFEST, PYTHON_REFERENCE  # noqa: E402
from evaluation.protocol.case_runner import run_case  # noqa: E402


def worker(path: Path, command: list[str]) -> None:
    result = subprocess.run(command, capture_output=True)
    sys.stdout.buffer.write(result.stdout)
    sys.stderr.buffer.write(result.stderr)
    peak = resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss
    if sys.platform != "darwin":
        peak *= 1024
    with path.open("a", encoding="utf-8") as output:
        output.write(json.dumps({"peak_rss_bytes": peak, "exit_code": result.returncode}) + "\n")
    raise SystemExit(result.returncode)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--worker", type=Path)
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    if args.worker:
        worker(args.worker, args.command[1:] if args.command[:1] == ["--"] else args.command)
        return
    if not args.binary or not args.output:
        parser.error("--binary and --output are required")
    binary = args.binary.resolve(strict=True)
    scratch = ROOT / "target/perf-session/real-tasks-tmp"
    scratch.mkdir(parents=True, exist_ok=True)
    os.environ["TMPDIR"] = str(scratch)
    tempfile.tempdir = str(scratch)
    manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))
    tasks = {item["id"]: item for item in manifest["tasks"]}
    result = {"schema_version": 1, "platform": platform.platform(),
              "guion_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
              "method": "resource.getrusage(RUSAGE_CHILDREN) from one wrapper per process; "
                        "max step RSS, not a simultaneous process-tree peak; separate from timings",
              "applications": {}}
    for name, task_id in APP_IDS.items():
        case = tasks[task_id]["initial"]["cases"][0]
        observations = {}
        for label, command, root, extra in (
            ("guion", [str(binary), "run", "--no-cache", "${candidate}"], APPS / name,
             {"workspace_root_project": True, "env": {"GUION_FILE_ROOT": "${workspace}"}}),
            ("python", [sys.executable, "${candidate}/main.py"], PYTHON_REFERENCE, {}),
        ):
            metrics = scratch / f"{name}-{label}-rss.jsonl"
            metrics.unlink(missing_ok=True)
            config = {"command": [sys.executable, str(Path(__file__).resolve()), "--worker", str(metrics), "--", *command], **extra}
            outcome = run_case(case, config, root, timeout_seconds=90)
            if not outcome["passed"]:
                raise RuntimeError(f"{name} {label} failed acceptance: {outcome['failures']}")
            samples = [json.loads(line) for line in metrics.read_text().splitlines()]
            observations[label] = {"max_step_peak_rss_bytes": max(x["peak_rss_bytes"] for x in samples),
                                   "step_samples": samples}
            metrics.unlink()
        result["applications"][name] = {"case_id": case["id"], **observations}
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
        print(json.dumps({"application": name, "rss": {k: v["max_step_peak_rss_bytes"] for k, v in observations.items()}}), flush=True)


if __name__ == "__main__":
    main()
