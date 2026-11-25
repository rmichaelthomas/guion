"""Guion against standard CPython, free-threaded CPython and PyPy, one local comparison.

Usage: python3 run.py [trials] [output.json]

Workloads: the plain-Python programs of the arrays kit (`*_py.py`, each with a Guion twin
`NAME.gu`) and the five first-source workloads (each with `main.gu` and `main.py`).
Protocol as in the arrays kit: every trial runs each lane of each workload once, in a
shuffled order, and records end-to-end wall time (process start to exit, including
compilation for Guion, with `--no-cache`). Every lane's output must match the standard
CPython lane (numbers to a relative 1e-9, everything else exactly) or the run stops.
Interpreters are found with `uv python find`; nothing is installed here.
"""

import importlib.util
import json
import os
import platform
import random
import shutil
import statistics
import subprocess
import sys
import tempfile
import time

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, "..", "..", ".."))
GUION = os.path.join(ROOT, "target", "release", "guion")
ARRAYS = os.path.join(HERE, "..", "arrays")
FIRST = os.path.join(ROOT, "evaluation", "first_source_speed")
SPECS = {"cpython": "3.14.7", "cpython-free-threaded": "3.14t", "pypy": "pypy@3.11"}
ARRAY_WORKLOADS = ["null", "elementwise", "sum", "matmul", "sort", "loop"]


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


arrays_kit = load("arrays_run", os.path.join(ARRAYS, "run.py"))
first_kit = load("first_run", os.path.join(FIRST, "run.py"))
bootstrap_median_ci = arrays_kit.bootstrap_median_ci


def find(spec):
    done = subprocess.run(["uv", "python", "find", spec], capture_output=True, text=True)
    if done.returncode != 0:
        raise SystemExit(f"uv cannot find {spec}: {done.stderr}")
    return done.stdout.strip()


def version(path):
    done = subprocess.run([path, "-c", "import sys; print(sys.version)"],
                          capture_output=True, text=True)
    return " ".join(done.stdout.split())


def build_jobs(interpreters, scratch):
    """workload name -> (source directory or None, cwd, {lane: command}).

    Arrays programs run in place (they only print). First-source workloads run in a fresh
    copy of their directory for every sample (`{P}` stands for it), because sqlite_toml
    writes a database; Python lanes there run without `-I` so `import helper` finds the
    script directory."""
    jobs = {}
    programs = os.path.join(ARRAYS, "programs")
    for name in ARRAY_WORKLOADS:
        lanes = {"guion": [GUION, "run", "--no-cache", f"{name}.gu"]}
        for lane, exe in interpreters.items():
            lanes[lane] = [exe, "-I", f"{name}_py.py"]
        jobs[f"arrays/{name}"] = (None, programs, lanes)
    for name, workload in first_kit.WORKLOADS_BY_NAME.items():
        project = "{P}"
        guion = [GUION, "run", "--no-cache"]
        if workload.needs_file_root:
            guion += ["--file-root", project]
        guion += ["main.gu", *workload.arguments]
        lanes = {"guion": guion}
        for lane, exe in interpreters.items():
            lanes[lane] = [exe, "main.py", *workload.arguments]
        jobs[f"first_source/{name}"] = (os.path.join(first_kit.WORKLOADS, name), project, lanes)
    return jobs


def run(command, cwd):
    start = time.perf_counter()
    done = subprocess.run(command, cwd=cwd, capture_output=True, text=True)
    elapsed = time.perf_counter() - start
    if done.returncode != 0:
        raise SystemExit(f"{command} failed: {done.stderr}")
    return elapsed, done.stdout.split()


def same(reference, values):
    if len(reference) != len(values):
        return False
    for expected, actual in zip(reference, values):
        try:
            e, a = float(expected), float(actual)
        except ValueError:
            if expected != actual:
                return False
            continue
        if abs(e - a) > 1e-9 * max(1.0, abs(e)):
            return False
    return True


def main():
    trials = int(sys.argv[1]) if len(sys.argv) > 1 else 15
    output = sys.argv[2] if len(sys.argv) > 2 else os.path.join(HERE, "results.json")
    interpreters = {lane: find(spec) for lane, spec in SPECS.items()}
    versions = {lane: version(path) for lane, path in interpreters.items()}
    scratch = tempfile.mkdtemp(prefix="guion-interpreters-")
    jobs = build_jobs(interpreters, scratch)
    samples = {w: {lane: [] for lane in lanes} for w, (_, _, lanes) in jobs.items()}
    order_rng = random.Random(2026)
    for trial in range(trials):
        order = [(w, lane) for w, (_, _, lanes) in jobs.items() for lane in lanes]
        order_rng.shuffle(order)
        outputs = {}
        for w, lane in order:
            source, cwd, lanes = jobs[w]
            command = lanes[lane]
            if source is not None:
                cwd = tempfile.mkdtemp(prefix=f"{lane}-", dir=scratch)
                shutil.copytree(source, cwd, dirs_exist_ok=True)
                command = [c.replace("{P}", cwd) for c in command]
            elapsed, values = run(command, cwd)
            if source is not None:
                shutil.rmtree(cwd, ignore_errors=True)
            samples[w][lane].append(elapsed)
            outputs[(w, lane)] = values
        for w, (_, _, lanes) in jobs.items():
            reference = outputs[(w, "cpython")]
            for lane in lanes:
                if lane != "cpython" and not same(reference, outputs[(w, lane)]):
                    raise SystemExit(f"output mismatch in {w}/{lane}: "
                                     f"{outputs[(w, lane)]} vs cpython {reference}")
        print(f"trial {trial + 1}/{trials} done", file=sys.stderr)

    summary = {}
    for w, lane_samples in samples.items():
        entry = {"median_seconds": {l: statistics.median(v) for l, v in lane_samples.items()}}
        for other in [l for l in lane_samples if l != "guion"]:
            ratios = [g / o for g, o in zip(lane_samples["guion"], lane_samples[other])]
            low, high = bootstrap_median_ci(ratios)
            entry[f"guion_over_{other}"] = {"median": statistics.median(ratios),
                                            "ci95": [low, high]}
        summary[w] = entry

    report = {
        "schema": "guion-interpreter-comparison/v1",
        "trials": trials,
        "machine": {"platform": platform.platform(), "processor": platform.processor(),
                    "machine": platform.machine(),
                    "memory_bytes": int(subprocess.run(["sysctl", "-n", "hw.memsize"], capture_output=True, text=True).stdout)},
        "guion": subprocess.run([GUION, "--version"], capture_output=True, text=True)
        .stdout.strip(),
        "interpreters": {lane: {"path": interpreters[lane], "spec": SPECS[lane],
                                "version": versions[lane]} for lane in interpreters},
        "summary": summary,
        "samples_seconds": samples,
    }
    with open(output, "w") as handle:
        json.dump(report, handle, indent=2)
        handle.write("\n")
    shutil.rmtree(scratch, ignore_errors=True)
    for w, entry in summary.items():
        line = f"{w:<28}"
        for lane, sec in entry["median_seconds"].items():
            line += f" {lane} {sec:.4f}"
        for key, value in entry.items():
            if key.startswith("guion_over_"):
                line += f"  {key[11:]} {value['median']:.3f} ({value['ci95'][0]:.3f}-{value['ci95'][1]:.3f})"
        print(line)


if __name__ == "__main__":
    main()
