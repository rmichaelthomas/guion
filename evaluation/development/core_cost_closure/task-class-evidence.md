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

## Native-tier ceiling on the CSV ledger

Before building a native tier that owns `Text`/`List`/`Map` values, its payoff was bounded with the PC sampler on the 200k-row CSV ledger (three runs, 1,315 samples, format 50 release binary):

| Share of samples | Where |
| ---: | --- |
| 34% | VM dispatch loop (`execute_vm_frame_with_forwarders`) |
| about 9% | frame setup, `value_matches`, `host_op` dispatch |
| about 15% | `Value` clone and drop, limit checks, allocation accounting |
| about 8% | map lookup and replace (`BTreeMap::get`, `replace_map_value`) |
| about 7% | CSV read and integer parse |
| about 5.5% | atomic refcount and compare-and-swap traffic |
| about 4% | allocation |

A native tier removes dispatch and frame setup, at most about 43% of the run. Ownership, limits and allocation accounting must stay in native code, so the other shares remain. The ceiling is therefore about 450 ms to 260 ms, a Guion/Python ratio near 2.2 against the measured 3.6. It cannot reach 1.0 on this workload, so it was not built. The same shares on the JSONL gate program put its ceiling near 1.0 only if the native tier lost no accounting cost at all, which is not a result to plan on.

## Conclusion

The gate (upper 95% below 1.0 in the source and artifact lanes at 20k and 200k rows) is not met: JSONL is 1.36 at 200k and the CSV ledger is 3.6. Every lever the records list has been measured. The remaining ones are each under 10%, and the native-tier ceiling above is about 40%. The claim this evidence supports is per task class: Guion is faster than CPython on short-lived programs, compute-bound programs, parallel CPU work and the lockfile-auditor holdout, and is slower on programs that run a long interpreter loop over a large stream of rows.

## Measured bound on the owned-value native tier (VM profile, JSONL 200k, line-table build)

PC-sampled VM run (2,174 in-binary samples). Real data work is small: serde JSON parse 0.9%, file line read 1.0%, text collapse 0.9%, BTreeMap get 1.3%, `json_try_get` 0.9% (about 5–6% together). `execute_vm_frame_with_forwarders` is 28.1% and includes inlined handlers, so pure dispatch is below that. Value plumbing is about 22%: `drop_glue<Value>` 4.1%, `value_limit_error_with_json` 3.7%, `cloned_value_allocation` 3.2%, alloc 2.9%, `Value::clone` 2.6%, atomics 3.4%, `charge_allocation` 1.2%, `shared_json_metrics` 1.1%.

Reading: an owned-pointer native tier removes dispatch (at most about 28%) but keeps clone, drop, limit and quota work unless values also get an unboxed representation. If native callbacks matched VM handler cost, JSONL would land near 0.72 x 1.15 s = 0.83 s against Python's about 0.85 s: at best break-even with the gate, not a clear win, and only after every op family is native. The fully native run today is 1.60 s against the VM's 1.15 s, so callbacks currently cost more than VM handlers. CSV stays above 2x on the same arithmetic. This bound is measured from the profile, not from the earlier 43% guess.

## Native callback-path steps (bridge build, JSONL 200k, output hash a1a9b95d unchanged)

| Step | Native median | VM |
| --- | ---: | ---: |
| Before | 1.60 s | 1.10 s |
| Native-to-native calls hand remaining fuel to the callee through the output buffer instead of reserve/release on the shared budget | 1.57 s | 1.10 s |
| Literal Text/Bytes constants charge a compile-time byte count instead of an arena lookup | 1.56 s | 1.10 s |

Both steps pass the native test suites with and without `native-value-bridge`; the IR-size guard thresholds in `execution.rs` were raised to cover the extra fuel load/store per function. Run-level counts for the same program: 3.57M native calls, 20.2M successful allocation charges (about 100 per row), 4.38M record charges, 3.48 GB gross charged bytes. The profile stays flat: the native gap is spread across per-operation callbacks, arena stores and quota charges, not one hotspot.

## Load-time forwarder inlining (relaxes the forwarder call-count and depth guarantees)

