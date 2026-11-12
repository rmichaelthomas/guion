"""Paired, interleaved, output-checked data-frame benchmarks: Guion `frames` vs pandas.

Usage: python3 generate.py; python3 -m venv .venv; .venv/bin/pip install -r requirements.txt;
       python3 run.py [trials] [output.json]

Workloads, each on the generated 1,000,000-row CSV: `null` (start-up only), `read` (read the
CSV), `filter` (x > 0.5, sum y), `group` (group by `group`, mean and count of x), `join`
(inner join to a 1,000-row lookup on `group`), `sort` (by `group`, then x descending).
Every workload reads the file, so `workload - read` estimates the work after reading.

Every trial runs each lane of each workload once, in a shuffled order, and records
end-to-end wall time (process start to exit, including compilation for Guion and the
pandas import). A Guion lane's output must match the pandas lane's output to a relative
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
PROGRAMS = os.path.join(HERE, "programs")
ROOT = os.path.abspath(os.path.join(HERE, "..", "..", ".."))
GUION = os.path.join(ROOT, "target", "release", "guion")
PANDAS_PYTHON = os.path.join(HERE, ".venv", "bin", "python")
WORKLOADS = ["null", "read", "filter", "group", "join", "sort"]


def lanes(workload):
    return {
        "guion": [GUION, "run", f"{workload}.gu"],
        "pandas": [PANDAS_PYTHON, "-I", f"{workload}_pd.py"],
    }


def setup():
    if not os.path.exists(os.path.join(PROGRAMS, "data", "rows.csv")):
        raise SystemExit("run `python3 generate.py` first")
    if not os.path.exists(os.path.join(PROGRAMS, "guion.lock")):
        package = os.path.join(ROOT, "packages", "frames")
        subprocess.run([GUION, "add", "frames", "--path", package], cwd=PROGRAMS, check=True)


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
    setup()
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
            reference = outputs[(workload, "pandas")]
            for lane in lanes(workload):
                if lane != "pandas" and not same(reference, outputs[(workload, lane)]):
                    raise SystemExit(
                        f"output mismatch in {workload}/{lane}: "
                        f"{outputs[(workload, lane)]} vs pandas {reference}"
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
        "schema": "guion-frame-benchmarks/v1",
        "trials": trials,
        "machine": {
            "platform": platform.platform(),
            "processor": platform.processor(),
            "python": platform.python_version(),
        },
        "pandas": subprocess.run(
            [PANDAS_PYTHON, "-I", "-c", "import pandas, numpy; print(pandas.__version__, numpy.__version__)"],
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
