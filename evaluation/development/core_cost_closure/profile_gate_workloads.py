#!/usr/bin/env python3
"""Hardware counters and hot functions for the speed-gate workloads, on Linux with `perf`.

For each workload at each size, `perf stat` measures the Python lane and Guion's artifact lane
(cycles, instructions, cache and branch misses), so the per-row cost can be compared between
sizes and hosts. At the largest size `perf record` samples both lanes and saves the hottest
functions. Uses the fixtures from run_speed_gate.py.

usage: profile_gate_workloads.py --guion GUION --python PYTHON --output DIRECTORY
                                 [--sizes 20000,200000] [--repeat 5]
"""

from __future__ import annotations

import argparse
import subprocess
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import run_speed_gate as gate  # noqa: E402

GUION_LANE = {"text": "artifact", "jsonl": "fused-artifact", "csv": "artifact"}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--guion", required=True)
    parser.add_argument("--python", required=True)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--sizes", default="20000,200000")
    parser.add_argument("--repeat", type=int, default=5)
    options = parser.parse_args()
    guion = str(Path(options.guion).resolve())
    python = str(Path(options.python).absolute())
    sizes = [int(size) for size in options.sizes.split(",")]
    output = options.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="gate-profile-") as scratch_name:
        scratch = Path(scratch_name)
        for workload, prepare in gate.WORKLOADS.items():
            for rows in sizes:
                project, lanes, _ = prepare(guion, python, scratch, rows)
                for lane in ("python", GUION_LANE[workload]):
                    name = f"{workload}-{rows}-{'python' if lane == 'python' else 'guion'}"
                    command = lanes[lane]
                    subprocess.run(command, cwd=project, check=True, capture_output=True)
                    stat = subprocess.run(
                        ["perf", "stat", "-d", "-r", str(options.repeat), "--", *command],
                        cwd=project, capture_output=True, text=True, check=True)
                    (output / f"{name}.stat.txt").write_text(stat.stderr, encoding="utf-8")
                    print(f"== {name}\n{stat.stderr}", flush=True)
                    if rows != max(sizes):
                        continue
                    data = output / f"{name}.perf.data"
                    subprocess.run(["perf", "record", "-F", "4999", "-o", str(data), "--", *command],
                                   cwd=project, capture_output=True, check=True)
                    report = subprocess.run(
                        ["perf", "report", "-i", str(data), "--stdio", "--sort", "dso,symbol",
                         "--percent-limit", "0.4"],
                        capture_output=True, text=True, check=True).stdout
                    (output / f"{name}.hot.txt").write_text(report, encoding="utf-8")
                    data.unlink()
    return 0


if __name__ == "__main__":
    sys.exit(main())
