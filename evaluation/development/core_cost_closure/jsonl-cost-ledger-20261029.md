# JSONL 200k cost ledger — completed research-target audit

## Status against the three-step investigation

| Step originally promised | Status | Result |
| --- | --- | --- |
| 1. Audit one row from input to output | **Complete for selecting research targets.** | Additive execution timing, actual heap API counts, CPU samples, source-operation comparison, and profiler overhead are recorded below. These measurements do not identify one independently sufficient opcode fix. |
| 2. Work backward from the required saving | **Complete as a screening budget.** | The arithmetic below ranks feasible cost combinations. It does not predict that a proposed implementation will achieve them. |
| 3. Research the measured bottlenecks and code history | **Complete as research and a proposed implementation sequence.** | See the [step-3 synthesis](jsonl-step3-research-20261029.md). No speedup is claimed yet. |

An earlier statement called the additive phase measurement alone “step 1 complete”; that was premature. The subsequent CPU and actual heap-allocation audits close the promised research-target audit. Exact per-opcode time is not a prerequisite for step 3, and the measurements do not establish a single root cause.

## Baseline and target

The final output-checked 15-pair artifact run measured a Guion median of **2.651506 s** and a pinned Python 3.11.2 median of **0.734498 s**. Matching those absolute medians needs **1.917008 s**, or **72.3%**, less Guion time. This is a screening calculation; the actual closure gate is a paired 95% upper candidate/Python ratio below 1.0 in all four source/artifact lanes. See the [final paired run](results/sprint-20261029-final-current-clean-paired-15.json) and [sprint outcome](results/sprint-20261029-jsonl-core-performance-outcome.md).

## Step 2: backward speed budget

The absolute median gaps are **1.917008 s** at 200k artifact, **1.994090 s** at 200k source, **0.178392 s** at 20k artifact, and **0.189366 s** at 20k source. The 200k artifact lane is the useful budget for the runtime hot path; the source lane additionally includes compilation/startup. These are differences of lane medians, not paired confidence bounds.

The additive diagnostic run below averaged **2.765868 s**. Compared with Python's 0.734498 s median, that diagnostic run has a **2.031370 s** gap. Because the diagnostic run carries 0.218 s median timing overhead, these are *screening scenarios within the diagnostic time budget*, not production predictions:

| Hypothetical VM-body reduction | Further host reduction needed to reach 0.734498 s in diagnostic budget | Fraction of measured 0.773362 s host category |
| ---: | ---: | ---: |
| 70% | 0.693 s | 89.6% |
| 80% | 0.502 s | 64.9% |
| 90% | 0.311 s | 40.2% |
| 100% | 0.119 s | 15.4% |

The frame-creation and driver categories total only **0.080433 s** in the diagnostic run. Their complete removal would address under 4% of the 2.031370 s diagnostic gap. The host category alone is 0.773362 s, also too small. Even complete removal of the 1.911960 s VM-body category would still leave 0.119411 s to remove elsewhere in that diagnostic budget. Thus the measured categories point to a combined VM-body and host improvement. The on/off overhead and cross-run Python comparison prevent treating these as exact production speed ceilings.

## Additive execution audit

A temporary timing patch measured the frozen 200k artifact with the same release diagnostic binary, source, artifact, and input in **15 randomized on/off pairs**. Every run returned success and reproduced the prior output SHA-256. The timer was enabled only in the `on` runs. The five categories below add *exactly* to each timed program execution in each `on` run; the table uses the means of those 15 runs, which also add exactly. Raw trials: [additive timing record](results/sprint-20261029-jsonl-additive-cost-trials-15.json). Reproduction: apply the [temporary instrumentation patch](results/sprint-20261029-jsonl-additive-cost-instrumentation.patch), build a release `guion-cli`, and run [the paired driver](run_jsonl_cost_ledger.py) on the frozen artifact and inputs. The temporary VM code was removed from the working tree after measurement.

