#!/usr/bin/env python3
"""Portable Guion/Python speed gate for the text, JSONL and CSV ledger workloads.

Builds every fixture from this repository in a scratch directory, so it runs on any host with a
Guion binary and a Python interpreter, without the pinned container's prepared files. Each
workload checks every lane's output against Python, then times fresh processes in a shuffled
order per trial. A lane passes when the one-sided 95% upper bound of its mean per-trial
Guion/Python ratio is below 1.0.

usage: run_speed_gate.py --guion GUION --python PYTHON [--trials 15] [--sizes 20000,200000]
                         [--workloads text,jsonl,csv] [--output report.json]
Exits 1 when any lane misses the gate.
"""

from __future__ import annotations

import argparse
import json
import math
import os
import platform
import random
import shutil
import statistics
import subprocess
import sys
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "evaluation/completion/performance"))
import runner  # noqa: E402

TEXT_WORKLOAD = ROOT / "evaluation/development/famous_benchmarks/text_processing"
FUSED_JSONL = ROOT / "evaluation/development/core_north_star_sprint/profile-inputs/jsonl-summary-fused-accessors.gu"
CSV_DIRECTORY = ROOT / "evaluation/development/core_cost_closure/second_workload"
RUNTIME_LIMITS = "[runtime]\ninstruction_limit = 2000000000\nallocation_limit_bytes = 8589934592\n"
# Two-sided 97.5% t quantiles by degrees of freedom, for the 95% upper bound of the mean ratio.
T_975 = {4: 2.776, 5: 2.571, 6: 2.447, 7: 2.365, 8: 2.306, 9: 2.262, 10: 2.228, 11: 2.201,
         12: 2.179, 13: 2.160, 14: 2.145, 19: 2.093, 24: 2.064, 29: 2.045}


def cpu_model() -> str:
    if platform.system() == "Darwin":
        return subprocess.run(["sysctl", "-n", "machdep.cpu.brand_string"], capture_output=True, text=True).stdout.strip()
    try:
        for line in Path("/proc/cpuinfo").read_text(encoding="utf-8").splitlines():
            key, _, value = line.partition(":")
            if key.strip() in ("model name", "CPU part"):
                return value.strip()
    except OSError:
        pass
    return platform.processor()


# Runs in a tiny interpreter that starts the lane and reports the lane's peak resident memory.
# The extra process is needed because on Linux a child's peak starts at the high-water mark of
# the process that launched it, so launching from this (much larger) script would put a floor
# of this script's own size under every lane. The tiny interpreter's floor is about 7 MiB, so a
# peak near that is a floor and not a measurement.
MEMORY_PROBE = (
    "import os,sys;os.chdir(sys.argv[1]);cmd=sys.argv[2:];"
    "devnull=os.open(os.devnull,os.O_WRONLY);"
    "pid=os.posix_spawnp(cmd[0],cmd,os.environ,file_actions=[(os.POSIX_SPAWN_DUP2,devnull,1),(os.POSIX_SPAWN_DUP2,devnull,2)]);"
    "_,status,usage=os.wait4(pid,0);print(usage.ru_maxrss,status)"
)
MEMORY_RUNS = 5


def peak_mib(command: list[str], cwd: Path) -> float:
    """Peak resident memory of one fresh run of `command`, in MiB. Linux reports KiB, macOS bytes."""
    out = subprocess.run([sys.executable, "-S", "-E", "-c", MEMORY_PROBE, str(cwd), *command],
                         check=True, capture_output=True, text=True).stdout.split()
    if int(out[1]) != 0:
        raise subprocess.CalledProcessError(int(out[1]), command)
    scale = 1 if platform.system() == "Darwin" else 1024
    return int(out[0]) * scale / (1024 * 1024)


def run(command: list[str], cwd: Path) -> str:
    return subprocess.run(command, cwd=cwd, check=True, capture_output=True, text=True).stdout


def write_project(project: Path, name: str) -> None:
    project.mkdir(parents=True)
    (project / "guion.toml").write_text(
        f'[project]\nname = "{name}"\nversion = "1.0.0"\nentry = "main.gu"\n' + RUNTIME_LIMITS,
        encoding="utf-8")


