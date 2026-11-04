"""Times each Guion benchmark against CPython, alternating runs, and prints a Markdown table.

Usage: python3 run.py PATH_TO_GUION [TRIALS]
Each pair is checked for the same output first (the random benchmark compares only that both finish,
because the two generators are different algorithms and draw different numbers).
"""
import statistics
import subprocess
import sys
import time
from pathlib import Path

here = Path(__file__).resolve().parent
guion = sys.argv[1]
trials = int(sys.argv[2]) if len(sys.argv) > 2 else 7

LANES = [
    ("random_draws", ["random_draws.py"], False),
    ("unit_loop", ["unit_loop.py"], True),
    ("chained_appends", ["chained_appends.py", "chained_appends_idiomatic.py"], True),
]


def timed(command):
    start = time.perf_counter()
    done = subprocess.run(command, capture_output=True, text=True, check=True)
    return time.perf_counter() - start, done.stdout


def cell(samples):
    return f"{statistics.median(samples):.3f} s ({min(samples):.3f}–{max(samples):.3f})"


print(f"CPython {sys.version.split()[0]}, {trials} alternating runs per lane; median (min–max)\n")
print("| Benchmark | Guion | CPython | CPython, idiomatic form |")
print("|---|---|---|---|")
for name, pythons, same_output in LANES:
    _, guion_out = timed([guion, "run", str(here / f"{name}.gu")])
    for script in pythons:
        _, python_out = timed([sys.executable, str(here / script)])
        if same_output and guion_out != python_out:
            sys.exit(f"{name}: outputs differ for {script}")
    samples = {"guion": []} | {script: [] for script in pythons}
    for _ in range(trials):
        samples["guion"].append(timed([guion, "run", str(here / f"{name}.gu")])[0])
        for script in pythons:
            samples[script].append(timed([sys.executable, str(here / script)])[0])
    idiomatic = cell(samples[pythons[1]]) if len(pythons) > 1 else "same form"
    print(f"| {name} | {cell(samples['guion'])} | {cell(samples[pythons[0]])} | {idiomatic} |")