| Exclusive category in diagnostic run | Mean time | Share of diagnostic execution | About per 200k input row | Boundary |
| --- | ---: | ---: | ---: | --- |
| VM instruction body outside host calls | **1,911.960 ms** | **69.13%** | 9.56 µs | Dispatch loop, stack/local traffic, guest value semantics, allocation checks, and timing overhead inside VM segments. |
| Host operations called by VM | **773.362 ms** | **27.96%** | 3.87 µs | Time inside 3,384,984 calls to `host_op`, including their timer overhead. |
| VM driver bookkeeping | 48.676 ms | 1.76% | 0.24 µs | Parent/child frame stack transitions and driver work outside timed VM segments and `start_frame`. |
| Frame creation via `start_frame` | 31.756 ms | 1.15% | 0.16 µs | 200,952 actual frame starts. |
| Program execution outside VM driver | 0.114 ms | 0.004% | <0.01 µs | Invocation setup and return outside `run_frames_with_stack`. |
| **Total** | **2,765.868 ms** | **100%** | **13.83 µs** | Same-run measured program execution; includes diagnostic overhead. |

The `off` runs of that same binary averaged **2.555742 s** (median 2.529173 s); the `on` runs averaged **2.765868 s** (median 2.745442 s). The paired median added time was **218.225 ms**, with a percentile bootstrap 95% interval of about **186–268 ms** for that median. Instrumentation therefore added roughly **8.9%** at the paired median ratio. It is not valid to assign all that overhead to one category or treat the instrumented shares as exact production shares. The direct finding is that the VM instruction body dominates the diagnostic execution, host calls are substantial, and frame creation plus driver bookkeeping are small. The low-overhead `off` run remains near the previous uninstrumented artifact result, but it is a different binary and run cohort.

The detailed profiler reported **3,770,554 VM frames**, but the additive timer observed **401,905 `execute_vm_frame` segments** and **200,952 `start_frame` calls**. The profiler's frame count includes short forwarded function executions and resumed frames; it must not be interpreted as 3.77 million heap frame constructions. The tiny measured `start_frame` category rules out frame creation alone as the missing 1.917 s. Call/return and wrapper work can still be embedded in the VM instruction body and host categories.

## Work counts and narrower clues

The separate detailed profile records **61,476,177 VM instructions** (about 307 per 200k input row), **3,569,601 `Call`**, **3,569,602 `Return`**, **17,820,595 `Load`**, **5,955,380 `Store`**, **1,590,885 `IsOk`**, **601,896 `IsSome`**, and **3,560,465 `Unwrap`**. These are overlapping subsets of VM work, not time categories to add. Native compiled functions and native calls were **zero** for this workload. `main` and `jsonl_next` first fail native eligibility at `IsOk`; typed JSON/Result operations exclude the JSON helpers. See [detailed artifact profile](results/sprint-20261029-current-profile-200k-artifact.json), [std:json](../../../language/stdlib/json.gu), [std:jsonl](../../../language/stdlib/jsonl.gu), and [the workload](../core_north_star_sprint/profile-inputs/jsonl-summary-baseline.gu).

That detailed, much more intrusive profile recorded these inclusive host-operation observations: `JsonTryGet` **158.586 ms / 985,575 calls**, `JsonParse` **151.194 ms / 200,947 calls**, `FileNextLine` **107.711 ms / 200,948 calls**, `JsonSetLocal` **93.353 ms / 205,283 calls**, `JsonTryInt` **68.338 ms / 588,723 calls**, `TimeFormatUtc` **64.234 ms / 196,381 calls**, `JsonInt` **57.214 ms / 202,151 calls**, and `JsonTryText` **49.013 ms / 396,852 calls**. Their times belong to a *different instrumented build* and are not an additive partition of the 773 ms host category above. JSON parse subphase timings are contained within `JsonParse`.

The detailed profile also records **27,543,651 successful guest allocation budget charges**, estimated at **1,835,423,993 logical bytes**. These are not actual allocator calls or retained bytes. They identify allocation accounting and value handling as candidates *within* the 1.912 s VM instruction body, but do not prove their individual time cost. Peak RSS in the final paired run was 9,699,328 bytes.

## Source-level operation comparison with Python

The [Python reference](../../completion/performance/data_reference.py) parses each nonblank JSON line, validates service and timestamp, then aggregates accepted events in a `dict` keyed by `(service, day)` with a mutable three-item list holding count, oldest, and newest. The [Guion workload](../core_north_star_sprint/profile-inputs/jsonl-summary-baseline.gu) also parses and validates each line, but aggregates into nested `Map[Text, Map[Text, Json]]` values. Updating an existing group reads the old JSON object, converts several JSON fields, constructs new JSON values, and applies `json_set` before replacing the map entry. Both produce the frozen expected output; they perform different intermediate operations.

