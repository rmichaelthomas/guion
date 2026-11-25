# Speed report: baseline on 0.9.0

Date: 2026-11-25. Branch `perf/data-speed`. Guion 0.9.0 (artifact format 90, timezone data 2026c). This is the "before" measurement for the data-speed work: nothing in the engine changed between the 12 Nov report ([2026-11-12-speed-report.md](2026-11-12-speed-report.md)) and these runs except the releases since.

## Setup

One Apple M1 Pro (10 cores, 16 GB), macOS 27.0.1 (arm64), release build (`cargo build --release`), CPython 3.14.7, pandas 3.1.1 with NumPy 2.5.4. The arrays kit has no environment of its own and uses `sys.executable` for NumPy, so its scripts were run with `evaluation/development/frames/.venv/bin/python`: the arrays results use NumPy 2.5.4, not the 2.5.1 of the 12 Nov report. The machine was an ordinary desktop session (Spotify and the window server were using CPU), not a stripped quiet machine. Each kit ran one at a time, never alongside another job.

Commands, from the repository root:

```
cargo build --release
python3 evaluation/development/frames/run.py                      # 15 trials -> frames/results.json
cd evaluation/development/arrays
../frames/.venv/bin/python run.py                                   # 15 trials -> results.json
../frames/.venv/bin/python run.py 15 results_large.json programs_large
../frames/.venv/bin/python matmul_compute.py                        # 15 trials -> results_compute.json
python3 evaluation/development/frames/read_compute.py              # 31 trials -> frames/results_read_compute.json
```

Every kit ran its default protocol: interleaved, shuffled, output-checked trials (15 for the end-to-end kits and `matmul_compute.py`, 31 for `read_compute.py`). The "12 Nov" columns are the numbers in the earlier report. `matmul_compute.py` ran 15 trials here; the 12 Nov report quoted a 31-trial figure.

## Compute-only ratios (the numbers later tasks compare against)

| Workload | Guion | Competitor | Guion / competitor | Source |
|---|---|---|---|---|
| CSV read, 1M rows | 132.9 ms | pandas 49.7 ms | **2.67** (per-trial 1.70-4.60) | `frames/results_read_compute.json`, `ratio` |
| `a*b+c`, 5 passes, 10M `f64` | 127.2 ms | NumPy 94.8 ms | **1.34** | `arrays/results_large.json`, start-up subtracted |
| sum, 10 passes, 10M `f64` | 28.4 ms | NumPy 36.1 ms | **0.79** | `arrays/results_large.json`, start-up subtracted |
| stable sort, 1M `f64` | 31.6 ms | NumPy 21.1 ms | **1.50** | `arrays/results_large.json`, start-up subtracted |
| 512x512 `f64` matmul, per multiply | 1.380 ms | NumPy 0.617 ms | **2.24** (per-trial 1.09-5.73) | `arrays/results_compute.json`, `ratio_of_medians` |

Targets for the data-speed plan: CSV read, `a*b+c` and sort each at most 1.0. Baseline misses all three (2.67, 1.34, 1.50). Sum already beats NumPy; matmul is out of scope.

Methods. The read and matmul figures come from in-process lanes: each side runs the same program with 1 and with N+1 operations (6 reads for CSV, 21 multiplies for matmul) in one process, and the per-operation cost is (long run minus short run) divided by N, as a median over interleaved trials. Start-up, compilation, the library import and input construction cancel. The reads use the full row count and `column_sum(x)` as the agreement check (both 1000000 and 494764.345556 on both sides). The `a*b+c`, sum and sort figures use the 12 Nov method: for each trial subtract that lane's `null` (start-up only) time from the workload's time, then take the median over trials, per side. To reproduce them from the committed file:

```
python3 - <<'PY'
import json, statistics as s
sm = json.load(open("evaluation/development/arrays/results_large.json"))["samples_seconds"]
for w in ["elementwise", "sum", "sort"]:
    g = s.median(a - b for a, b in zip(sm[w]["guion"], sm["null"]["guion"]))
    n = s.median(a - b for a, b in zip(sm[w]["numpy"], sm["null"]["numpy"]))
    print(w, g * 1000, n * 1000, g / n)
PY
```

The sum figure (0.79) is lower than the "about 1.3" in the 12 Nov report; the end-to-end sum row is likewise lower than before (0.461 here, 0.612 then). I did not investigate the difference. The 12 Nov value may have come from a noisier estimate, or sum may have improved with a release since; I cannot say which from these runs.

## Frames against pandas, 1,000,000 rows (35.8 MB), default limits

Raw samples: `evaluation/development/frames/results.json`.

| Workload | Guion 12 Nov (s) | **Guion baseline 0.9.0 (s)** | pandas baseline (s) | Guion / pandas 12 Nov | **Guion / pandas baseline (95% interval)** |
|---|---|---|---|---|---|
| start-up only | 0.019 | 0.0285 | 0.3150 | 0.071 | 0.089 (0.086-0.091) |
| read the CSV | 0.209 | 0.1807 | 0.3790 | 0.654 | **0.479** (0.473-0.487) |
| read, filter, sum | 0.219 | 0.1891 | 0.3826 | 0.676 | 0.494 (0.470-0.501) |
| read, group by, mean and count | 0.225 | 0.1976 | 0.3934 | 0.673 | 0.501 (0.495-0.505) |
| read, inner join | 0.246 | 0.2235 | 0.4292 | 0.664 | 0.522 (0.515-0.531) |
| read, sort by two keys | 0.287 | 0.2619 | 0.5661 | 0.569 | 0.465 (0.458-0.468) |

