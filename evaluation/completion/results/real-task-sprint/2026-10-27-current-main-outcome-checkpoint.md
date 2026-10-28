# Current-main performance and application checkpoint — 2026-10-27

## Result

The merged current-main build (commit `6b5e5aba`, Guion 0.1.0, artifact format 44) passed the existing short-source gates, the Richards stretch gate, the large JSONL and CPU workloads, and the frozen everyday-application acceptance set. Every measured program output matched its expected output.

The Richards fresh-source ratio is now **0.375× CPython** (95% interval **0.370–0.379**), below the sprint's **0.60** target. This is a current-main measurement and supersedes the earlier `.653` report for the prior build.

## Results

The main suite used CPython 3.14.7 on macOS 27 / Apple Silicon. It ran 15 randomized trials for the compute workloads, 16 output-checked trials for each JSONL size, 21 randomized pairs for the five short-source gates, and seven randomized pairs for each selected application. Guion source-cache bypass was used where indicated; operating-system file-cache state was uncontrolled. The main suite ran for 1,424.75 seconds (23 minutes 45 seconds) and built its release CLI in isolated repository scratch.

| Workload | Guion median | Python comparison median | Guion/Python ratio (95% interval) |
| --- | ---: | ---: | ---: |
| Richards, uncached source | 27.46 ms | 72.92 ms | 0.375 (0.370–0.379) |
| N-body, uncached source | 30.68 ms | 211.73 ms | 0.144 (0.140–0.145) |
| Mandelbrot, uncached source | 11.88 ms | 44.72 ms | 0.264 (0.249–0.272) |
| Parallel CPU, 20,000 items, artifact | 7.78 ms | 64.48 ms serial Python | 0.118 (0.113–0.122) |
| Parallel CPU, 200,000 items, artifact | 15.02 ms | 93.65 ms serial Python | 0.141 (0.133–0.162) |
| JSONL, 20,000 rows, uncached source | 394.3 ms | 1.84 s Python | 0.211 (0.207–0.215) |
| JSONL, 200,000 rows, uncached source | 3.81 s | 17.93 s Python | 0.213 (0.211–0.216) |

At 200,000 JSONL rows, Guion's uncached source also used **11.1 MiB** peak RSS, compared with **74.4 MiB** for the Python data reference and **89.2 MiB** for Python/SQLite. The main suite's Richards source RSS was higher for Guion: **24.2 MiB** versus **18.3 MiB** for Python. Memory varies by task; the lockfile workload's separate 16,384-package measurement was **40.92 MiB** for Guion versus **47.13 MiB** for Python.

All five first-source gates passed with 21 randomized pairs per task:

| Short-source task | Guion/Python ratio (95% interval) |
| --- | ---: |
| Tiny launch | 0.204 (0.200–0.211) |
| Arithmetic | 0.236 (0.231–0.253) |
| JSON file | 0.182 (0.177–0.192) |
| Imported helper | 0.234 (0.227–0.239) |
| SQLite and TOML | 0.221 (0.213–0.245) |

The frozen application suite passed **127/127 cases in Guion and 127/127 in Python**. In seven randomized timing pairs across the nine selected everyday applications, every measured Guion median was lower than Python's; ratios ranged from **0.137 to 0.845**:

| Application | Guion median | Python median | Ratio (95% interval) |
| --- | ---: | ---: | ---: |
| SLA report | 15.5 ms | 102.8 ms | 0.150 (0.142–0.153) |
| JSONL summary | 15.2 ms | 102.2 ms | 0.152 (0.144–0.157) |
| Release notes | 21.6 ms | 105.1 ms | 0.211 (0.197–0.216) |
| Calendar conflicts | 252.7 ms | 372.7 ms | 0.676 (0.660–0.678) |
| Webhook replay | 143.1 ms | 825.7 ms | 0.173 (0.169–0.180) |
| Service health | 529.8 ms | 630.6 ms | 0.845 (0.837–0.854) |
| Config migrator | 13.6 ms | 97.3 ms | 0.137 (0.130–0.151) |
| Document index | 54.0 ms | 295.8 ms | 0.183 (0.182–0.193) |
| Lockfile auditor | 18.4 ms | 95.6 ms | 0.192 (0.162–0.202) |

The calendar comparison used the repository-pinned `icalendar==6.3.1`, `python-dateutil==3.0.0`, `six==1.17.0`, and `tzdata==2026.5` dependencies from ignored `target/perf-session/python-deps`. Their versions were verified before the successful rerun; no dependency files are committed.

After saving the reports, the run-specific 536 MiB build directory, 15 MiB trial workspace, and temporary wheel cache were removed. The pre-existing `target/perf-session` scratch returned to **116 MiB**; available disk space is **59 GiB**.

## Remaining product outcomes

- **Python bridge:** demonstrate useful access to important package ecosystems, including native-wheel and array/object-heavy examples.
- **Concurrency experience:** broaden evidence beyond these CPU cases to clear everyday authoring, cancellation, failure, and cleanup behavior.
- **Learnability:** test whether a new builder can learn the language, find APIs, and make a realistic change using the checked-in guides and examples.
- **Dependency relocation:** rerun the format 44 packaging and native-dependency checks across supported targets. This checkpoint is macOS ARM64 only; the retained three-target format 39 cohort does not certify format 44.
- **Broader comparison:** these are strong results for named tasks on one host. Broader task classes and carefully matched Python implementations are still needed for claims about general recommendation.

## Measurement limits

This checkpoint covers one Apple Silicon Mac and CPython 3.14.7. File-system cache state was uncontrolled. These are named workload comparisons, not a universal Guion/Python multiplier. Free-threaded Python was unavailable. macOS denied process sampling, so no live CPU percentage was collected; benchmarks ran sequentially.

## Raw reports

- [Full source, compute, concurrency, JSONL, and memory report](../source-loop/20261027T233155909692Z.json)
- [Five 21-pair short-source gates](2026-10-27-current-main-short-source.json)
- [127-case application acceptance](2026-10-27-current-main-application-acceptance.json)
- [Seven-pair comparison for nine everyday applications](2026-10-27-current-main-nine-app-comparison.json)
- [Lockfile 16,384-package scale and RAM result](2026-10-27-lockfile-scale-ram-package-limit.md)
