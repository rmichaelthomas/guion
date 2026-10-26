# Richards runtime cost profile

## Intent

Use the Richards scheduler workload to determine whether its remaining runtime cost is driven mainly by list copy/allocation work or by control-flow and native helper overhead. Separate one-time source/JIT compilation from repeated execution, then make one small runtime change supported by the profile.

## Goals

- Record untimed runtime diagnostics for estimated guest allocation charges and bytes, list updates and copy-on-write copies, record charges, and dynamically executed guest conditional branches.
- Distinguish dynamic guest branch counts from hardware branch-misprediction counters; this pass does not claim to measure branch misses.
- Compare an uncached one-shot Richards source run with a repeated in-process Richards source run, with output checked against the canonical expected counts on every timed sample.
- Retain compiler phase data and counters in one compact repository-local report; keep binaries and generated source under ignored `target/` scratch and remove them at completion.
- Optimize only a cost supported by the measurements and preserve list aliasing, runtime budgets, and language behavior.

## Measurement rules

- Build the release CLI once with locked, offline dependencies and one Cargo target directory in the local Guion repo.
- Use the existing performance runner's timing and output-checking utilities. Keep diagnostic and timing runs separate so instrumentation does not contaminate elapsed-time comparisons.
- Profile the original one-shot program and a warm source program that executes the Richards workload 25 times in one process. Report native compilation phases separately from total program execution.
- Use 15 randomized samples for the one-shot and repeated-source lanes. Label cache conditions accurately: `--no-cache` bypasses Guion's compile cache but does not clear the OS file cache.
- Profile counters are opt-in and must have no counter-update work in ordinary invocations.
- Keep reports in `evaluation/completion/results/runtime-efficiency/`; keep all transient files in `target/perf-session/` and clean that directory after verification.

## Success criteria

- Instrumented and uninstrumented Richards runs both produce `9297` and `23246` for each logical execution.
- The report identifies estimated allocation volume, list copy count/bytes, in-place list updates, record charge volume, dynamic conditional branch count, native compilation time, and one-shot versus repeated runtime.
- Counter names and documentation do not imply physical heap allocation counts or hardware branch-miss counts where the runtime only records budget charges or guest branch executions.
- One measured optimization improves its stated target without changing output, aliasing behavior, runtime limits, or the existing short-source speed gates.
- Relevant Rust tests, performance harness tests, formatting, and focused benchmark checks pass; scratch files are removed and the branch is ready for a PR against `main`.

## Boundaries

- The result is specific to the Richards implementation, release CLI, macOS ARM64, and pinned benchmark environment; it is not a general Python comparison.
- No hardware performance-counter or Instruments run is required. This pass will not create files outside the Guion repository, install tools, alter permissions, or change the language's semantics to improve one benchmark.
