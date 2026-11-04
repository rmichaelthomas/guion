# B4 regex: local paired samples (2026-11-03)

Workload: 20 rounds of find_all (two patterns), replace_all and split over a 1,500-record, ~60 KB text. Both programs print 1206040. Wall-clock seconds from `/usr/bin/time -p`, five runs each, including process start-up (CPython start-up is itself about 0.02 s, so the figures are coarse at this size).

Guion (release build): 0.02 x5. CPython 3 `re`: 0.06 x3, 0.07 x2. Guion is not slower here because the Rust `regex` engine does the work; the workload is too short to claim a ratio.
