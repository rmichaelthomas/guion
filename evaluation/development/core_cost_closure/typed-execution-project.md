# Typed execution model project (opened 2026-11-01)

Goal: close the remaining 22–35% Guion/Python gap on long JSONL loops (and the CSV ledger) by running verified bytecode through a typed native tier that avoids the VM's per-instruction dispatch and per-value plumbing, keeping the allocation quota, instruction limit, cancellation, error traces and output identical.

## Why this and not more VM levers
Measured on the 200k JSONL run (see `task-class-evidence.md`): dispatch plus the loop function is about 28% of samples, value plumbing (clone, drop, atomics, allocation, limit and quota checks) about 22%, real data work 5–6%. Every single VM lever measured 0–7%; stacked they reach about 10–15%. Removing all quota and limit work outright is worth at most 10%. Only a design that removes dispatch and most clone/drop traffic together can reach the gate.

## Baseline for the project (container, JSONL fused program, 200k rows)
VM 1.00 s; native bridge build 1.47 s; Python about 0.73–0.85 s. Per-row cost is flat from 60k to 180k rows (about 4.6 us), so the gap is steady-state, not scaling.

## Milestones and kill criteria
1. Native beats the VM on the JSONL loop: replace generic per-operation callbacks with typed, specialised entry points, make value access borrow-based, and charge statically known sizes inline. Kill if five measured steps leave native slower than the VM.
2. Native at or below Python on JSONL at 200k (paired, both lanes).
3. Extend to the CSV ledger and re-run the holdout and compute workloads; ship only with identical output and passing suites.

Each step is committed with its measurement. Failed steps are reverted and recorded.

## Milestone 0 findings (2026-11-01)
- Per-row cost is flat with input size (4.83, 4.57, 4.60 us at 60k, 120k, 180k rows; RSS 19, 35, 44 MB), so the 20k-versus-200k ratio difference is Python's startup amortisation, not a Guion scaling problem. The program fails with "cannot read events JSONL" at 240k rows (a file-size limit, not a speed issue).
- The compiled `main` is a stack-machine translation with heavy redundancy (`Load r; IsOk; Not; JumpIfFalse ...; Load r; Unwrap; Store s`, jumps to the next instruction, unreachable jumps after `Return`). `crates/runtime/examples/dump_ops.rs` prints a function's opcode stream from a `.gub` for this analysis.
- The fused-accessor program is not native-eligible (`JsonTryGetText`, `JsonTryGetInt`, `TimeFormatUtcDate` have no native lowering), so the native comparison so far is on the frozen program only.
- Native bridge profile after forwarder inlining stays flat: `native_charge_value_clone` 6.4%, `execute_native_operation` 4.7%, allocation 4.5%, atomics about 4.8%, direct-entry prologue 4.2%.
- A shallow `Return` limit check measured 0% on the frozen program and was reverted.

## Next: Milestone 1 design
A call-threaded baseline: pre-decode each function once into a vector of handler entries with operands resolved (slot numbers, constant references, jump targets, statically known clone charges), executed without the per-instruction `match`, bounds check and cancellation/fuel head, keeping the existing value stack, quota, fuel and error behaviour. Expected ceiling from the profile: the dispatch share (up to about 28%) minus handler-call cost. Kill if it measures under 8% on the JSONL loop.

## Milestone 1 result: call-threaded baseline killed by probe

The charter's ceiling for a call-threaded baseline is the per-instruction head
(cancellation loads, `ip` bounds check, fuel decrement). A probe build deleted
that head outright, which a real design cannot do because it must still honor
cancellation and fuel. On `base2.gu`/`jsonl.gu` at 200k rows, median of 3,
alternating: release 948 ms, no-head probe 907 ms, same output hash
`a1a9b95d`. The ceiling is 4.1%, below the 8% kill line, so the full refactor
was not built. The VM's remaining cost is in the handlers and value plumbing,
not in dispatch. The probe was reverted and not committed.

## Lever result: Arc payloads for Option/Result

`OptionSome`/`ResultOk`/`ResultErr` were switched from `Box<Value>` to
`Arc<Value>` across model, vm-core, runtime and tests (the quota charge was
left unchanged). 200k-row JSONL, alternating runs: baseline 960 ms, Arc build
994 and 1005 ms, same output hash `a1a9b95d`. The atomic refcount traffic on
clone and drop costs more than the `Box` malloc it avoids, so this is about 4%
slower. Reverted and not committed.

