# Server speed gate: text, JSONL and CSV against CPython (2026-11-03)

**Result:** on both server hosts measured, Guion took less time than CPython on every lane of the speed gate, at 20,000 and 200,000 rows. The worst 95% upper bound of the Guion/Python time ratio was 0.976 on x86_64 (AMD EPYC 7763) and 0.859 on ARM64 (Neoverse N2). A lane passes when that upper bound is below 1.0.

## What was measured

- **Build:** commit `1746d10` (artifact format 51), profile-guided release binary built on each runner with `distribution/pgo/build.sh`. Its code is the code of commit `bfd0059` on branch `speed/csv-x86`.
- **Python:** the uv standalone CPython 3.14.7 (`uv python install 3.14.7`), the build most builders get.
- **Hosts:** GitHub's `ubuntu-24.04` runner (x86_64, AMD EPYC 7763, 4 CPUs) and `ubuntu-24.04-arm` runner (ARM64, Neoverse N2, 4 CPUs), Linux.
- **Method:** [`run_speed_gate.py`](../../../development/core_cost_closure/run_speed_gate.py), 15 trials per lane. Each trial runs every lane once as a fresh process, in a shuffled order. Every lane's output is checked against Python's before timing. The ratio is per trial, Guion time ÷ Python time; the bound is the one-sided 95% upper bound of the mean ratio. Times include process start.
- **Workloads:**
  - Text: split, trim, replace and find over generated lines.
  - JSONL: a per-service summary of an event log. The base and fused lanes are two Guion versions of the program.
  - CSV: a per-service ledger over `service,status,latency_ms,bytes` rows, using Python's C `csv` module as the reference.

  Source lanes run `guion run main.gu`; artifact lanes run a prebuilt `.gub`.

## x86_64, AMD EPYC 7763

| Workload | Rows | Lane | Guion median | Python median | Median ratio | Upper 95% |
|---|---|---|---|---|---|---|
| Text | 20,000 | source | 22.3 ms | 32.7 ms | 0.686 | 0.691 |
| Text | 20,000 | artifact | 21.8 ms | 32.7 ms | 0.669 | 0.674 |
| Text | 200,000 | source | 199.0 ms | 206.7 ms | 0.960 | 0.976 |
| Text | 200,000 | artifact | 197.3 ms | 206.7 ms | 0.957 | 0.967 |
| JSONL | 20,000 | base, source | 109.7 ms | 154.8 ms | 0.710 | 0.713 |
| JSONL | 20,000 | base, artifact | 106.7 ms | 154.8 ms | 0.689 | 0.695 |
| JSONL | 20,000 | fused, source | 106.4 ms | 154.8 ms | 0.684 | 0.694 |
| JSONL | 20,000 | fused, artifact | 103.0 ms | 154.8 ms | 0.665 | 0.673 |
| JSONL | 200,000 | base, source | 950.5 ms | 1063.9 ms | 0.897 | 0.904 |
| JSONL | 200,000 | base, artifact | 943.7 ms | 1063.9 ms | 0.892 | 0.895 |
| JSONL | 200,000 | fused, source | 915.2 ms | 1063.9 ms | 0.861 | 0.867 |
| JSONL | 200,000 | fused, artifact | 915.0 ms | 1063.9 ms | 0.860 | 0.866 |
| CSV | 20,000 | source | 18.1 ms | 28.9 ms | 0.623 | 0.629 |
| CSV | 20,000 | artifact | 17.1 ms | 28.9 ms | 0.589 | 0.595 |
| CSV | 200,000 | source | 140.5 ms | 161.0 ms | 0.875 | 0.881 |
| CSV | 200,000 | artifact | 139.2 ms | 161.0 ms | 0.862 | 0.866 |

## ARM64, Neoverse N2

| Workload | Rows | Lane | Guion median | Python median | Median ratio | Upper 95% |
|---|---|---|---|---|---|---|
| Text | 20,000 | source | 18.2 ms | 30.8 ms | 0.589 | 0.595 |
| Text | 20,000 | artifact | 17.8 ms | 30.8 ms | 0.576 | 0.579 |
| Text | 200,000 | source | 161.2 ms | 198.0 ms | 0.816 | 0.817 |
| Text | 200,000 | artifact | 160.4 ms | 198.0 ms | 0.810 | 0.814 |
| JSONL | 20,000 | base, source | 90.8 ms | 138.8 ms | 0.652 | 0.658 |
| JSONL | 20,000 | base, artifact | 88.2 ms | 138.8 ms | 0.635 | 0.640 |
| JSONL | 20,000 | fused, source | 90.0 ms | 138.8 ms | 0.650 | 0.652 |
| JSONL | 20,000 | fused, artifact | 86.9 ms | 138.8 ms | 0.627 | 0.630 |
| JSONL | 200,000 | base, source | 780.7 ms | 914.1 ms | 0.855 | 0.858 |
| JSONL | 200,000 | base, artifact | 781.9 ms | 914.1 ms | 0.854 | 0.859 |
| JSONL | 200,000 | fused, source | 773.8 ms | 914.1 ms | 0.849 | 0.851 |
| JSONL | 200,000 | fused, artifact | 771.7 ms | 914.1 ms | 0.844 | 0.846 |
| CSV | 20,000 | source | 15.2 ms | 26.9 ms | 0.567 | 0.570 |
| CSV | 20,000 | artifact | 14.4 ms | 26.9 ms | 0.536 | 0.539 |
| CSV | 200,000 | source | 118.3 ms | 147.9 ms | 0.799 | 0.801 |
| CSV | 200,000 | artifact | 116.9 ms | 147.9 ms | 0.789 | 0.795 |

The raw reports hold every trial: [x86_64](2026-11-03-server-speed-gate-x86_64.json) and [ARM64](2026-11-03-server-speed-gate-aarch64.json).

## Mean ratios with two-sided 95% intervals, 200,000 rows, artifact lanes

The gate's upper bound is the upper end of this interval: the mean per-trial ratio plus the t quantile for 14 degrees of freedom (2.145) times its standard error.

| Host | Workload | Mean ratio | 95% interval |
|---|---|---|---|
| x86_64 | Text | 0.954 | 0.940–0.967 |
| x86_64 | JSONL, base | 0.889 | 0.883–0.895 |
| x86_64 | JSONL, fused | 0.861 | 0.856–0.866 |
| x86_64 | CSV | 0.860 | 0.855–0.866 |
| ARM64 | Text | 0.810 | 0.807–0.814 |
| ARM64 | JSONL, base | 0.855 | 0.851–0.859 |
| ARM64 | JSONL, fused | 0.843 | 0.839–0.846 |
| ARM64 | CSV | 0.791 | 0.787–0.795 |

## Limits

- Three workloads, chosen as row-stream tasks where Python spends much of its time in C. They are not a broad benchmark suite.
- At 200,000 rows the margin on x86_64 is small: text is 3–4% under the line (upper bound 0.967–0.976). Runner-to-runner variance on the same CPU model was 1–3% in earlier runs.
- One run per host, on shared cloud machines without hardware counters. An Intel Xeon runner measured worse in an earlier run (200k ratios 1.12–1.43 on build `e1915f2`) and has not been measured on this build.
- The JSONL Python lane is the repository's data reference script, not the fastest possible Python.
- Not measured: free-threaded CPython, PyPy, Windows, and memory on these hosts.

The changes behind this result are described in the [task-class evidence](../../../development/core_cost_closure/task-class-evidence.md); the same-runner comparison against the earlier base is in [`speed-ab-csv-outline-20261103`](../../../development/core_cost_closure/results/speed-ab-csv-outline-20261103/).
