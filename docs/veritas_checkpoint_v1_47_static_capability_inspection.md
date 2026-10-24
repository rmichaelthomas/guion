# Veritas checkpoint v1.47 — static capability inspection

## Change

`veritas inspect` now reports statically reachable host effects through ordinary function calls and `parallel_map` worker functions. It identifies output, argument access, file reads/writes, HTTP requests, concurrency and opaque Python calls. The JSON form is schema version 1 and includes per-function direct/transitive effects, calls, entry reachability, Python declarations, and an explicit boundary statement. The human form shows the same per-function effects.

This is bytecode reachability analysis. It does not confine execution, prove Python declarations truthful, reveal native or operating-system effects, or guarantee that an effect is safe. Current effects are collected from known bytecode host operations; the report's `analysis_scope` describes its limits.

## Prior-project inventory

Added `docs/reuse-inventory.md`. The locally inspected Planes general-purpose language and its Rust workload spike, Decan, and the available Liminate-dev snapshot are documented with evidence and transfer/exclusion decisions. No source code was copied; original repositories remain unchanged. The Liminate inventory is explicitly bounded because the available snapshot contains only three router modules and no language/package documentation.

## Verification

The installed CLI integration test checks transitive effects through a helper, opaque Python labeling, schema identity and the human-readable inspection output. Full workspace and installed-package results are recorded with the RC71 milestone report.
