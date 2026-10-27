# JSON parsing duplicate-tracker follow-up — 2026-10-27

## What changed

The runtime JSON parser now passes duplicate-key state through a mutable stack reference instead of allocating an `Rc<RefCell<Option<String>>>` for each parsed document. The JSON projection path transfers a nested duplicate key into its shared error state only when a duplicate is found. This preserves duplicate-key errors, escaped-key handling, arbitrary-precision numbers, and projection behavior; it does not change Guion syntax, public APIs, or artifact format.

## Large JSONL baseline

The pre-change build processed 200,000 JSONL rows in a randomized 16-trial cohort. Every timed run was checked against the Python and SQLite output oracles.

| Implementation | Median | Peak RSS |
| --- | ---: | ---: |
| Guion source, cache warm | 3.844 s | 12.62 MiB |
| Guion source, cache bypassed | 3.799 s | 13.75 MiB |
| Guion artifact | 3.850 s | 12.44 MiB |
| Python data reference | 17.452 s | 74.20 MiB |
| Python with native SQLite | 17.625 s | 83.31 MiB |

For uncached source, Guion/Python was **0.218** (95% interval 0.215–0.221), about **4.6× faster** on this workload. Source compilation took about 5.3 ms; program execution took about 3.84 s, so compiler latency was not the limiting factor here. The operating-system file cache was uncontrolled.

An untimed guest-runtime profile recorded 31.9 million estimated allocation-budget charges, including 400,947 record charges, and zero list copy-on-write operations. These are VM accounting estimates, not host allocator calls or CPU-stack attribution. The [baseline timings and memory](2026-10-27-jsonl-200k-current-build.json) and [runtime profile](2026-10-27-jsonl-200k-runtime-profile.json) contain the raw data.

## Parser change result

A 7-pair, 20,000-row screen was inconclusive: candidate/baseline was 0.989 (95% interval 0.981–1.010). The larger workload gave a clearer result. In 16 randomized, output-checked pairs at 200,000 rows, the candidate/baseline median ratio was **0.994** (95% interval **0.987–0.998**), a **0.65%** time reduction. Median source time moved from 3.796 s to 3.763 s. Four of 16 individual pairs were ties or regressions. Peak RSS moved from 13.59 MiB to 13.53 MiB.

The 200k result is a small, statistically reliable improvement for this parser-heavy workload. It does not establish a general performance advantage. Runtime package verification passed: 180 tests across unit, execution, HTTP, lifecycle, standard-library data, and service suites; formatting and the optimized CLI build also passed.

See the [20k screen](2026-10-27-json-duplicate-tracker-20k-screen.json) and [200k confirmation](2026-10-27-json-duplicate-tracker-200k-confirmation.json).
