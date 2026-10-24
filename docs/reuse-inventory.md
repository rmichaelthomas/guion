# Prior language and prototype reuse inventory

Status: completed for the local repositories inspected on 2026-10-22. No source files in those repositories were modified.

This inventory answers whether Veritas should copy or adapt existing implementation code. It is a bounded source review, not an exhaustive audit of every historical repository or commit.

## Decisions

| Project/material | Evidence inspected | Decision for Veritas |
| --- | --- | --- |
| Planes general-purpose language | Local README, grammar/API and implementation layout, examples, and cross-implementation checks in `planes-rust-experiment` | Reuse engineering practices: publish grammar and machine-readable contracts, compare independent implementations against shared behavior cases, test with real programs, and make effects inspectable. Do not transplant the language grammar, exact rational semantics, provenance model, policy rules, or effect claims as Veritas semantics. No implementation code was copied. |
| Planes Horizon/browser experience | User's account and the Planes project context recorded in `north-star-design.md`; the full Horizon application was not present in the inspected Planes checkout | Retain as future workload evidence for persistent calls, host boundaries, output, deterministic logic and latency. Do not claim Horizon was independently re-run or use its app-specific snapshots/rendering contracts in the foundation. |
| Planes Rust spike | `planes-rust-experiment/rust/planes-spike/README.md` | Exclude as a Veritas runtime base: it describes a workload-specific invoice/record implementation that does not parse Planes source. It is useful as a model for parity-first benchmark hygiene only. No source code was copied. |
| Decan | Local README and CLI/package structure in `Documents/Codex/decan` | Reuse the practice of exact support matrices, stable errors, loss-aware conversion reports, and executable consumer evidence. Exclude temporal-intent semantics and scheduler/calendar behavior from the general-purpose language. No source code was copied. |
| Liminate-dev snapshot | Three router modules under `2026-10-16/continue-the-load-bearing-excavation-s-3/work/cloudflare-backward-channel/provenance/liminate-dev`; no README, language grammar, or package manifest was present in that snapshot | This snapshot is insufficient evidence for reusable Liminate language/runtime components. Keep the broader Liminate experience as design context only; audit the full monorepo separately if concrete components are proposed for reuse. No source code was copied. |

## Practices adopted in this build

- Treat examples as inputs to general compiler/runtime behavior; tests check behavior changes, malformed inputs and installed-product use.
- Keep human-readable docs, machine-readable reports and the implemented API in sync.
- Report static known host operations separately from opaque Python calls. The inspection report explicitly states that static bytecode reachability is not a sandbox or proof of foreign-code behavior.
- Record observed platform/tool versions, limits, commands and raw results. Separate author-run evidence from independent recommendation evidence.
- Keep a written compatibility boundary for each artifact, package and conversion surface.

## Result

There is no approved direct code reuse from Planes, Liminate or Decan in the current Veritas foundation. The useful inheritance is process and evidence discipline. Veritas is an independent general-purpose language implementation with its own specification and runtime contracts.