## Lever results: metrics allocation, and the remaining levers against a flat profile

- **JSON number metrics without `to_string`.** `json_metrics` and
  `shared_json_metrics` allocated a `String` per JSON number. Replacing that
  with an arithmetic digit count for `i64`/`u64` (floats and big numbers fall
  back to `to_string`) measured as noise on the 200k JSONL loop: baseline
  958/965/938 ms, candidate 963/947/951 ms, same hash `a1a9b95d`. Reverted.
- **Fresh VM profile (`guion-prof`, 200k JSONL, about 1000 samples).** No
  location holds more than 5%. The top entries are `drop_in_place` (5.0%),
  the instruction match (3.9%), thread-local access in the pool allocator
  (about 5.3% combined) and `lib.rs:4368` clone accounting (2.1%).
  `serde_json` parsing does not appear in the top 45 locations.
- **Consequences for the other levers.**
  - Lazy JSON parsing has no parse hotspot to remove, so it is not built.
  - Folding the metrics walk into the parse is bounded by the metrics result
    above, so it is not built.
  - `JumpIfTrue`/compaction is covered by the earlier instruction-pair
    fusion negative result (`Not`→`JumpIfFalse` and similar, all within noise).
  - Native lowering of `JsonTryGetText`/`JsonTryGetInt`/`TimeFormatUtcDate`
    would make the fused program native-eligible, but native is currently
    about 47% slower than the VM on the frozen program (1.47 s vs 1.00 s), so
    eligibility alone cannot close the gap.

## Milestone 2 (scoped 2026-11-01): typed register tier in vm-core

### Why this shape
Five levers in a row measured 0–7% because the cost is spread across the value
model, not concentrated. Facts that shape the design:
- `main` in `base2.gu` is one 709-op function holding the loop. By static count
  53% of its ops are pure stack traffic (165 `Load`, 123 `Const`, 87 `Store`),
  plus 42 `Jump` and 18 `Not` that the compiler emits mechanically. `jsonl_next`
  is 14 `Load` of 62 ops. Every one of these clones or drops a `Value` and
  charges the quota.
- The verifier already computes a `FlowState` (stack types and
  `Vec<Option<Type>>` local types) at every instruction, so type facts exist at
  load time with no compiler or artifact-format change.
- Dispatch itself is only about 4% (Milestone 1 probe), so the win has to come
  from executing fewer, cheaper operations, not from dispatching faster.

### Design
Lower verified bytecode at `Runtime::load`, next to `inline_host_forwarders`,
into a register form per function. Artifact format 50 is unchanged. The new
code lives in a new module (`crates/vm-core/src/register.rs`), not in the
7k-line `lib.rs`.
- **Eligibility is per function.** A function runs in the register tier only if
  every op in it is supported; otherwise it keeps the current path. Both tiers
  share `VmFrame` (same `locals`), so calls between them and error traces keep
  working. This lets support grow op by op.
- **Exact accounting is kept, not approximated.**
  - Fuel is charged per basic block with the exact count of original
    instructions in it, so `steps_used` totals are identical. If the remaining
    fuel is smaller than a block's cost, that block runs through the original
    bytecode path so `R_STEP_LIMIT` fires at the same instruction with the same
    span.
  - Every register op keeps the span of the instruction it came from.
  - Quota charges are never dropped. Statically known amounts are folded into
    one charge per block; amounts that depend on the value stay dynamic.
- **Stage A, register lowering.** Stack to registers, then copy propagation,
  dead-store elimination, jump threading, removal of unreachable code, and
  fusion of `Load;IsOk;Not;JumpIfFalse` style sequences. Registers are extra
  slots after the declared locals.
- **Stage B, typed slots.** Using `FlowState`, slots proven `Int`, `Bool` or
  `Text` hold unboxed values: `i64` with checked arithmetic that deopts to
  `BigInt` on overflow, `bool`, `Arc<str>`. This removes tag checks and
  statically known clone charges.
