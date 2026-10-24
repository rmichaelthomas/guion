#!/usr/bin/env python3
"""Matching CPU task used to compare Python serial/thread/process execution."""

from concurrent.futures import ProcessPoolExecutor, ThreadPoolExecutor
import sys

DEFAULT_COUNT = 20_000


def work(count: int) -> int:
    index = 0
    total = 0
    while index < count:
        total += index % 97
        index += 1
    return total


def main(mode: str, count: int) -> None:
    tasks = [count, count, count, count]
    if mode == "serial":
        results = [work(value) for value in tasks]
    elif mode == "threads":
        with ThreadPoolExecutor(max_workers=4) as executor:
            results = list(executor.map(work, tasks))
    elif mode == "processes":
        with ProcessPoolExecutor(max_workers=4) as executor:
            results = list(executor.map(work, tasks))
    else:
        raise SystemExit("mode must be serial, threads, or processes")
    print("\n".join(str(value) for value in results))


if __name__ == "__main__":
    if len(sys.argv) not in (2, 3):
        raise SystemExit("usage: parallel_cpu.py serial|threads|processes [count]")
    main(sys.argv[1], int(sys.argv[2]) if len(sys.argv) == 3 else DEFAULT_COUNT)