def text_lanes(guion: str, python: str, scratch: Path, rows: int):
    def sized(suffix: str) -> str:
        source = (TEXT_WORKLOAD.parent / (TEXT_WORKLOAD.name + suffix)).read_text(encoding="utf-8")
        if source.count("40000") != 1:
            raise SystemExit(f"expected one line count in {TEXT_WORKLOAD.name}{suffix}")
        return source.replace("40000", str(rows))

    project = scratch / f"text-{rows}"
    write_project(project, "text-gate")
    (project / "main.gu").write_text(sized(".vr"), encoding="utf-8")
    (project / "reference.py").write_text(sized(".py"), encoding="utf-8")
    run([guion, "build", "main.gu", "-o", "main.gub"], project)
    lanes = {
        "python": [python, "reference.py"],
        "source": [guion, "run", "main.gu"],
        "artifact": [guion, "run", "main.gub"],
    }
    return project, lanes, None


def jsonl_lanes(guion: str, python: str, scratch: Path, rows: int):
    project = scratch / f"jsonl-{rows}"
    shutil.copytree(runner.APP, project)
    runner.prepare_guion_application(project)
    manifest = project / "guion.toml"
    manifest.write_text(manifest.read_text(encoding="utf-8") + "\n" + RUNTIME_LIMITS, encoding="utf-8")
    shutil.copy2(FUSED_JSONL, project / "fused.gu")
    shutil.copy2(runner.DATA_REFERENCE, project / "data_reference.py")
    inputs = project / "performance-inputs"
    run([python, str(runner.GENERATOR), "--rows", str(rows), "--output", str(inputs / "events.jsonl"),
         "--services", str(inputs / "services.txt")], project)
    for name in ("main", "fused"):
        run([guion, "build", f"{name}.gu", "-o", f"{name}.gub"], project)
    data = ["performance-inputs/events.jsonl", "performance-inputs/services.txt"]
    arguments = ["--events", data[0], "--output", "summary.json", "--services", data[1]]
    lanes = {
        "python": [python, "data_reference.py", "python", *data, "summary.json"],
        "base-source": [guion, "run", "main.gu", *arguments],
        "base-artifact": [guion, "run", "main.gub", *arguments],
        "fused-source": [guion, "run", "fused.gu", *arguments],
        "fused-artifact": [guion, "run", "fused.gub", *arguments],
    }

    def summary() -> dict:
        return runner.normalize_data(json.loads((project / "summary.json").read_text(encoding="utf-8")))

    return project, lanes, summary


def csv_lanes(guion: str, python: str, scratch: Path, rows: int):
    project = scratch / f"csv-{rows}"
    write_project(project, "csv-ledger-gate")
    shutil.copy2(CSV_DIRECTORY / "csv-ledger.gu", project / "main.gu")
    run([python, str(CSV_DIRECTORY / "make_csv_ledger_data.py"), str(rows), "ledger.csv"], project)
    run([guion, "build", "main.gu", "-o", "main.gub"], project)
    lanes = {
        "python": [python, str(CSV_DIRECTORY / "csv_ledger_reference.py"), "--input", "ledger.csv"],
        # The source lane compiles on every run, as the CSV gate always has.
        "source": [guion, "run", "--no-cache", "main.gu", "--input", "ledger.csv"],
        "artifact": [guion, "run", "main.gub", "--input", "ledger.csv"],
    }
    return project, lanes, None


WORKLOADS = {"text": text_lanes, "jsonl": jsonl_lanes, "csv": csv_lanes}


def measure(prepare, guion: str, python: str, scratch: Path, rows: int, trials: int, rng: random.Random):
    # Lanes print their result, or write a file that `file_result` reads back.
    project, lanes, file_result = prepare(guion, python, scratch, rows)

    def result_of(lane: str):
        stdout = run(lanes[lane], project)
        return file_result() if file_result else stdout

    expected = result_of("python")
    for lane in lanes:  # checks every lane and warms the source lanes' compilation cache
        if result_of(lane) != expected:
            raise SystemExit(f"{lane} output differs from Python at {rows} rows in {project.name}")
    samples = {lane: [] for lane in lanes}
    for _ in range(trials):
        order = list(lanes)
        rng.shuffle(order)
        for lane in order:
            started = time.perf_counter()
            subprocess.run(lanes[lane], cwd=project, check=True, capture_output=True)
            samples[lane].append(time.perf_counter() - started)
    # Memory is measured apart from the timing, so the timing method is the one earlier runs used.
    memory = {lane: [peak_mib(lanes[lane], project) for _ in range(MEMORY_RUNS)] for lane in lanes}
    return samples, memory