- **Stage C, moves by liveness.** A `Load` of a slot that is dead afterwards
  becomes a move, so `Json`, `Record` and `Map` values stop paying clone and
  drop atomics. No allocation charge is skipped for anything that actually
  allocates.
- **Stage D, native consumption, only if needed.** The register IR is the input
  Cranelift would have wanted all along (typed, no stack). Attempted only if
  A–C leave the VM above Python.

### One behavior change that needs sign-off
Cancellation is checked per instruction today. In the register tier it is
checked at each basic-block head, so a block of N original instructions can
run to completion after cancellation. Blocks contain no loops (back-edges end
blocks), so the delay is bounded by block length. Parallel-worker budgets keep
exact atomic fuel charging as they do now.

### Gates and kill lines (each stage is committed with its measurement)
- **2.0 Trace analysis, no executor.** From the real 200k-row run, measure the
  dynamic count of removable instructions (copy propagation, dead stores,
  threaded jumps) and the dynamic type mix of `Load`/`Store`. Go only if at
  least 35% of dynamic instructions are removable. This is a day-scale
  analysis and prevents building an executor on a wrong estimate.
- **2.A** Kill if the JSONL loop improves by under 6% (target 8–12%).
- **2.B** Kill if it adds under 5% on top of 2.A (target about 10%).
- **2.C** Kill if it adds under 3% (target 3–5%).
- Stacked target: 20–25% against the 22–30% gap. My estimate that A–C reaches
  an upper 95% ratio below 1.0 in all four lanes is about 35–40%, so the plan
  includes Stage D rather than assuming A–C suffice.
- **Every stage:** same output hash (`a1a9b95d` for the JSONL loop), full
  workspace tests including conformance and the xtask inventory, the
  limit/cancellation/trace tests unchanged, then a paired measurement at 20k
  and 200k in both lanes. CSV ledger, holdout and compute workloads are
  re-run at the end of A and of C; any regression there stops the stage.

### Out of scope
Changing the language, the artifact format, quota semantics, or the
self-hosted compiler; benchmark-specific special cases; re-enabling the
per-op native bridge by default.

## Milestone 2 results (2026-11-01)

### Stage 2.0, trace analysis: go
A temporary probe counted every executed `main` instruction in the 200k-row
frozen run, including the ones the VM's existing fused `Load` handler skips
(44.1M dynamic instructions, 220 per row, output hash `a1a9b95d`).
- Removable by operand folding, store-destination fusion, jump threading and
  `Not;JumpIfFalse` fusion: 54.7% of dynamic instructions.
- The VM already fuses `Load` with a following `IsOk`/`IsSome`/`Unwrap`/
  `GetField`/`Index`, which covers 9.5M of the 16.0M removable `Load`/`Const`
  executions. Excluding those, the genuinely removable share is about 40% of
  real dispatches, still above the 35% bar. The cheap instructions (`Const`,
  `Store`, `Not`, `Jump`) are the ones left.

### Stage A, as built: fused entries instead of a register IR
A register form needed a second interpreter or new `Op` variants across the
model, verifier, native backend and wasm crates. Fused entries reach the same
instructions with far less surface:
- A load-time table (`build_fused_tables`, `VmHost::fused_tables`) indexed by
  original `ip`, so `ip`, spans, jump targets and frame state keep their
  meaning and the artifact format is unchanged.
- Four entries: `Load;IsOk|IsSome;[Not];JumpIfFalse`, `Not;JumpIfFalse`,
  `Load;Unwrap;Store`, `Const;Store`. Each charges exactly the fuel and
  allocation the plain sequence charges and falls back to the plain
  instruction when fuel is short or a precondition fails, so `R_STEP_LIMIT`
  fires at the same instruction. Disabled when profiling and under exact
  atomic fuel (parallel workers). No entry spans a jump target.
- `fused_entries_match_plain_execution_at_every_instruction_limit` sweeps the
  instruction limit from 0 to 39 and requires the same result, error code,
  trace and remaining fuel with and without fusion.
- Cancellation is checked at the head of a fused entry rather than before each
  covered instruction (the sign-off item above); entries contain no loops.

Measured (alternating A/B, 5 rounds, median of 3 per round; the baseline's own
spread across runs is about 1.5%):

