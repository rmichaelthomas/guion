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
