#!/usr/bin/env python3
"""Paired Guion/Python timing for the text-processing workload, in the same shape as the JSONL
and CSV gates: shuffled order per trial, fresh processes, output checked against Python, and a
one-sided 95% upper bound on the per-trial Guion/Python ratio for the source and artifact lanes.

usage: benchmark_text_paired_python.py GUION PYTHON TRIALS SIZES OUTPUT_JSON
"""

import json
import math
import random
import statistics
import subprocess
import sys
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
WORKLOAD = ROOT / "evaluation/development/famous_benchmarks/text_processing"
# Two-sided 97.5% t quantiles by degrees of freedom, for the 95% upper bound of the mean ratio.
T_975 = {4: 2.776, 5: 2.571, 6: 2.447, 7: 2.365, 8: 2.306, 9: 2.262, 10: 2.228, 11: 2.201,
         12: 2.179, 13: 2.160, 14: 2.145, 19: 2.093, 24: 2.064, 29: 2.045}

guion, python = sys.argv[1], sys.argv[2]
trials = int(sys.argv[3])
sizes = [int(size) for size in sys.argv[4].split(",")]
output = Path(sys.argv[5])
if trials - 1 not in T_975:
    raise SystemExit(f"no t quantile for {trials} trials; use one of {sorted(d + 1 for d in T_975)}")


def sized(suffix: str, lines: int) -> str:
    source = (WORKLOAD.parent / (WORKLOAD.name + suffix)).read_text(encoding="utf-8")
    if source.count("40000") != 1:
        raise SystemExit(f"expected one line count in {WORKLOAD.name}{suffix}")
    return source.replace("40000", str(lines))


def run(command: list[str]) -> str:
    return subprocess.run(command, check=True, capture_output=True, text=True).stdout


results = {}
rng = random.Random(7)
with tempfile.TemporaryDirectory(prefix="text-paired-") as scratch:
    for lines in sizes:
        project = Path(scratch) / f"text-{lines}"
        project.mkdir()
        # The artifact sits inside the project so both lanes run under the same runtime limits.
        (project / "guion.toml").write_text(
            '[project]\nname = "text-paired"\nversion = "1.0.0"\nentry = "main.gu"\n'
            '[runtime]\ninstruction_limit = 2000000000\nallocation_limit_bytes = 8589934592\n',
            encoding="utf-8")
        (project / "main.gu").write_text(sized(".vr", lines), encoding="utf-8")
        (project / "reference.py").write_text(sized(".py", lines), encoding="utf-8")
        subprocess.run([guion, "build", str(project), "-o", str(project / "main.gub")],
                       check=True, capture_output=True)
        commands = {
            "python": [python, str(project / "reference.py")],
            "guion-src": [guion, "run", str(project / "main.gu")],
            "guion-art": [guion, "run", str(project / "main.gub")],
        }
        expected = run(commands["python"])
        for kind, command in commands.items():
            if run(command) != expected:  # also warms the compilation cache for the source lane
                raise SystemExit(f"{kind} output differs from Python at {lines} lines")
        samples = {kind: [] for kind in commands}
        for _ in range(trials):
            order = list(commands)
            rng.shuffle(order)
            for kind in order:
                started = time.perf_counter()
                subprocess.run(commands[kind], check=True, capture_output=True)
                samples[kind].append(time.perf_counter() - started)
        results[lines] = samples

summary = {}
for lines, samples in results.items():
    python_times = samples["python"]
    print(f"lines={lines} python median {statistics.median(python_times):.3f}s")
    summary[lines] = {}
    for kind in ("guion-src", "guion-art"):
        ratios = [g / p for g, p in zip(samples[kind], python_times)]
        upper = statistics.mean(ratios) + T_975[trials - 1] * statistics.stdev(ratios) / math.sqrt(trials)
        summary[lines][kind] = {"median_ratio": statistics.median(ratios), "upper95": upper}
        print(f"  {kind:9s} median {statistics.median(samples[kind]):.3f}s "
              f"ratio median {statistics.median(ratios):.3f} upper95 {upper:.3f}")
output.write_text(json.dumps({"samples": results, "summary": summary}, indent=1), encoding="utf-8")
