"""Paired, interleaved Guion/Python comparison for the CSV ledger second workload.

Usage: benchmark_csv_ledger_paired.py GUION_BINARY PYTHON TRIALS SIZES OUTPUT_JSON
Run from a project directory holding main.gu, guion.toml and ledger-<rows>.csv.
Every run's stdout is compared with the Python reference before it is timed.
"""
import json, math, os, random, statistics, subprocess, sys, time

G, PY, TRIALS, SIZES, OUT = sys.argv[1], sys.argv[2], int(sys.argv[3]), [int(x) for x in sys.argv[4].split(",")], sys.argv[5]
REF = os.environ["LEDGER_REFERENCE"]
T975 = {8: 2.306, 14: 2.145}[TRIALS - 1]
subprocess.run([G, "build", "main.gu", "-o", "main.gub"], check=True, capture_output=True)


def cmd(kind, n):
    data = f"ledger-{n}.csv"
    if kind == "python":
        return [PY, REF, "--input", data]
    return [G, "run", *(["--no-cache", "main.gu"] if kind == "source" else ["main.gub"]), "--input", data]


def run(kind, n):
    started = time.perf_counter()
    done = subprocess.run(cmd(kind, n), check=True, capture_output=True, text=True)
    return time.perf_counter() - started, done.stdout


kinds = ["python", "source", "artifact"]
rng = random.Random(11)
results = {}
for n in SIZES:
    expected = run("python", n)[1]
    for kind in kinds:
        assert run(kind, n)[1] == expected, (kind, n)
    samples = {kind: [] for kind in kinds}
    for _ in range(TRIALS):
        order = kinds[:]
        rng.shuffle(order)
        for kind in order:
            elapsed, stdout = run(kind, n)
            assert stdout == expected, (kind, n)
            samples[kind].append(elapsed)
    results[n] = samples
    python = samples["python"]
    print(f"rows={n} python median {statistics.median(python):.3f}s")
    for kind in kinds[1:]:
        ratios = [a / b for a, b in zip(samples[kind], python)]
        upper = statistics.mean(ratios) + T975 * statistics.stdev(ratios) / math.sqrt(len(ratios))
        print(f"  {kind:8s} median {statistics.median(samples[kind]):.3f}s ratio median {statistics.median(ratios):.3f} upper95 {upper:.3f}")
json.dump({"trials": TRIALS, "t_975": T975, "samples_seconds": {str(k): v for k, v in results.items()}}, open(OUT, "w"), indent=1)
