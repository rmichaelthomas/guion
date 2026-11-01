import json, os, random, statistics, subprocess, sys, time, math
D = "/tmp/sc-base/jsonl-summary-data-200000"
PY = "/opt/cpython-3.14.7/bin/python3.14"
G = sys.argv[1]
TRIALS = int(sys.argv[2])
SIZES = [int(x) for x in sys.argv[3].split(",")]
FUSED_SRC = sys.argv[4]
os.chdir(D)
services = "performance-inputs/services.txt"
def build(src, out):
    subprocess.run([G, "build", src, "-o", out], check=True, capture_output=True)
build("base.gu", "base.gub")
build(FUSED_SRC, "fused.gub")
def norm(path):
    d = json.load(open(path))
    return sorted((g["service"], g["date"], g["count"], g["oldest"], g["newest"]) for g in d["groups"]), d["invalid"], d["ignored_count"]
def cmd(kind, n):
    ev = f"performance-inputs/ev{n}.jsonl"
    if kind == "python":
        return [PY, "data_reference.py", "python", ev, services, "o.json"]
    prog = {"base-src": "base.gu", "base-art": "base.gub", "fused-src": FUSED_SRC, "fused-art": "fused.gub"}[kind]
    return [G, "run", prog, "--events", ev, "--output", "o.json", "--services", services]
kinds = ["python", "base-src", "base-art", "fused-src", "fused-art"]
ref = {}
res = {}
rng = random.Random(7)
for n in SIZES:
    for kind in kinds:
        subprocess.run(cmd(kind, n), check=True, capture_output=True)  # warm and verify
        out = norm("o.json")
        ref.setdefault(n, out)
        assert out == ref[n], (kind, n)
    res[n] = {k: [] for k in kinds}
    for t in range(TRIALS):
        order = kinds[:]
        rng.shuffle(order)
        for kind in order:
            if os.path.exists("o.json"): os.remove("o.json")
            s = time.perf_counter()
            subprocess.run(cmd(kind, n), check=True, capture_output=True)
            res[n][kind].append(time.perf_counter() - s)
def upper95(g, p):
    r = [a / b for a, b in zip(g, p)]
    m = statistics.mean(r); sd = statistics.stdev(r)
    return statistics.median(r), m + 2.306 * sd / math.sqrt(len(r))  # t(0.975, 8)
for n in SIZES:
    p = res[n]["python"]
    print(f"rows={n} python median {statistics.median(p):.3f}s")
    for k in kinds[1:]:
        med, up = upper95(res[n][k], p)
        print(f"  {k:10s} median {statistics.median(res[n][k]):.3f}s ratio median {med:.3f} upper95 {up:.3f}")
json.dump(res, open("/tmp/pair-result.json", "w"))
