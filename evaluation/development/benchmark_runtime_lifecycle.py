#!/usr/bin/env python3
"""Run a persistent-runtime Python/NumPy lifecycle stress workload."""

from __future__ import annotations

import argparse
import json
import math
import os
import platform
import subprocess
import sys
import threading
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
DEFAULT_REPORT = ROOT / "evaluation/results/runtime-lifecycle-stress.json"


def percentile(samples: list[float], quantile: float) -> float:
    if not samples:
        raise ValueError("at least one timing sample is required")
    if not 0 <= quantile <= 100:
        raise ValueError("quantile must be between 0 and 100")
    ordered = sorted(samples)
    position = (len(ordered) - 1) * quantile / 100
    lower = math.floor(position)
    upper = math.ceil(position)
    if lower == upper:
        return ordered[lower]
    fraction = position - lower
    return ordered[lower] * (1 - fraction) + ordered[upper] * fraction


def summarize_samples(samples_ns: list[int], duration_seconds: float) -> dict[str, object]:
    if not samples_ns:
        raise ValueError("at least one timing sample is required")
    if duration_seconds <= 0:
        raise ValueError("duration must be positive")
    samples_ms = [sample / 1_000_000 for sample in samples_ns]
    return {
        "sample_count": len(samples_ns),
        "samples_ms": [round(sample, 6) for sample in samples_ms],
        "p50_ms": round(percentile(samples_ms, 50), 6),
        "p95_ms": round(percentile(samples_ms, 95), 6),
        "p99_ms": round(percentile(samples_ms, 99), 6),
        "max_ms": round(max(samples_ms), 6),
        "throughput_calls_per_second": round(len(samples_ns) / duration_seconds, 3),
        "duration_seconds": round(duration_seconds, 6),
    }


def parse_process_table(output: str) -> list[tuple[int, int, int]]:
    processes = []
    for line in output.splitlines():
        columns = line.split()
        if len(columns) != 3:
            continue
        try:
            pid, parent_pid, rss_kb = (int(column) for column in columns)
        except ValueError:
            continue
        if pid > 0 and parent_pid >= 0 and rss_kb >= 0:
            processes.append((pid, parent_pid, rss_kb))
    return processes


def process_tree_memory(processes: list[tuple[int, int, int]], root_pid: int) -> dict[str, object] | None:
    by_pid = {pid: (parent_pid, rss_kb) for pid, parent_pid, rss_kb in processes}
    if root_pid not in by_pid:
        return None
    process_ids = {root_pid}
    while True:
        children = {pid for pid, (parent, _) in by_pid.items() if parent in process_ids}
        expanded = process_ids | children
        if expanded == process_ids:
            break
        process_ids = expanded
    root_rss_bytes = by_pid[root_pid][1] * 1024
    descendant_rss_bytes = sum(by_pid[pid][1] for pid in process_ids if pid != root_pid) * 1024
    return {
        "process_ids": sorted(process_ids),
        "root_rss_bytes": root_rss_bytes,
        "descendants_rss_bytes": descendant_rss_bytes,
        "tree_rss_bytes": root_rss_bytes + descendant_rss_bytes,
    }


def process_tree_rss(pid: int) -> dict[str, object] | None:
    try:
        result = subprocess.run(
            ["ps", "-axo", "pid=,ppid=,rss="],
            capture_output=True,
            text=True,
            check=False,
            timeout=2,
        )
        if result.returncode != 0:
            return None
        return process_tree_memory(parse_process_table(result.stdout), pid)
    except (OSError, subprocess.TimeoutExpired):
        return None


