"""Recursion benchmark: naive fib(32) in Guion and in Python, median of N runs.

Usage: python3 run.py [--guion PATH] [--runs N]
"""
import argparse, statistics, subprocess, sys, time
from pathlib import Path

HERE = Path(__file__).parent


def timed(command):
    start = time.perf_counter()
    done = subprocess.run(command, capture_output=True, text=True, cwd=HERE)
    elapsed = time.perf_counter() - start
    if done.returncode != 0:
        sys.exit(f"{command} failed: {done.stderr}")
    return elapsed, done.stdout.strip()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--guion", default=str(HERE.parents[2] / "target/release/guion"))
    parser.add_argument("--runs", type=int, default=5)
    args = parser.parse_args()
    rows = {}
    for name, command in (("guion", [args.guion, "run", "fib.gu"]), ("python", [sys.executable, "fib.py"])):
        times, outputs = [], set()
        for _ in range(args.runs):
            elapsed, out = timed(command)
            times.append(elapsed)
            outputs.add(out)
        if outputs != {"2178309"}:
            sys.exit(f"{name} printed {outputs}, expected 2178309")
        rows[name] = statistics.median(times)
        print(f"{name:7} median {rows[name]*1000:8.1f} ms over {args.runs} runs")
    print(f"guion / python = {rows['guion'] / rows['python']:.2f}")


main()
