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
