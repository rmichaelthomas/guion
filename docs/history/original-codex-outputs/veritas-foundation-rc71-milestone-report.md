# Veritas foundation RC71 — milestone report

**Built:** 2026-10-22, macOS arm64  
**Release:** `veritas 0.1.0`, artifact format 22  
**Status:** foundation release candidate built and verified; recommendation evidence remains open.

## What changed in this continuation

- Added `veritas inspect` capability reporting in both human and JSON form. It follows calls and `parallel_map` worker functions, and reports file, network, output, argument, concurrency and opaque Python effects. The report says explicitly that it is static bytecode reachability, not a sandbox or proof of foreign-code behavior.
- Completed a bounded reuse inventory for the locally available Planes, Decan and Liminate-dev materials. We adopted evidence practices and excluded domain-specific semantics. No source code was copied, and the original repositories and uploads were left untouched.
- Repeated the persistent NumPy sidecar stress test against an extracted package and recorded its raw report. Correctness, close behavior and post-close failure passed. Memory stayed stable during the measured steady portion of this run, while the stable sidecar RSS differed from the earlier RC69 run; see the raw data and the checkpoint for both observations.

## Verification

- `cargo fmt --all --check`: passed.
- `cargo test --workspace`: 126 passed, 0 failed, including the CLI, compiler, runtime, Python bridge, HTTP, managed-dependency and native-wheel relocation checks.
- Distribution build: installer/relocation checks passed; 4 Python distribution checks passed; task-ledger and Python interop bundles packaged and verified; the installed launcher produced identical output on repeated runs.
- Installed acceptance kit from the extracted RC71 package: 34 passed, 0 failed. Loopback tests passed. Python wheel installation ran offline using the existing local cache containing the exact locked wheel payloads.
- The packaged `veritas inspect --json` was run against a shipped source example and reported the expected transitive file read/write and output effects.
- The signed archive validator accepted all 5,653 entries. The package archive SHA-256 is `6e9adbcc8debcd7de546f2a0d6fc646d274022dd201c1e974bf86f14a55f4b6d`.

The first installed-kit attempt pointed `UV_CACHE_DIR` at the package's NumPy/orjson cache, which did not contain the feedparser wheel payloads used by the other examples. Those cases failed offline as expected. Rerunning with the local cache containing the exact locked feedparser wheels passed all 34 checks; this setup correction is recorded in the machine-readable validation report.

## What this establishes

The current macOS arm64 foundation can compile and run ordinary Veritas programs, report diagnostics, use bounded host capabilities, make typed calls into managed Python, run the tested concurrent/network workloads, and produce and verify relocatable packages. The new inspection surface helps builders and assistants see known runtime-mediated operations and foreign-code uncertainty before running a program.

## What remains open

RC71 does **not** establish that an assistant should recommend Veritas over Python for general work. The evidence is authored by the project owner and this build session; there is no independent unseen-task comparison. Other target systems, broad editor integration, ecosystem maturity, arbitrary Python object compatibility, native performance advantages, and security confinement are also unproven. Static inspection reports declared/reachable bytecode operations only; Python/native/OS behavior can go beyond that surface.

This is a foundation candidate for the stated north star, not the north-star result. The next evidence milestone should freeze this package and run equal, unseen briefs across multiple task classes with Veritas and well-tooled Python, including changed requirements, setup costs and clean deployment. Do not make a default recommendation until that evidence supports it.

## Files

- Installable package: `veritas-foundation-rc71/veritas-0.1.0-darwin-arm64.tar.gz`
- Source archive: `veritas-foundation-source-rc71.tar.gz`
- Machine-readable validation: `veritas-foundation-rc71-release-validation.json`
- Raw repeated lifecycle report: `veritas-foundation-rc71-lifecycle-stress.json`
