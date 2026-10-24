# Veritas Foundation Checkpoint v1.7 — Runtime Allocation Budgets

**Date:** 2026-10-22  
**Status:** Approved integrated foundation build in progress; implementation checkpoint, not a recommendation verdict.  
**Continues:** [v1.6 Option and Result matching](veritas_checkpoint_v1_6_option_result_matching.md), [v1.5 module graph](veritas_checkpoint_v1_5_module_graph.md), and the [inception checkpoint](../../../outputs/veritas_inception_checkpoint_v1_0.md), [north-star design](../../../outputs/veritas_north_star_build_design_v0_2.md), and [approved implementation plan](../../../outputs/veritas_foundation_implementation_plan_v0_2.md).  
**Build root:** `work/veritas-foundation/`. Source language repositories, NowLookHere, and starter uploads remain read-only.

## North star

Veritas is meant to become a general-purpose language and toolchain that an assistant would recommend over Python for suitable work. That remains an unproven product hypothesis. Evidence must eventually show better creation, repair, testing, execution, and delivery across ordinary projects, with clear cases where Python remains the better choice.

“Future-forward-but-back” means anticipating how assistant-mediated development may work in the future, then implementing its trust, repair, reproducibility, observability, and deployment needs now. Rust remains only the current implementation language.

## What this milestone adds

Each `Instance.invoke` now creates a shared, cumulative estimated-allocation budget. The default is 268435456 bytes (256 MiB), and exhaustion returns `R_ALLOCATION_LIMIT` with the current source location. The counter covers host-supplied argument values; VM frame locals and operand-stack capacity growth; heap-owned literal loads and copies; integer/text results; list, map, and record construction; map updates and key lists; JSON values; file and Python results; print rendering; and parallel work.

Nested function calls use the same counter. Parallel workers share one atomic allocation counter. A worker group can use at most three quarters of the allowance that is available when it starts, leaving the caller enough headroom to receive and inspect its result. Each later top-level invocation gets a fresh allocation budget. Output, instruction work, call depth, integer digits, text bytes, collection item counts, file bytes, source size, artifact size, and Python frames retain their separate ceilings.

Projects may set positive integer resource ceilings in `[runtime]` in `veritas.toml`; the CLI applies them to `run` and discovered tests. The supported names are `instruction_limit`, `allocation_limit_bytes`, `call_depth_limit`, `output_limit_bytes`, `max_integer_digits`, `max_text_bytes`, `max_collection_items`, `max_parallelism`, and `max_file_bytes`. Unknown fields and non-positive or out-of-range values fail clearly. Embedders can set equivalent values in `HostPolicy`.

Example:

```toml
[runtime]
instruction_limit = 2000000
allocation_limit_bytes = 268435456
call_depth_limit = 128
output_limit_bytes = 1000000
max_integer_digits = 4096
max_text_bytes = 16777216
max_collection_items = 1000000
max_parallelism = 8
max_file_bytes = 16777216
```

The VM now renders `print` output through an output-bounded writer, so a single printed value cannot first create a string larger than the remaining output allowance and only then be rejected.

## Verification evidence

- Test-first regressions exposed the missing allocation field, aggregate use across nested calls, aggregate use across parallel workers, unaccounted operand-stack growth, unaccounted print rendering, and the CLI ignoring project runtime limits.
- Runtime tests confirm repeated temporary text allocations exhaust the limit, a subsequent invocation starts with a fresh budget, worker allocations share their allowance and return `TaskError(R_ALLOCATION_LIMIT)`, stack growth is charged, and print rendering consumes the budget.
- A CLI integration test sets `allocation_limit_bytes = 1024` in a project manifest and observes `R_ALLOCATION_LIMIT` from the ordinary `veritas run` path.
- The known sales-report development workload still matches its independent Python standard-library reference (`12`, `1524`) after these runtime changes; the machine-readable run is `evaluation/results/development-sales-report.json`. This remains a fixed known case, not independent recommendation evidence.
- The offline workspace suite with pinned uv/CPython integration passed **74 Rust tests**. `cargo fmt --all -- --check`, Clippy with warnings denied, and `python3 -m unittest discover -s evaluation/tests -v` passed; the Python harness contains **2 tests**.
- The RC24 archive, adjacent checksum sidecar, extracted `doctor` report, and runtime-limit manifest smoke are delivered alongside this checkpoint.
- A source archive containing the workspace, examples, docs, and evaluation kit was extracted to a fresh temporary directory; the complete offline Rust workspace suite passed there as well, including the pinned managed Python integration. Its adjacent checksum sidecar is part of the output.

This is useful enforcement, not exact process-memory accounting. Charges estimate language-owned allocations at known creation points. Parsing and native library work can allocate before a charge is applied; allocator metadata, Rust runtime structures outside those points, the OS, and third-party processes are not measured. It is neither an RSS cap nor an operating-system sandbox. More complete accounting and stress evidence remain necessary.

## Remaining work and constraints

The full resource-accounting plan remains open: establish memory measurements and stress cases, cover all host/bridge/formatter/compiler allocation paths with explicit budgets, make temporary host allocations predictable, and compare estimated charges with observed peak and long-session memory. The project manifest currently exposes resource ceilings to `run` and tests; more lifecycle commands and build metadata should report the effective policy. HTTP/bytes, empty maps, selective imports, syntax-aware formatting, broader project lifecycle, multi-platform/relocatable packaging, and independent comparative evaluation also remain.

The release candidate is an unsigned macOS ARM64 evaluation bundle. This checkpoint does not establish a clean-machine or cross-platform installation, broad performance leadership, or a recommendation advantage over Python.

## Resume point

Continue the approved integrated plan in `work/veritas-foundation/`, following `docs/implementation-plan.md` and `.superpowers/sdd/implementation-plan/progress.md`. Next, extend resource-policy visibility into build artifacts and diagnostics, then advance host APIs and independent project acceptance while keeping the measured limits distinct from exact operating-system memory enforcement.
