# Task-class evidence and the dynamic-data gap

Branch `codex/guion-speed-closure`. The north star is to earn assistant recommendations over Python for specific task classes through comparative evidence, so results are reported per task class and not as one ratio.

## Per-class position

| Task class | Guion/Python ratio | Source | Notes |
| --- | ---: | --- | --- |
| Short-source launch, JSON file, imported helper, SQLite/TOML | 0.18–0.24 | [current-main checkpoint](../../completion/results/real-task-sprint/2026-10-27-current-main-outcome-checkpoint.md) | 21 randomized pairs each; macOS ARM64, format 44 |
| Compute (Richards, N-body, Mandelbrot) | 0.14–0.38 | same | CPython 3.14.7 |
| Parallel CPU (20k, 200k items) | 0.12–0.14 | same | serial Python reference |
| Nine everyday applications | 0.14–0.85 | same | 127/127 cases pass in both |
| JSONL summary, recorded corpus | 0.21 | same | different corpus and Python program than the sprint gate below; not comparable |
| **JSONL summary, sprint gate (frozen 200k-row corpus)** | **see the table below** | this branch, pinned Linux container, PGO CPython 3.14.7, 9 interleaved trials, `--services` filter, output checked against Python | the earlier 1.19 and 1.45 figures ran without the services filter and are not comparable |

The compute and everyday-application classes already favor Guion. The open gap is the dynamic-data class: per-row work made of `Json`, `Text`, `Map`, `Option` and `Result` values.

## Final paired measurement (format 50 binary)

Nine randomized interleaved trials per program in the pinned container, run with the `--services` allow-list, with Guion output checked against the Python reference before timing. Ratios are Guion/Python; the second figure is the upper bound of the 95% interval. Script: [`benchmark_jsonl_paired_python.py`](benchmark_jsonl_paired_python.py); raw samples: [`results/jsonl-fused-vs-python-paired-9.json`](results/jsonl-fused-vs-python-paired-9.json).

| Rows | Program | Lane | Median | Ratio (median / upper 95%) |
| ---: | --- | --- | ---: | ---: |
| 20,000 | Python | | 0.110 s | |
| 20,000 | frozen | source | 0.148 s | 1.350 / 1.413 |
| 20,000 | frozen | artifact | 0.143 s | 1.294 / 1.356 |
| 20,000 | fused accessors and `format_utc_date` | source | 0.139 s | 1.235 / 1.273 |
| 20,000 | fused accessors and `format_utc_date` | artifact | 0.120 s | 1.096 / 1.145 |
| 200,000 | Python | | 0.761 s | |
| 200,000 | frozen | source | 1.150 s | 1.488 / 1.637 |
| 200,000 | frozen | artifact | 1.263 s | 1.658 / 1.714 |
| 200,000 | fused accessors and `format_utc_date` | source | 1.049 s | 1.363 / 1.404 |
| 200,000 | fused accessors and `format_utc_date` | artifact | 1.026 s | 1.358 / 1.383 |

The variant program is [`jsonl-summary-fused-accessors.gu`](../core_north_star_sprint/profile-inputs/jsonl-summary-fused-accessors.gu). Its output is identical to the frozen program's. The frozen program's artifact lane at 200k is slower than its source lane here; that is an unexplained run-to-run difference and is reported as measured.

The gate (upper 95% below 1.0 in both lanes at both sizes) is not met.

## Holdout and second workload (format 50 binary, same container and Python)

**Holdout: lockfile auditor** (`evaluation/completion/performance/benchmark_lockfile_diagnostics.py`, 15 fresh-process source runs per size, outputs checked against the expected report; raw: [`results/lockfile-diagnostics-format50-vs-python.json`](results/lockfile-diagnostics-format50-vs-python.json)). Guion/Python median ratio 0.325 (bootstrap 95% 0.314–0.343) at 512 packages and 0.412 (0.404–0.418) at 2,048. Guion leads. The `candidate_over_baseline` field in that file compares the current source against itself (`--baseline-commit HEAD`) and is a noise control (about 1.00), not a regression check.