Compute alone (workload minus start-up lane, median of per-trial differences): read is 153.2 ms for Guion against 67.1 ms for pandas, 2.28 times. The in-process lane gives the cleaner figure, 2.67 times (132.9 ms against 49.7 ms), and is the one later tasks quote. The 12 Nov report said "about 3 times pandas" (0.19 s against 0.064 s); this baseline is a little lower on both methods. The end-to-end win over pandas comes mostly from pandas' import.

Work after the read (median of per-trial differences from the read-only lane):

| Part | Guion 12 Nov | **Guion baseline** | pandas 12 Nov | **pandas baseline** | **Guion / pandas baseline** |
|---|---|---|---|---|---|
| filter and sum | ~9.7 ms | 7.0 ms | ~2.5 ms | 5.3 ms | 1.32 |
| group-by mean and count | ~16 ms | 15.7 ms | ~13 ms | 17.6 ms | 0.89 |
| inner join | ~37 ms | 41.7 ms | ~50 ms | 49.2 ms | 0.85 |
| two-key sort | ~78 ms | 82.1 ms | ~187 ms | 188.7 ms | 0.44 |

These are differences of noisy end-to-end medians, as in the 12 Nov report.

## Arrays against NumPy, the plan's sizes (10M elements, 512x512 matmul)

Raw samples: `evaluation/development/arrays/results_large.json`. NumPy 2.5.4.

| Workload | Guion 12 Nov (s) | **Guion baseline (s)** | NumPy baseline (s) | Guion / NumPy 12 Nov | **Guion / NumPy baseline (95% interval)** |
|---|---|---|---|---|---|
| start-up only | 0.0142 | 0.0196 | 0.0705 | 0.233 | 0.292 (0.283-0.294) |
| `a*b+c`, 5 passes | 0.1413 | 0.1472 | 0.1651 | 0.925 | **0.888** (0.856-0.911) |
| sum, 10 passes | 0.0584 | 0.0487 | 0.1043 | 0.612 | 0.461 (0.458-0.471) |
| 512x512 matmul | 0.0166 | 0.0227 | 0.0747 | 0.258 | **0.306** (0.302-0.311) |
| stable sort of 1M | 0.0452 | 0.0514 | 0.0904 | 0.559 | 0.565 (0.547-0.586) |
| 1M `array_get` loop | 0.0871 | 0.0929 | 0.2048 | 0.446 | 0.458 (0.448-0.467) |

Start-up for both sides is a few milliseconds slower than on 12 Nov (Guion 0.0142 to 0.0196 s, NumPy 0.0610 to 0.0705 s), so this run was on a somewhat busier machine; the ratios are what carry over.

## Arrays at smaller sizes (4M elements, 256x256), with plain CPython

Raw samples: `evaluation/development/arrays/results.json`.

| Workload | Guion (s) | NumPy (s) | CPython (s) | Guion / NumPy (95% interval) | Guion / CPython |
|---|---|---|---|---|---|
| start-up only | 0.0194 | 0.0687 | 0.0266 | 0.286 (0.281-0.295) | 0.730 |
| `a*b+c`, 5 passes | 0.0716 | 0.1049 | 1.5621 | 0.682 (0.636-0.722) | 0.046 |
| sum, 10 passes | 0.0301 | 0.0827 | 0.3216 | 0.367 (0.353-0.376) | 0.093 |
| 256x256 matmul | 0.0204 | 0.0686 | 0.5858 | 0.291 (0.278-0.306) | 0.035 |
| sort of 1M | 0.0518 | 0.0873 | 0.1819 | 0.597 (0.572-0.606) | 0.282 |
| 1M `array_get` loop | 0.0921 | 0.1991 | 0.1365 | 0.452 (0.436-0.468) | 0.666 |

(12 Nov, Guion / NumPy: `a*b+c` 0.674, sum 0.441, matmul 0.242, sort 0.563, loop 0.431.)

## Matrix multiplication, compute only

`evaluation/development/arrays/matmul_compute.py`, 15 trials, `results_compute.json`: Guion 1.380 ms per 512x512 multiply, NumPy 0.617 ms, ratio **2.24** (per-trial 1.09-5.73). The 12 Nov report committed 2.26 (31 trials; 1.46 ms against 0.64 ms), so matmul is unchanged within the run-to-run spread. Each multiply includes a 262,144-element `array_sum` on both sides.

## What was added

`evaluation/development/frames/read_compute.py` with programs in `programs_compute/` (`read_x1.gu`, `read_x6.gu`, `read_x1_pd.py`, `read_x6_pd.py`) and the committed `results_read_compute.json`. It prints and stores `guion_ms`, `pandas_ms` and `ratio` (medians over interleaved trials, default 31). `programs_compute/data/rows.csv` is a hard link to `programs/data/rows.csv`, created by the script, because the package's file capability refuses a symbolic link; the link, `guion.lock` and `.guion/` there are ignored by git.

## Caveats

- One machine, one size, one day. The per-trial ratios for the in-process lanes span a factor of 2.5 to 5; the medians are what to compare, and a later task should expect about 10% movement between runs of the same build.
- NumPy is 2.5.4 here, 2.5.1 in the 12 Nov arrays rows.
- The machine was not idle (see Setup); start-up lanes ran 5 to 10 ms slower than on 12 Nov.