`Runtime::load` now rewrites each `Call` to a tiny forwarding function (`Load 0..n; <host operation>; Return`) into the host operation itself, for host operations only (`MakeSome`/`MakeNone` forwarders stay as calls because the native backend does not lower them inside functions returning other types). Instruction indices, spans and stack effect are unchanged; no format bump, no compiler or seed change, so the source and artifact lanes both get it. The result-limit check that the forwarder's return applied is kept after each such host operation in the VM. User-approved relaxations: wrapper calls no longer appear in `function_call_counts`, no longer consume call depth, and no longer add a trace frame.

JSONL 200k, output hash a1a9b95d unchanged:

| Build | VM median | Native (bridge) median |
| --- | ---: | ---: |
| Before (fuel handoff only) | 1.10–1.15 s | 1.56 s |
| Host-op forwarders inlined | 1.00 s | 1.47 s |

A first attempt that also inlined `MakeSome`/`MakeNone` appeared to bring native to 1.08 s, but the profile showed `main` and `jsonl_next` had stopped compiling natively (6 native calls), so that figure was a VM run and is not used.

## Paired result after forwarder inlining and the JSON key pre-check (container, PGO CPython 3.14.7, 9 interleaved output-checked trials)

Raw output: [`results/jsonl-forwarder-inlined-paired-9.txt`](results/jsonl-forwarder-inlined-paired-9.txt) (binary before the JSON key pre-check). Guion/Python median / upper 95%:

| Rows | Program | Lane | Ratio |
| ---: | --- | --- | ---: |
| 20k | frozen | source | 1.238 / 1.312 |
| 20k | frozen | artifact | 1.082 / 1.149 |
| 20k | fused + `format_utc_date` | source | 1.144 / 1.238 |
| 20k | fused + `format_utc_date` | artifact | **0.995 / 1.047** |
| 200k | frozen | source | 1.385 / 1.427 |
| 200k | frozen | artifact | 1.354 / 1.392 |
| 200k | fused + `format_utc_date` | source | 1.325 / 1.355 |
| 200k | fused + `format_utc_date` | artifact | 1.250 / 1.286 |

The gate is still not met. Compared with the previous paired run, the frozen artifact lane at 200k moved from 1.658 to 1.354 and the fused artifact lane at 20k from 1.096 to 0.995. Per-row cost is about 4.5 us for Guion against about 3.5 us for Python at 200k rows. Executed instructions are about 265 per row (Load 72, Store 30, Const 23, JumpIfFalse 21, Unwrap 18, Not 13, Index 12); the remaining levers measured so far are each 1–5%: lazy JSON, typed constructors, superinstruction fusion of the top opcode pairs (Store-Load, Load-Const, Load-Unwrap, Not-JumpIfFalse), folding JSON metrics into the parse.

## Instruction-pair fusion (negative result)

Fused `Not`->`JumpIfFalse`, plain `Store`->`Load` and `Load`->`Const` inside the VM loop with exact fuel and quota accounting, plus a scalar fast path for `Const` limit checks (the existing `Load` fast paths already fuse `Load` with `IsOk`/`IsSome`/`Unwrap`/`GetField`/list `Index`, so those pairs in the transition profile were already single dispatches). Output hashes unchanged. JSONL 200k: 1.018 s to 0.998 s; second benchmark pair 0.913 s to 0.933 s; CSV 434 ms to 419 ms; integer loop 2467 ms to 2489 ms. All within run-to-run noise; reverted.

## Upper bound on the quota and limit plumbing (throwaway build, not committed)

To bound what a value-representation redesign could recover, a probe build turned `charge_allocation`, `Budget::allocate` and `value_limit_error` into no-ops (all allocation accounting and value-limit checks removed; output hash a1a9b95d unchanged). JSONL 200k: 1.02 s to about 0.92 s (median of the three runs after the first), roughly 10%. That is the ceiling for removing every quota charge and limit check, which a real design must keep in some form (compile-time-known sizes, batched charges), so the realistic recovery is well under 10%. The gap to Python at 200k is about 22%; quota and limit plumbing cannot close it, and the rest of the value cost (clone and drop with atomic refcounts, allocation, dispatch) is spread across the same flat profile.
