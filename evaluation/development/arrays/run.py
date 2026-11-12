"""Paired, interleaved, output-checked array benchmarks: Guion vs NumPy vs plain CPython.

Usage: python3 run.py [trials] [output.json] [programs-directory]

The default directory, `programs`, has Guion, NumPy and plain-CPython lanes at sizes the
plain-CPython lane can finish. `programs_large` has the plan's sizes (10M-element vectors, a
512x512 matmul) with Guion and NumPy lanes only, so that computation rather than process
start-up and the NumPy import dominates.

Every trial runs each lane of each workload once, in a shuffled order, and records
end-to-end wall time (process start to exit, including compilation for Guion and the
NumPy import for NumPy). A lane's output must match the NumPy lane's output to a relative
tolerance of 1e-9, or the run stops.
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
PROGRAMS = os.path.join(HERE, sys.argv[3] if len(sys.argv) > 3 else "programs")
LARGE = os.path.basename(PROGRAMS) == "programs_large"
ROOT = os.path.abspath(os.path.join(HERE, "..", "..", ".."))
GUION = os.path.join(ROOT, "target", "release", "guion")
PYTHON = sys.executable
WORKLOADS = ["null", "elementwise", "sum", "matmul", "sort", "loop"]


def lanes(workload):
    result = {
        "guion": [GUION, "run", f"{workload}.gu"],
        "numpy": [PYTHON, "-I", f"{workload}_np.py"],
    }
    if not LARGE:
        result["python"] = [PYTHON, "-I", f"{workload}_py.py"]
    return result


def run(command):
    start = time.perf_counter()
    done = subprocess.run(command, cwd=PROGRAMS, capture_output=True, text=True)
    elapsed = time.perf_counter() - start
    if done.returncode != 0:
        raise SystemExit(f"{command} failed: {done.stderr}")
    return elapsed, done.stdout.split()


def same(reference, values):
    if len(reference) != len(values):
        return False
    for expected, actual in zip(reference, values):
        expected, actual = float(expected), float(actual)
        if abs(expected - actual) > 1e-9 * max(1.0, abs(expected)):
            return False
    return True


def bootstrap_median_ci(values, rounds=10000, seed=7):
    rng = random.Random(seed)
    medians = sorted(
        statistics.median(rng.choice(values) for _ in values) for _ in range(rounds)
    )
    return medians[int(0.025 * rounds)], medians[int(0.975 * rounds)]


def main():
    trials = int(sys.argv[1]) if len(sys.argv) > 1 else 15
    output = sys.argv[2] if len(sys.argv) > 2 else os.path.join(HERE, "results.json")
    samples = {workload: {lane: [] for lane in lanes(workload)} for workload in WORKLOADS}
    order_rng = random.Random(2026)
    for trial in range(trials):
        jobs = [(workload, lane) for workload in WORKLOADS for lane in lanes(workload)]
        order_rng.shuffle(jobs)
        outputs = {}
        for workload, lane in jobs:
            elapsed, values = run(lanes(workload)[lane])
            samples[workload][lane].append(elapsed)
            outputs[(workload, lane)] = values
        for workload in WORKLOADS:
            reference = outputs[(workload, "numpy")]
            for lane in lanes(workload):
                if lane != "numpy" and not same(reference, outputs[(workload, lane)]):
                    raise SystemExit(
                        f"output mismatch in {workload}/{lane}: "
                        f"{outputs[(workload, lane)]} vs numpy {reference}"
                    )
        print(f"trial {trial + 1}/{trials} done", file=sys.stderr)

    summary = {}
    for workload in WORKLOADS:
        lane_samples = samples[workload]
        entry = {lane: statistics.median(values) for lane, values in lane_samples.items()}
        for other in [lane for lane in lane_samples if lane != "guion"]:
            ratios = [g / o for g, o in zip(lane_samples["guion"], lane_samples[other])]
            low, high = bootstrap_median_ci(ratios)
            entry[f"guion_over_{other}"] = {
                "median": statistics.median(ratios),
                "ci95": [low, high],
            }
        summary[workload] = entry

    report = {
        "schema": "guion-array-benchmarks/v1",
        "programs": os.path.basename(PROGRAMS),
        "trials": trials,
        "machine": {
            "platform": platform.platform(),
            "processor": platform.processor(),
            "python": platform.python_version(),
        },
        "numpy": subprocess.run(
            [PYTHON, "-I", "-c", "import numpy; print(numpy.__version__)"],
            capture_output=True,
            text=True,
        ).stdout.strip(),
        "guion": subprocess.run([GUION, "--version"], capture_output=True, text=True)
        .stdout.strip(),
        "summary": summary,
        "samples_seconds": samples,
    }
    with open(output, "w") as handle:
        json.dump(report, handle, indent=2)
        handle.write("\n")
    for workload, entry in summary.items():
        line = f"{workload:<12}"
        for lane in samples[workload]:
            line += f" {lane} {entry[lane]:.4f}"
        for key, value in entry.items():
            if key.startswith("guion_over_"):
                low, high = value["ci95"]
                line += f"  {key} {value['median']:.3f} ({low:.3f}-{high:.3f})"
        print(line)


if __name__ == "__main__":
    main()
