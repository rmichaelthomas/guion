#!/usr/bin/env python3
"""Matching CPU task used to compare Python serial/thread/process execution."""

from concurrent.futures import ProcessPoolExecutor, ThreadPoolExecutor
import json
import os
import platform
import resource
import sys

DEFAULT_COUNT = 20_000


def work(count: int) -> int:
    index = 0
    total = 0
    while index < count:
        total += index % 97
        index += 1
    return total


def work_with_memory(count: int) -> tuple[int, int, int]:
    result = work(count)
    peak = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
    peak_bytes = int(peak if platform.system() == "Darwin" else peak * 1024)
    return result, os.getpid(), peak_bytes


def main(mode: str, count: int, *, report_memory: bool = False) -> None:
    tasks = [count, count, count, count]
    worker_peaks: dict[int, int] = {}
    if mode == "serial":
        if report_memory:
            raise SystemExit("--report-memory is supported only with process mode")
        results = [work(value) for value in tasks]
    elif mode == "threads":
        if report_memory:
            raise SystemExit("--report-memory is supported only with process mode")
        with ThreadPoolExecutor(max_workers=4) as executor:
            results = list(executor.map(work, tasks))
    elif mode == "processes":
        with ProcessPoolExecutor(max_workers=4) as executor:
            if report_memory:
                observations = list(executor.map(work_with_memory, tasks))
                results = [result for result, _, _ in observations]
                for _, pid, peak_bytes in observations:
                    worker_peaks[pid] = max(worker_peaks.get(pid, 0), peak_bytes)
            else:
                results = list(executor.map(work, tasks))
    else:
        raise SystemExit("mode must be serial, threads, or processes")
    print("\n".join(str(value) for value in results))
    if report_memory:
        workers = [
            {"pid": pid, "peak_rss_bytes": peak}
            for pid, peak in sorted(worker_peaks.items())
        ]
        report = {
            "method": "resource.getrusage(resource.RUSAGE_SELF).ru_maxrss in each worker",
            "scope": "sum of each observed worker's peak (upper bound)",
            "worker_count": len(workers),
            "workers": workers,
            "sum_peak_rss_bytes_upper_bound": sum(item["peak_rss_bytes"] for item in workers),
        }
        print("GUION_WORKER_MEMORY=" + json.dumps(report, sort_keys=True), file=sys.stderr)


if __name__ == "__main__":
    arguments = sys.argv[1:]
    report_memory = "--report-memory" in arguments
    if report_memory:
        arguments.remove("--report-memory")
    if len(arguments) not in (1, 2):
        raise SystemExit("usage: parallel_cpu.py serial|threads|processes [count] [--report-memory]")
    main(arguments[0], int(arguments[1]) if len(arguments) == 2 else DEFAULT_COUNT, report_memory=report_memory)