| Workload | Before | After | Delta |
| --- | ---: | ---: | ---: |
| JSONL 200k | 924 ms | 905 ms | -2.1% |
| CSV ledger | 433 ms | 433 ms | 0.0% |
| Text | 4723 ms | 4735 ms | +0.3% |
| Int compute | 2505 ms | 2531 ms | +1.0% |

Output hashes are unchanged (`a1a9b95d` JSONL, `9eafdec9` CSV). The fusions
removed 14.9% of dynamic instructions (6.57M of 44.1M) for about 2% of time.
A first version with the handler inline in the dispatch function slowed the
int workload by 2.2%; moving it into an out-of-line function removed that.

**Verdict: Stage A missed its 6% kill line (2.1%).** The code stays because it
is exact, tested and not slower anywhere beyond noise, but the stage is closed.
The remaining sequences are scattered: the largest un-fused 2-grams are
`Store;Load` (3.6M, already measured as noise), `Load;Const` (3.1M),
`Call;Store` (2.0M), each under 8% of dispatches and worth about 0.5% apiece.

### Stage B, typed slots: not built
Gated on adding at least 5% over Stage A, which it cannot. Small `BigInt`
values are already stored inline as `i64` (two words), so unboxing saves tag
checks only. The arithmetic and comparison ops are about 8% of dynamic
instructions, so a one-third cost cut is about 1%.

### Stage C, moves by liveness: not built
A liveness pass over `main` shows 4.0M of 12.1M dynamic `Load`s (33%) are last
uses of their slot. Two things cap the value:
- `MapSet` and `MapRecordSet` already peek at the following `Store(slot)` and
  take the value out of the local, so the `Load m; MapSet; Store m` pattern
  already mutates in place.
- A generic move would break those handlers (they expect the slot to still
  hold the map), and mutation charges depend on `Arc::strong_count`, so a
  move would change quota charges.
- All atomic operations together are 3.0% of the profile, and moves can only
  touch a share of them. Ceiling about 1–2%, under the 3% kill line.

### Stage D, native consumption: not attempted
The condition was that A–C leave the VM above Python. They do, but native is
about 47% slower than the VM on the frozen program and the native profile is
flat, so a typed register IR is not the blocker native needs fixed first.

### Paired gate after Stage A (container, PGO CPython 3.14.7, 9 trials)
Guion/Python ratio, median / upper 95%, build `d1ad443`:

| Rows | Program | Lane | Ratio |
| ---: | --- | --- | ---: |
| 20k | frozen | source | 1.220 / 1.274 |
| 20k | frozen | artifact | 1.087 / 1.137 |
| 20k | fused | source | 1.171 / 1.215 |
| 20k | fused | artifact | 1.035 / 1.080 |
| 200k | frozen | source | 1.383 / 1.391 |
| 200k | frozen | artifact | 1.357 / 1.383 |
| 200k | fused | source | 1.250 / 1.287 |
| 200k | fused | artifact | 1.260 / 1.275 |

Against the pre-Stage-A paired run (`jsonl-forwarder-inlined-paired-9.txt`) the
200k fused-source lane improved (1.325 to 1.250) and the others are within the
run-to-run spread (the 20k fused-artifact median moved 0.995 to 1.035 on a
0.106 s Python baseline). **The gate is not met in any of the four lanes at
200k.** The best upper 95% bound is 1.275.

## Value-model ceiling probe: allocation charging removed

Probe (reverted, never committed): `Budget::allocate` always succeeds and
`refund_allocation` does nothing, so every charge calculation and atomic
operation on the allocation budget disappears. This breaks quota semantics and
is an upper bound for any value model that avoids clone-and-charge, not a
candidate. Alternating A/B, 5 rounds, medians in ms, against `d1ad443`:

| Workload | HEAD | No charging | Delta |
| --- | ---: | ---: | ---: |
| int compute | 2506 | 2406 | -4.0% |
| text | 4648 | 4428 | -4.7% |
| CSV | 421 | 412 | -2.1% |
| JSONL | 888 | 853 | -3.9% |

Closing the 200k gap needs roughly 20% (ratio 1.25 to below 1.0). The entire
charging machinery is worth about 4%, so a typed value model that only removes
charge-and-clone cannot reach the gate on its own.