**Second workload: CSV request ledger** ([`second_workload/`](second_workload/)): a per-service group-by over `service,status,latency_ms,bytes` rows with typed integer parsing, about 1% malformed rows, 40 services, and Python's C `csv` module as the reference. Stdout is identical to the reference on every run. Nine interleaved trials (raw: [`results/csv-ledger-vs-python-paired-9.json`](results/csv-ledger-vs-python-paired-9.json)):

| Rows | Python | Lane | Median | Ratio (median / upper 95%) |
| ---: | ---: | --- | ---: | ---: |
| 20,000 | 0.020 s | source | 0.057 s | 2.813 / 2.839 |
| 20,000 | | artifact | 0.054 s | 2.624 / 2.684 |
| 200,000 | 0.119 s | source | 0.434 s | 3.623 / 3.696 |
| 200,000 | | artifact | 0.436 s | 3.611 / 3.667 |

Cumulative cost at 200k rows (ms): read rows only 140; plus field access and two `text_parse_integer` calls 250; full ledger with four map updates 450. Reading alone already costs more than the whole Python run, and it is five VM operations per row around one host call (`csv_next_row`, `is_ok`, `unwrap`, `is_some`, `unwrap`), so the cost is again per-operation overhead, not the CSV state machine.

The calendar-conflicts scaling harness was also tried and rejected as a second workload: its Python reference needs the third-party `icalendar` package, which the pinned CPython does not have, so the Python side failed (`results/calendar-scaling-format50-vs-python.json` records that failed preflight).

Reading across classes: Guion leads on short-lived and compute-bound programs, and trails Python on programs whose time is spent in many small operations over a large stream of rows, where Python runs its loop body in C (`csv`, `json`) and Guion runs it in the interpreter. The CSV gap (3.6×) is larger than the JSONL gap (1.36×), so the per-operation cost is the common lever, and it is worth more than any JSONL-specific change.

## What was measured on the JSONL gate workload (200k rows, container)

Cumulative wall time for loop bodies of increasing depth:

| Stage | ms |
| --- | ---: |
| `fs_next_line` only | 80 |
| plus `json_parse` | 205 |
| `jsonl_next` as written in the stdlib | 318 |
| plus whitespace collapse | 370 |
| plus two `json_get_safe` and two `json_text_safe` | 450 |
| plus timestamp parse and both formatters | 515 |
| full program | about 1,090 |

About 575 ms is the grouping logic after the fields are decoded. The PC profile of the full program is flat: the interpreter loop is 31%, and nothing else exceeds 5%. Spread across it are limit checks (about 8%), `Value` clone and drop, allocation, and Arc refcount traffic.

The grouping logic was split further with a synthetic 200k-iteration loop (10 services, 60 dates, no file I/O), adding one block at a time:

| Block added | ms |
| --- | ---: |
| loop, index two lists | 74 |
| `var by_date = {}` and the `groups[service]` lookup | 10 |
| `map_has`, `by_date[date]` and branch | 70 |
| three `json_int_safe(unwrap(json_get_safe(...)))` reads | 120 |
| up to three `json_set` updates and compares | 125 |
| two `map_set` writes | 110 |

Every block is about six operations at roughly 100 ns each, so the cost is per-operation overhead (call frame, `Result` box, unwrap, accounting) spread evenly, not one slow operation.

Levers tried, all with byte-identical output:

| Lever | Gain | Decision |
| --- | ---: | --- |
| Pool allocator (committed) | about 15% | kept |
| Fat LTO, one codegen unit (committed) | about 4% | kept |
| Typed `record Group` with `Map[Text, Map[Text, Group]]` instead of `Json` group objects | none (1.18 s vs 1.15 s) | not adopted |
| `format_utc_date` instead of `text_chars` plus nine concatenations | about 2.5% | valid idiom; frozen program left unchanged |
| Hand-inlined stdlib wrappers | about 6.5% | not built: conflicts with the forwarder call-count and depth-limit guarantees |
| Fuel carry across frames | none | reverted |
| Cancellation polling every 64 instructions | n/a | broke 3 HTTP cancellation tests; reverted |
| Register-VM dispatch | about 11% ceiling | not built |

