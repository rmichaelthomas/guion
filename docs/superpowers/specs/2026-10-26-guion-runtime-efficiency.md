# Guion runtime efficiency at useful scale

## Intent

Extend the recent first-source speed win to larger, more agent-shaped work: concurrent CPU tasks and JSONL processing. Measure fresh compilation, cached source launches, artifact runs, phase time, and memory separately so the next runtime/compiler optimization follows evidence.

## Goals

- Rerun all six established comparisons on the merged Rust-default CLI: Richards, N-body, Mandelbrot, parallel CPU at 20,000 and 200,000 items, and the 20,000-row JSONL summary application.
- Add a 200,000-row JSONL scale point to show how elapsed time and memory change as input grows.
- For every applicable workload, retain fresh-process uncached source, repeated cached source, artifact, and matched Python measurements. Preserve the Python serial/thread/process comparisons for parallel CPU and Python/SQLite comparisons for JSONL.
- Collect compiler/runtime phase telemetry outside timed lanes, plus peak RSS with the measurement method and scope clearly stated.
- Optimize a bottleneck identified in the baseline data. Keep the five short first-source workloads within their existing speed gates and preserve output correctness.
- Update the README and current status/readiness records so Python comparisons match the merged compiler and current evidence.

## Measurement rules

- Use pinned Python 3.14.7 on the available macOS ARM64 host and check every timed result against an independent expected output.
- Use the existing repository-local performance session, preflight, report format, and one Cargo target directory. Build with locked offline dependencies and no incremental compilation.
- Run at least seven randomized paired samples for each new uncached-source comparison and retain the established 15 compute / 16 data trial counts where the existing suite defines them.
- Treat OS file-cache conditions as uncontrolled. A fresh Guion process with `--no-cache` means the Guion compilation cache is bypassed; it does not mean the operating system file cache is cold.
- Profile separately from timed lanes using existing `GUION_PHASE_TIMINGS` fields. Preserve total process elapsed time alongside internal phase durations.
- Validate `resource.getrusage` sampling before relying on it. Report its units and process scope. For Python process-pool work, report per-worker high-water RSS and its sum as an upper bound; do not call that an exact simultaneous process-tree peak.
- Keep raw reports compact and under `evaluation/completion/results/runtime-efficiency/`. Keep compiler artifacts, generated input, and temporary projects under `target/perf-session/` and remove that scratch tree after the report is retained.

## Success criteria

- All outputs match for all six established workloads and the 200,000-row JSONL scale point.
- The merged compiler’s five short no-cache gates still pass.
- The report shows source, artifact, Python, phase, and memory evidence for the named tasks, including uncertainty and cache conditions.
- At least one measured runtime/compiler bottleneck receives a tested optimization with a reproducible improvement in its target metric and no regression in correctness or the five short gates. If the measurements do not support a safe optimization, record the evidence and the remaining gap rather than making an unmeasured change.
- README, status, and readiness claims agree with the retained evidence and remain scoped to the tested tasks and host.

## Boundaries

- This pass does not claim Guion is universally faster than Python or measure genuinely cold disk access.
- The Python process-pool RSS sum is a conservative per-worker high-water estimate, not a synchronized whole-tree peak.
- Do not install benchmark packages, request elevated system sampling, weaken runtime limits, publish a release, or edit project-level `AGENTS.md`.
