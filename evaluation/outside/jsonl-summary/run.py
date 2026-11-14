#!/usr/bin/env python3
"""Time the outside JSONL summarizer: Guion vs its Python twin vs hello world.

Builds nothing. Generates data into a temporary directory, runs each case
REPS times, reports medians in milliseconds, and exits non-zero if the Guion
and Python outputs differ byte for byte (or any run fails).
"""
import argparse
import json
import os
import shutil
import statistics
import subprocess
import sys
import tempfile
import time

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.abspath(os.path.join(HERE, "..", "..", ".."))
sys.path.insert(0, HERE)
import gen  # noqa: E402

HELLO = 'fn main() -> Int:\n    print("hello")\n    return 0\n'


def timed(cmd, cwd, reps):
    times, out = [], None
    # One untimed run first: the first run after a build or another command pays for cold
    # caches, and for a few-millisecond program that is larger than the allowance.
    subprocess.run(cmd, cwd=cwd, capture_output=True)
    for _ in range(reps):
        start = time.perf_counter()
        done = subprocess.run(cmd, cwd=cwd, capture_output=True)
        times.append((time.perf_counter() - start) * 1000.0)
        if done.returncode != 0:
            raise RuntimeError(f"{cmd} exited {done.returncode}: {done.stderr[:300]!r}")
        if out is None:
            out = done.stdout
        elif done.stdout != out:
            raise RuntimeError(f"{cmd} gave different output between runs")
    return statistics.median(times), out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--binary", default=os.path.join(REPO, "target", "release", "guion"))
    ap.add_argument("--rows", type=int, nargs="+", default=[5000, 200000])
    ap.add_argument("--reps", type=int, default=7)
    ap.add_argument("--json", metavar="PATH", help="also write the medians to PATH as JSON")
    args = ap.parse_args()
    binary = os.path.abspath(args.binary)

    ok = True
    result = {"reps": args.reps, "rows": {}}
    with tempfile.TemporaryDirectory(prefix="guion-jsonl-") as tmp:
        hello = os.path.join(tmp, "hello")
        os.makedirs(hello)
        with open(os.path.join(hello, "hello.gu"), "w") as f:
            f.write(HELLO)
        med, _ = timed([binary, "run", "hello.gu"], hello, args.reps)
        print(f"hello world                      median {med:9.1f} ms  ({args.reps} reps)")
        result["hello_ms"] = round(med, 1)

        for rows in args.rows:
            work = os.path.join(tmp, f"rows{rows}")
            shutil.copytree(os.path.join(HERE, "src"), os.path.join(work, "src"))
            shutil.copy(os.path.join(HERE, "guion.toml"), work)
            gen.generate(rows, os.path.join(work, "access.jsonl"))
            g_med, g_out = timed([binary, "run", "."], work, args.reps)
            p_med, p_out = timed(
                [sys.executable, os.path.join(HERE, "summarize.py")], work, args.reps
            )
            same = g_out == p_out
            ok = ok and same
            print(f"guion  {rows:>7} rows            median {g_med:9.1f} ms")
            print(f"python {rows:>7} rows            median {p_med:9.1f} ms")
            print(f"  output byte-identical: {same}")
            result["rows"][str(rows)] = {
                "guion_ms": round(g_med, 1),
                "python_ms": round(p_med, 1),
                "output_identical": same,
            }
    result["outputs_identical"] = ok
    if args.json:
        with open(args.json, "w") as out:
            json.dump(result, out, indent=1)
            out.write("\n")
    if not ok:
        print("FAIL: Guion and Python outputs differ", file=sys.stderr)
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
