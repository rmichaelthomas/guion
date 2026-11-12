"""In-process timing of the filter-and-sum step after reading, Guion `frames` vs pandas.

Each side reads the 1M-row CSV once, runs one warm-up batch, then times 15 batches of 20
filter-and-sum repetitions (x > 0.5, sum y) with a wall clock (`unix_time_ms()` in Guion,
`time.time()` in pandas), accumulating every repetition's sum so none can be skipped.
Per-step time is batch time / 20. Usage: python3 step_timing.py
"""

import os
import statistics
import subprocess

HERE = os.path.dirname(os.path.abspath(__file__))
PROGRAMS = os.path.join(HERE, "programs")
ROOT = os.path.abspath(os.path.join(HERE, "..", "..", ".."))
GUION = os.path.join(ROOT, "target", "release", "guion")
PANDAS_PYTHON = os.path.join(HERE, ".venv", "bin", "python")
BATCH = 20


def refresh_package():
    """Copy the current `frames` sources into the project's package cache. The timed program
    imports `pkg:frames`, a snapshot taken when the package was added, so without this a
    run measures whatever the package looked like then."""
    done = subprocess.run([GUION, "update"], cwd=PROGRAMS, capture_output=True, text=True)
    if done.returncode != 0:
        raise SystemExit(f"guion update failed: {done.stderr}")


def lane(command):
    done = subprocess.run(command, cwd=PROGRAMS, capture_output=True, text=True)
    if done.returncode != 0:
        raise SystemExit(f"{command} failed: {done.stderr}")
    values = done.stdout.split()
    return [float(v) for v in values[:-1]], float(values[-1])


def main():
    refresh_package()
    guion, guion_total = lane([GUION, "run", "--allow-clock", "filter_step.gu"])
    pandas, pandas_total = lane([PANDAS_PYTHON, "-I", "filter_step_pd.py"])
    if abs(guion_total - pandas_total) > 1e-9 * max(1.0, abs(pandas_total)):
        raise SystemExit(f"totals differ: {guion_total} vs {pandas_total}")
    g = statistics.median(guion) / BATCH
    p = statistics.median(pandas) / BATCH
    print(f"guion  {g:.3f} ms per step (batches ms: {guion})")
    print(f"pandas {p:.3f} ms per step (batches ms: {pandas})")
    print(f"ratio  {g / p:.2f}x")


if __name__ == "__main__":
    main()
