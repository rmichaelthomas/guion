# Guion Runtime Gap Follow-up Sprint

> Implementation stays local in `codex/guion-core-sprint-spec`. This sprint continues only from measured evidence; it does not authorize an outside evaluator, outside work, PR, push, merge, or commit.

## Goal

Close the remaining JSONL execution gap against pinned Python and resolve the small typed-HTTP source penalty at the phase that causes it, while preserving Guion's north-star goals: useful agent-facing programs, clear diagnostics, compatible artifacts, and enforceable runtime boundaries.

## Current diagnosis

### JSONL

- After the retained runtime and source-level improvements, the unchanged 200k-row workload remains roughly 3.8× slower than pinned Python; 20k remains roughly 3× slower.
- A representative current profile reports about 61.5M interpreted bytecode instructions, millions of VM frames/calls, and no native-compiled functions for the mixed JSONL application path. `Load`, `Store`, conditional branches, calls, and result/option operations dominate instruction counts.
- JSON parsing and individual host operations are measurable but do not explain most end-to-end time. Map copy-on-write and allocation accounting have already been isolated; local opcode fusions, reservation, and the mixed-Value per-host-op bridge did not clear paired speed gates.
- The native bridge reduced executed VM instructions but lost end-to-end time because callback/conversion costs outweighed dispatch savings. Do not re-enable it by default or expand it operation by operation without a new profile and a causal test.

### HTTP

- Direct checked response-bytes JSON parsing is materially faster than legacy for 1 MiB payloads.
- The 64 KiB cold-source penalty traced to `std:http -> std:types -> std:time`. A shared `std:http_types` module and explicit `pub import` exports remove that transitive graph for HTTP-only programs while preserving `std:types` imports.
- A fresh 15-pair run now measures 64 KiB typed/legacy at 0.929 source (95% interval 0.913–0.974) and 0.886 artifact (0.822–0.931), inside the 2% non-inferiority bound. At 1 MiB, source is 0.761 (0.678–0.788) and artifact 0.716 (0.705–0.795). The cached-source 64 KiB lane is 1.023 (0.962–1.052), near parity but not part of the stated cold-source/artifact acceptance gate.
- The HTTP acceptance gate is met. Preserve the bytes parser and shared types module; keep the result distinct from the still-open JSONL goal. Raw report: `evaluation/development/core_cost_closure/results/http-shared-types-paired-15.json`.

## Historical basis

Adaptive specialization can improve an interpreter without a JIT and should deopt cheaply when assumptions stop holding ([PEP 659](https://peps.python.org/pep-0659/)). Cross-language conclusions need a range of workloads over equivalent core abstractions ([Are We Fast Yet?](https://doi.org/10.1145/2989225.2989232)). A low-startup baseline tier can bridge interpreter performance and an optimizing JIT ([Deegen](https://arxiv.org/abs/2411.11469)). These are design precedents, not Guion performance promises.

## Constraints

- Reuse the current worktree and already-running benchmark container; do not start, stop, restart, or prune Docker/Colima.
- Keep Python3 as the only Python command. Preserve all existing local changes and do not edit any project-level `AGENTS.md`.
- Keep the core Python-free. No benchmark-specific summary operation, dropped JSON validation, weaker allocation/fuel/cancellation behavior, or hidden permission changes.
- Keep artifact format unchanged unless a new serialized opcode is demonstrably required. Prefer an ephemeral execution plan or internal tier with the existing verified artifact as the source of truth.
- Run builds, tests, and performance cohorts sequentially. Retain failed candidate reports, remove rejected code, and keep the worktree uncommitted.

## Work

### 1. Freeze the present state and improve causal attribution

- Rebuild the current default-off candidate and pinned baseline in the same container with identical release settings. Verify hashes and benchmark input manifests.
- Add a reproducible, opt-in native eligibility report that names each application function, whether it compiled, and the first reason it did not. Keep profiling clocks out of default execution.
- Capture a fresh uninstrumented 15-pair JSONL cohort at 20k and 200k and a profile-only cohort. Separate parse, host callback, frame/call, and VM instruction work; report actual memory separately from logical allocation-budget charges.
- Confirm whether the mixed workload's hottest path is a loop/call region that can remain in one execution tier. Do not infer a hardware branch-misprediction problem from opcode counts.

### 2. Test one stable hot-region execution tier

- Build a narrow prototype from one real hot function/loop in the unchanged JSONL application plus a small differential fixture. Use the existing verified bytecode and types to select the region.
- Lower common operations and internal calls directly into one stable native region. Do not call back through a generic mixed-Value trampoline for every opcode. On an unsupported operation or value, side-exit at the exact instruction to the VM and resume with equivalent stack, locals, source span, budget, and cancellation state.
- Keep the current VM as the fallback and the existing native numeric backend unchanged. Do not expand the current per-operation mixed-Value bridge.
- Verify exact output, error trace, capability policy, instruction exhaustion, shared allocation quota, cancellation, and VM/native differential behavior before timing.
- Run a 3-pair smoke, then at least 15 randomized paired trials for both sizes if the smoke is clean. Retain the region only if the paired interval demonstrates a meaningful end-to-end gain in both source and artifact lanes without an unexplained peak-RSS increase. Keep Python parity as the ultimate open gate; a local gain alone is not closure.
- If the stable-region prototype cannot demonstrate a clear gain, stop that implementation path and compare a register-based/predecoded interpreter against baseline-tier compilation with a small throwaway experiment. Do not stack uncertain optimizations.

### 3. Close the cold typed-HTTP compile phase — complete

- Repeated same-build 64 KiB source, cached-source, artifact, and 1 MiB controls in the 15-pair report above.
- Per-module attribution confirmed the typed graph loaded unrelated `std:types` and `std:time` modules. The shared type module and explicit public re-exports removed those dependencies from HTTP-only programs.
- Compiler, self-host, compatibility, benchmark-harness, and normal CLI execution checks passed. Output equivalence held across the payload matrix. The 64 KiB cold-source and artifact confidence intervals meet the stated 2% non-inferiority bound; the 1 MiB candidate remains faster than same-build legacy.

### 4. Re-measure against the north star

- Repeat the unchanged JSONL 20k and 200k benchmark and HTTP payload matrix on the final candidate. Report source, cached source, uncached source, artifact, cold compile, execution time, intervals, peak RSS, and correctness separately.
- Keep both required performance gates open until their intervals pass. Update the README/status and agent-facing docs with the observed supported boundary and remaining gap.
- Preserve raw manifests/reports and candidate history. Review the final diff and worktree state; do not commit unless asked.

## Acceptance gates

- JSONL output is identical, behavior/policy/resource tests pass, and both 20k and 200k candidate/Python 95% upper ratio bounds are below 1.0 in source and artifact modes.
- Typed HTTP 64 KiB source and artifact are within the agreed 2% non-inferiority margin against same-build legacy, while the direct bytes path remains faster than the before-build typed path and does not regress 1 MiB behavior. **Met** in `http-shared-types-paired-15.json` (15 paired trials; all output checks passed).
- No speed claim relies on a single sample, an instrumented binary, a changed workload, or a mismatched executable. A correctness pass and a speed pass remain distinct results.

## Stop conditions

- No external evaluator or approval step is a prerequisite to local implementation.
- Do not keep an optimization whose 15-pair interval includes parity and whose mechanism adds complexity without independently demonstrated value.
- If the follow-up tier work misses parity, report the remaining 2×+ JSONL distance plainly and recommend the next architecture investment; do not claim this sprint closed the north-star gap.
