# Server speed gate, memory and repeat runs (2026-11-14)

A later run of the gate described in `2026-11-03-server-speed-gate.md`, on the same two GitHub
runner types (x86_64 AMD EPYC 7763 and ARM64 Neoverse), with the same CPython 3.14.7. This run
also records peak memory for each lane. The per-lane data is in the two JSON files beside this
page (GitHub Actions run 43632373445, 15 trials per lane).

## Repeat runs of the x86_64 text lane, 200,000 rows, source

The 95% upper bound of Guion's time divided by Python's was 1.000 on `main` (run 43629755934),
1.048 on a branch (run 43632373445) and 0.988 on the pull request (run 43636033331). The pass
line is 1.0. Shared runners vary from run to run, so this lane can land on either side of it.

## Peak memory, 200,000 rows

Peak resident memory of a fresh run, median of 5, measured through a tiny launcher process.
A peak near the launcher's own floor (about 11 to 13 MiB here) is a floor, not a measurement.

| Workload | x86_64 Guion | x86_64 Python | ARM64 Guion | ARM64 Python |
|---|---|---|---|---|
| text | 75.7 MiB | 68.2 MiB | 73.8 MiB | 70.3 MiB |
| JSON Lines (base) | 12.2 MiB | 71.2 MiB | 13.1 MiB | 73.5 MiB |
| CSV | 10.7 MiB | 11.4 MiB | 13.1 MiB | 13.7 MiB |

Ratio of Guion's peak to Python's: JSON Lines 0.17, text 1.11 on x86_64 and 1.05 on ARM64, CSV at the floor.
The text workload builds and keeps the whole document in both languages.