def summarize(samples: dict[str, list[float]], trials: int, memory: dict[str, list[float]]) -> dict[str, dict[str, float]]:
    python_times = samples["python"]
    python_mib = statistics.median(memory["python"])
    summary = {"python": {"median_seconds": statistics.median(python_times), "median_peak_mib": python_mib}}
    for lane, times in samples.items():
        if lane == "python":
            continue
        ratios = [guion / python for guion, python in zip(times, python_times, strict=True)]
        upper = statistics.mean(ratios) + T_975[trials - 1] * statistics.stdev(ratios) / math.sqrt(trials)
        lane_mib = statistics.median(memory[lane])
        summary[lane] = {"median_seconds": statistics.median(times),
                         "median_ratio": statistics.median(ratios), "upper95": upper,
                         "median_peak_mib": lane_mib, "memory_ratio": lane_mib / python_mib}
    return summary


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--guion", required=True)
    parser.add_argument("--python", required=True)
    parser.add_argument("--trials", type=int, default=15)
    parser.add_argument("--sizes", default="20000,200000")
    parser.add_argument("--workloads", default="text,jsonl,csv")
    parser.add_argument("--output", type=Path)
    options = parser.parse_args()
    if options.trials - 1 not in T_975:
        parser.error(f"no t quantile for {options.trials} trials; use one of {sorted(d + 1 for d in T_975)}")
    guion = str(Path(options.guion).resolve())
    python = str(Path(options.python).absolute())
    sizes = [int(size) for size in options.sizes.split(",")]
    workloads = options.workloads.split(",")
    unknown = sorted(set(workloads) - set(WORKLOADS))
    if unknown:
        parser.error(f"unknown workloads {unknown}; choose from {sorted(WORKLOADS)}")

    report = {
        "host": {"system": platform.system(), "machine": platform.machine(), "cpu": cpu_model(),
                 "cpus": os.cpu_count()},
        "guion_version": run([guion, "--version"], ROOT).strip(),
        "python_version": run([python, "--version"], ROOT).strip(),
        "trials": options.trials,
        "workloads": {},
    }
    rng = random.Random(7)
    failures = []
    with tempfile.TemporaryDirectory(prefix="speed-gate-") as scratch_name:
        scratch = Path(scratch_name)
        for workload in workloads:
            report["workloads"][workload] = {}
            print(f"== {workload}", flush=True)
            for rows in sizes:
                samples, memory = measure(WORKLOADS[workload], guion, python, scratch, rows, options.trials, rng)
                summary = summarize(samples, options.trials, memory)
                report["workloads"][workload][str(rows)] = {"summary": summary, "samples_seconds": samples,
                                                            "samples_peak_mib": memory}
                print(f"rows={rows} python median {summary['python']['median_seconds']:.3f}s "
                      f"peak {summary['python']['median_peak_mib']:.1f} MiB")
                for lane, values in summary.items():
                    if lane == "python":
                        continue
                    passed = values["upper95"] < 1.0
                    if not passed:
                        failures.append(f"{workload} {rows} {lane}")
                    print(f"  {lane:15s} median {values['median_seconds']:.3f}s ratio median "
                          f"{values['median_ratio']:.3f} upper95 {values['upper95']:.3f}"
                          f"  peak {values['median_peak_mib']:.1f} MiB (x{values['memory_ratio']:.2f})"
                          f"{'' if passed else '  MISSED'}", flush=True)
    report["status"] = "failed" if failures else "passed"
    report["missed_lanes"] = failures
    if options.output:
        options.output.write_text(json.dumps(report, indent=1) + "\n", encoding="utf-8")
    print(f"gate {report['status']}" + (f": {', '.join(failures)}" if failures else ""))
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