The detailed Guion profile records **392,762 `MapSet` operations**, all in-place under its local-ownership optimization, and **205,283 `JsonSetLocal` operations**. Of the JSON updates, **196,241 caused copy-on-write copies**, with an estimated **94,786,273 copied bytes**. These are implementation counters, not exclusive time. They are a concrete benchmark-level asymmetry to measure against a typed Guion aggregation state before attributing the whole gap to the VM. The Guion program also calls `text_chars` **196,381** times after UTC formatting to form a date from ten characters; Python slices the formatted string (`instant[:10]`).

## Audit conclusion and limits

### Noninvasive CPU and actual heap evidence

Ten randomized, output-checked CPU sampler on/off pairs produced [27,230 program-counter samples](results/sprint-20261029-jsonl-pc-summary-10.json); [raw paired timings](results/sprint-20261029-jsonl-pc-audit-runs-10.json) and [raw samples](results/sprint-20261029-jsonl-pc-samples-10.tar.gz) are retained. The paired median sampler overhead was **22.467 ms**. Of all samples, **20,215** landed in the Guion binary and **7,014** in libc. Direct Guion locations include the VM dispatch function (3,837 samples), `host_op` (1,276), `Value` pointer writes (977), stack `pop` (806), `Value` drop (653), and `Value` clone (613). The optimized VM function also contains inlined work; these are direct program-counter samples, not inclusive times or savings estimates. Mapping libc addresses to exported allocator functions attributes **1,852 samples to malloc**, **1,509 to free**, and **61 to realloc**: at least **12.6% of all CPU samples** resided directly in those allocator functions. Other libc samples are not assigned to allocation.

Five randomized, output-checked [heap-counter on/off pairs](results/sprint-20261029-jsonl-heap-audit-runs-5.json) measured **28,295,730 malloc**, **5 calloc**, **790,147 realloc**, and **28,295,722 free** calls in each 200k-row process run. That is **29,085,882 allocation/reallocation API calls**, about **145.4 per input row**, including process startup. Malloc requested about **1.770 GB** in aggregate and realloc requested about **0.632 GB** in aggregate; these are requested-byte totals, not retained memory or physical page allocations. The paired median heap-counter overhead was **205.873 ms**. These are actual allocator API calls, unlike the guest budget charges above; the wrapper does not attribute each call to a source expression. The [CPU sampler](audit_tools/guion_pc_sampler.c), [line analyzer](audit_tools/analyze_guion_lines.py), [CPU driver](audit_tools/run_guion_pc_audit.py), [heap wrapper](audit_tools/guion_alloc_counter.c), and [heap driver](audit_tools/run_guion_alloc_audit.py) record the audit method. All audited outputs matched the frozen output SHA-256.

**Measured center of gravity:** the VM instruction body accounts for about 1.912 s of the 2.766 s diagnostic run; host operations account for about 0.773 s. Frame creation and driver bookkeeping together account for only about 0.080 s. The parity target requires about 1.917 s from the uninstrumented final median. A frame-pool or driver-only change is far too small. Host-only changes are also too small in this measured run. A successful closure likely needs a large reduction inside VM instruction/value execution, and may also need host savings. The diagnostic overhead prevents claiming exact production shares.

The sparse per-opcode timing experiment was **rejected as a time partition**: extrapolating only its sampled stack/control opcode category produced about 2.34 s, greater than the same run's entire 2.05 s measured VM guest body. Timer and RAII overhead dominated such short operations. Its output-checked [negative record](results/sprint-20261029-jsonl-opcode-sample-trials-5.json) is retained, but no opcode time estimate from it is used here. Likewise, the older 1-in-1024 frame timer is not a wall-time partition.

**Steps 1 and 2 are complete; step 3 research is recorded in the linked synthesis.** The roughly 1.9 s diagnostic VM instruction body has not been divided into exact exclusive per-opcode times; short-operation timers proved too intrusive. CPU sampling and actual allocator counts instead establish that VM value/stack work and heap traffic are substantial, while 196,241 JSON copy-on-write updates establish a concrete workload inefficiency. These observations warrant targeted research and controlled changes. None alone proves the size of a future speedup or a defect in Guion's core design.