def run(binary: Path, python: Path, args: argparse.Namespace) -> dict[str, object]:
    command = [
        str(binary), str(python), str(args.array_size), str(args.burst_calls),
        str(args.paced_calls), str(args.pace_ms), str(args.warmups),
    ]
    process = subprocess.Popen(command, cwd=ROOT, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    memory_samples: list[dict[str, object]] = []
    memory_errors: list[str] = []
    done = threading.Event()

    def sample_memory() -> None:
        while not done.is_set():
            sample = process_tree_rss(process.pid)
            if sample is not None:
                memory_samples.append({"elapsed_seconds": round(time.monotonic() - started, 3), **sample})
            elif not memory_samples and not memory_errors:
                try:
                    subprocess.run(["ps", "-axo", "pid=,ppid=,rss="], capture_output=True, check=True, timeout=2)
                except (OSError, subprocess.SubprocessError) as error:
                    memory_errors.append(f"process-tree RSS sampling unavailable: {error}")
                else:
                    memory_errors.append("process-tree RSS sampling could not see the benchmark process")
            done.wait(0.05)

    started = time.monotonic()
    sampler = threading.Thread(target=sample_memory, daemon=True)
    sampler.start()
    try:
        stdout, stderr = process.communicate(timeout=args.timeout)
    except subprocess.TimeoutExpired:
        process.kill()
        stdout, stderr = process.communicate()
        raise RuntimeError(f"stress executable timed out after {args.timeout}s; stderr={stderr[-2000:]}")
    finally:
        done.set()
        sampler.join(timeout=2)
    if process.returncode:
        raise RuntimeError(f"stress executable exited {process.returncode}: {stderr[-4000:]}")
    try:
        runtime = json.loads(stdout)
    except json.JSONDecodeError as error:
        raise RuntimeError(f"expected JSON from stress executable, got {stdout[:1000]!r}; stderr={stderr[-1000:]}") from error
    root_values = [sample["root_rss_bytes"] for sample in memory_samples]
    descendant_values = [sample["descendants_rss_bytes"] for sample in memory_samples]
    tree_values = [sample["tree_rss_bytes"] for sample in memory_samples]
    runtime["process_memory"] = {
        "sampling_interval_ms": 50,
        "samples": memory_samples,
        "min_rss_bytes": min(root_values) if root_values else None,
        "max_rss_bytes": max(root_values) if root_values else None,
        "delta_rss_bytes": root_values[-1] - root_values[0] if len(root_values) > 1 else None,
        "min_descendant_rss_bytes": min(descendant_values) if descendant_values else None,
        "max_descendant_rss_bytes": max(descendant_values) if descendant_values else None,
        "delta_descendant_rss_bytes": descendant_values[-1] - descendant_values[0] if len(descendant_values) > 1 else None,
        "min_tree_rss_bytes": min(tree_values) if tree_values else None,
        "max_tree_rss_bytes": max(tree_values) if tree_values else None,
        "delta_tree_rss_bytes": tree_values[-1] - tree_values[0] if len(tree_values) > 1 else None,
        "sampling_error": memory_errors[0] if memory_errors else None,
        "sampling_note": "RSS sums the Rust benchmark process and its descendant processes, including the Python sidecar when it is a child. Shared pages are counted in each process; brief peaks may be missed.",
    }
    return {
        "schema_version": 1,
        "evidence_kind": "author-run stress probe; not independent recommendation evidence",
        "host": {
            "system": platform.platform(),
            "machine": platform.machine(),
            "cpu_count": os.cpu_count(),
        "python_executable": str(python),
            "python_version": subprocess.check_output([str(python), "--version"], text=True).strip(),
            "numpy_version": subprocess.check_output([str(python), "-c", "import numpy; print(numpy.__version__)"], text=True).strip(),
        },
        "configuration": vars(args) | {"binary": str(binary), "python": str(python), "report": str(args.report)},
        "runtime": runtime,
        "stderr": stderr[-4000:],
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=ROOT / "target/release/examples/runtime_lifecycle_stress")
    parser.add_argument("--python", type=Path, default=ROOT / "evaluation/development/python_interop/.veritas/python-bridge/.venv/bin/python3")
    parser.add_argument("--array-size", type=int, default=512)
    parser.add_argument("--burst-calls", type=int, default=1000)
    parser.add_argument("--paced-calls", type=int, default=100)
    parser.add_argument("--pace-ms", type=int, default=20)
    parser.add_argument("--warmups", type=int, default=20)
    parser.add_argument("--timeout", type=float, default=300)
    parser.add_argument("--report", type=Path, default=DEFAULT_REPORT)
    args = parser.parse_args()
    if min(args.array_size, args.burst_calls, args.paced_calls, args.warmups) < 1 or args.pace_ms < 0:
        parser.error("array size, call counts and warmups must be positive; pace must be nonnegative")
    if not args.binary.is_file():
        parser.error(f"stress executable does not exist: {args.binary}; build the runtime example first")
    if not args.python.is_file():
        parser.error(f"managed Python interpreter does not exist: {args.python}")
    # Preserve a venv's bin/python3 symlink: resolving it points at the base
    # interpreter and drops the environment's site-packages directory.
    report = run(args.binary.resolve(), args.python.absolute(), args)
    args.report.parent.mkdir(parents=True, exist_ok=True)
    args.report.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    runtime = report["runtime"]
    def compact(summary: dict[str, object]) -> dict[str, object]:
        return {key: value for key, value in summary.items() if key != "samples_ms"}

    print(json.dumps({
        "report": str(args.report),
        "host": report["host"],
        "burst": compact(summarize_samples(runtime["burst"]["latency_ns"], runtime["burst"]["elapsed_ns"] / 1_000_000_000)),
        "paced": compact(summarize_samples(runtime["paced"]["latency_ns"], runtime["paced"]["elapsed_ns"] / 1_000_000_000)),
        "scheduling_lateness": compact(summarize_samples(runtime["paced"]["scheduling_lateness_ns"], runtime["paced"]["elapsed_ns"] / 1_000_000_000)),
        "close_ms": round(runtime["close_ns"] / 1_000_000, 3),
        "process_memory": {key: value for key, value in runtime["process_memory"].items() if key != "samples"},
    }, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