Correction to an earlier note: charge-on-load is already O(1). `cloned_value_allocation` charges 0 for lists, maps and JSON because their storage is shared, so there was no load-charge quota to change.

## Language finding

Bytecode `Some`, `None`, `Ok` and `Err` take their types from the enclosing function's return type. The verifier rejects them elsewhere ("Option constructor outside Option-returning function") with a function-level `R_ARTIFACT_INVALID` at line 0:0, although the compiler accepts the source. Stdlib code works around this with one-line shim functions; `jsonl_next` makes six shim calls per row. The measured wrapper cost of `jsonl_next` over a direct `fs_next_line` plus `json_parse` loop is about 113 ms (about 10%), so the ceiling for removing the shims is under 7% on this workload.

Typed constructor ops (`MakeNoneOf(T)` and so on) would remove the shims and the opaque error. They need an artifact-format bump and matching changes in the self-hosted compiler, so they are recorded here as a candidate and not built. A cheaper first step is for the compiler to report this case at the source line.

## Position

Incremental levers on this workload are each 10% or less. The best measured candidates are a fused field accessor (get, type-check and unwrap in one operation, about 10%) and removing the `jsonl_next` shims (about 7%); lazy JSON parsing is now estimated at about 5%, because parsing is only 11% of the run. Closing the remaining gap (ratio 1.45 to below 1.0 needs about 31% less wall time) needs a different value representation for dynamic data, such as lazy JSON field access and fewer allocations per row, not another VM tweak. That is a multi-session engine project whose payoff is unmeasured until prototyped.

The gate is not met. The honest claim today is the per-class table above: Guion leads on compute, parallel, short-source, and everyday-application classes, and trails on large-corpus dynamic-data shaping.

## Option/Result prototype: negative result and the per-instruction finding

The planned prototype (cheaper `Option`/`Result` handling) was checked against the profile before any layout change. Results, all in the pinned container with identical output:

| Experiment | Measurement | Decision |
| --- | --- | --- |
| Quote-free fast path in the CSV field parser | read-only loop 140 to about 138 ms; full ledger 439 to 440 ms | reverted: the parser is not the cost |
| Inlined scalar fast path ahead of the recursive `value_limit_error` | integer loop 2.77 to 2.69 s (3%), text loop 5.0 to 4.9 s (3%), CSV ledger unchanged | reverted: under the 5% bar and does nothing for the target |
| `Box` per `Ok`/`Some` | not built: about 3 small pool allocations in roughly 25 instructions and 625 ns per row, so at most 3% | closed |

What the measurements do show: the CSV read-only loop is 25 VM instructions per row (500,080 for 20,000 rows) and the full ledger is 102 per row (2,042,237 for 20,000 rows), which is about 20–25 ns per instruction. A pure integer loop in the VM costs about 10 ns per instruction (2.77 s for 20 million iterations of about 14 instructions), roughly twice CPython's cost for the same loop. The PC profile of that integer loop puts 62% in the VM dispatch function itself, with the limit check (11%), `Value` clone (8%) and drop (5%) and the allocation accounting (4%) behind it. Compute benchmarks beat Python because their hot loops run in the native tier; data programs do not, because `main` is rejected from native compilation at the first mixed `Value` operation and the mixed-value bridge made things slower (2.67 s against 1.46 s).

So the remaining distance on data-shaping programs is the interpreter tier's per-instruction cost, not any one operation or container. Another small VM change is not expected to close it. The two ways to change that are a native tier that owns `Text`/`List`/`Map`/`Json` values (so data programs run native like compute programs), or a much leaner interpreter core. Both are multi-session projects.
