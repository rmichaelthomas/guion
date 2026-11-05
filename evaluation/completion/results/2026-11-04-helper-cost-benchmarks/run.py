"""Time each helper program with a given guion binary: median of --trials runs.

    python3 run.py --binary target/release/guion [--trials 3] [--timeout 300]

The programs are in this directory. Each hands a list, map or text to a helper
function and stores the result back into its own variable, the common pattern.
"""
import argparse
import statistics
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
PROGRAMS = ["map_helper", "list_helper", "list_set_helper", "text_helper"]


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--trials", type=int, default=3)
    parser.add_argument("--timeout", type=int, default=300)
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    print(f"{binary.name}, median of {args.trials} runs")
    for name in PROGRAMS:
        times = []
        for _ in range(args.trials):
            started = time.perf_counter()
            try:
                run = subprocess.run(
                    [str(binary), "run", "--no-cache", str(HERE / f"{name}.gu")],
                    capture_output=True, text=True, timeout=args.timeout, cwd=HERE,
                )
            except subprocess.TimeoutExpired:
                print(f"{name}: over {args.timeout} s")
                break
            if run.returncode != 0:
                sys.exit(f"{name} failed: {run.stderr.strip()}")
            times.append(time.perf_counter() - started)
        else:
            print(f"{name}: {statistics.median(times):.3f} s ({min(times):.3f}-{max(times):.3f})")


if __name__ == "__main__":
    main()
