"""Compute-only 512x512 f64 matmul, Guion vs NumPy.

Each side runs the same program with 1 and with 21 multiplies (inputs built once, a
262,144-element sum per multiply on both sides). Start-up, compilation, the NumPy import and
input construction are common to the 1- and 21-multiply runs, so they cancel in
(21-run time - 1-run time) / 20, the cost of one multiply. Every trial runs all four
commands once in a shuffled order; the two sides' outputs must agree to a relative
tolerance of 1e-9 or the run stops. Usage: python3 matmul_compute.py [trials] [output.json]
Needs a release build (`cargo build --release -p guion-cli`) and NumPy.
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
PYTHON = sys.executable
TRIALS = int(sys.argv[1]) if len(sys.argv) > 1 else 15
OUT = sys.argv[2] if len(sys.argv) > 2 else os.path.join(HERE, "results_compute.json")
EXTRA = 20

COMMANDS = {
    "guion_1": [GUION, "run", "matmul_x1.gu"],
    "guion_21": [GUION, "run", "matmul_x21.gu"],
    "numpy_1": [PYTHON, "-I", "matmul_x1_np.py"],
    "numpy_21": [PYTHON, "-I", "matmul_x21_np.py"],
}


def run(name):
    start = time.perf_counter()
    done = subprocess.run(COMMANDS[name], cwd=PROGRAMS, capture_output=True, text=True)
    elapsed = time.perf_counter() - start
    if done.returncode != 0:
        raise SystemExit(f"{name} failed: {done.stderr}")
    return elapsed, float(done.stdout.split()[-1])


def agree(a, b):
    return abs(a - b) <= 1e-9 * max(1.0, abs(b))


def main():
    times = {name: [] for name in COMMANDS}
    outputs = {}
    for _ in range(TRIALS):
        order = list(COMMANDS)
        random.shuffle(order)
        for name in order:
            elapsed, value = run(name)
            times[name].append(elapsed)
            outputs.setdefault(name, value)
    for size in ("1", "21"):
        if not agree(outputs["guion_" + size], outputs["numpy_" + size]):
            raise SystemExit(f"outputs differ at {size}: {outputs}")
    per = {}
    for side in ("guion", "numpy"):
        per[side] = [
            (b - a) / EXTRA * 1000
            for a, b in zip(times[side + "_1"], times[side + "_21"])
        ]
    median = {side: statistics.median(values) for side, values in per.items()}
    # Ratio of the medians, and the spread of per-trial ratios (trial i vs trial i).
    pairs = sorted(g / n for g, n in zip(per["guion"], per["numpy"]))
    ratio = median["guion"] / median["numpy"]
    result = {
        "schema": "guion-matmul-compute/v1",
        "trials": TRIALS,
        "machine": {"platform": platform.platform(), "python": platform.python_version()},
        "guion": subprocess.run([GUION, "--version"], capture_output=True, text=True).stdout.strip(),
        "numpy": subprocess.run(
            [PYTHON, "-I", "-c", "import numpy; print(numpy.__version__)"],
            capture_output=True, text=True,
        ).stdout.strip(),
        "outputs": outputs,
        "median_seconds": {k: statistics.median(v) for k, v in times.items()},
        "per_multiply_ms": median,
        "per_multiply_ms_trials": per,
        "guion_over_numpy": {"ratio_of_medians": ratio, "per_trial_min_max": [pairs[0], pairs[-1]]},
    }
    with open(OUT, "w") as handle:
        json.dump(result, handle, indent=2)
        handle.write("\n")
    print(f"guion {median['guion']:.3f} ms per multiply, numpy {median['numpy']:.3f} ms")
    print(f"ratio {ratio:.2f}x (per-trial {pairs[0]:.2f}-{pairs[-1]:.2f})")


if __name__ == "__main__":
    main()
