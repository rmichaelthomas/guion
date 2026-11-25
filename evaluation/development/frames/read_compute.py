"""Compute-only CSV read of 1,000,000 rows, Guion `frames` vs pandas.

Each side runs the same program with 1 and with 6 reads of data/rows.csv in one process
(each read followed by the row count and the sum of column x). Start-up, compilation, the
pandas import and the first-read warm-up are common to both runs, so they cancel in
(6-read time - 1-read time) / 5, the cost of one read. Every trial runs all four commands
once in a shuffled order; the two sides' row count and column_sum(x) must agree to a
relative tolerance of 1e-9 or the run stops.
Usage: python3 read_compute.py [trials] [output.json]
Needs a release build (`cargo build --release -p guion-cli`), the `.venv` from
requirements.txt, programs/data/rows.csv (python3 generate.py), and
programs_compute/data/rows.csv (a hard link to it; the script creates it and runs `guion update`
in programs_compute itself before timing).
"""

import json
import os
import platform
import random
import statistics
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
PROGRAMS = os.path.join(HERE, "programs_compute")
ROOT = os.path.abspath(os.path.join(HERE, "..", "..", ".."))
GUION = os.path.join(ROOT, "target", "release", "guion")
PANDAS_PYTHON = os.path.join(HERE, ".venv", "bin", "python")
TRIALS = int(sys.argv[1]) if len(sys.argv) > 1 else 31
OUT = sys.argv[2] if len(sys.argv) > 2 else os.path.join(HERE, "results_read_compute.json")
EXTRA = 5

COMMANDS = {
    "guion_1": [GUION, "run", "read_x1.gu"],
    "guion_6": [GUION, "run", "read_x6.gu"],
    "pandas_1": [PANDAS_PYTHON, "-I", "read_x1_pd.py"],
    "pandas_6": [PANDAS_PYTHON, "-I", "read_x6_pd.py"],
}


def setup():
    data = os.path.join(PROGRAMS, "data", "rows.csv")
    source = os.path.join(HERE, "programs", "data", "rows.csv")
    if not os.path.exists(source):
        raise SystemExit("run `python3 generate.py` first")
    if not os.path.exists(data):
        # A hard link: the package's file capability refuses a symbolic link.
        os.makedirs(os.path.dirname(data), exist_ok=True)
        os.link(source, data)
    subprocess.run([GUION, "update"], cwd=PROGRAMS, check=True, capture_output=True)


def run(name):
    start = time.perf_counter()
    done = subprocess.run(COMMANDS[name], cwd=PROGRAMS, capture_output=True, text=True)
    elapsed = time.perf_counter() - start
    if done.returncode != 0:
        raise SystemExit(f"{name} failed: {done.stderr}")
    return elapsed, [float(v) for v in done.stdout.split()]


def agree(a, b):
    return len(a) == len(b) and all(abs(x - y) <= 1e-9 * max(1.0, abs(y)) for x, y in zip(a, b))


def main():
    setup()
    times = {name: [] for name in COMMANDS}
    outputs = {}
    for trial in range(TRIALS):
        order = list(COMMANDS)
        random.shuffle(order)
        current = {}
        for name in order:
            elapsed, values = run(name)
            times[name].append(elapsed)
            current[name] = values
            outputs.setdefault(name, values)
        for size in ("1", "6"):
            if not agree(current["guion_" + size], current["pandas_" + size]):
                raise SystemExit(f"outputs differ at {size}: {current}")
        print(f"trial {trial + 1}/{TRIALS} done", file=sys.stderr)
    per = {}
    for side in ("guion", "pandas"):
        per[side] = [
            (b - a) / EXTRA * 1000
            for a, b in zip(times[side + "_1"], times[side + "_6"])
        ]
    median = {side: statistics.median(values) for side, values in per.items()}
    pairs = sorted(g / n for g, n in zip(per["guion"], per["pandas"]))
    ratio = median["guion"] / median["pandas"]
    result = {
        "schema": "guion-read-compute/v1",
        "trials": TRIALS,
        "machine": {
            "platform": platform.platform(),
            "python": subprocess.run(
                [PANDAS_PYTHON, "-I", "-c", "import sys; print(sys.version)"],
                capture_output=True, text=True,
            ).stdout.strip(),
        },
        "guion": subprocess.run([GUION, "--version"], capture_output=True, text=True).stdout.strip(),
        "pandas": subprocess.run(
            [PANDAS_PYTHON, "-I", "-c", "import pandas, numpy; print(pandas.__version__, numpy.__version__)"],
            capture_output=True, text=True,
        ).stdout.strip(),
        "outputs": outputs,
        "median_seconds": {k: statistics.median(v) for k, v in times.items()},
        "per_read_ms": median,
        "per_read_ms_trials": per,
        "ratio": ratio,
        "guion_over_pandas": {"ratio_of_medians": ratio, "per_trial_min_max": [pairs[0], pairs[-1]]},
        "guion_ms": median["guion"],
        "pandas_ms": median["pandas"],
    }
    with open(OUT, "w") as handle:
        json.dump(result, handle, indent=2)
        handle.write("\n")
    print(f"guion {median['guion']:.1f} ms per read, pandas {median['pandas']:.1f} ms")
    print(f"ratio {ratio:.2f}x (per-trial {pairs[0]:.2f}-{pairs[-1]:.2f})")


if __name__ == "__main__":
    main()
