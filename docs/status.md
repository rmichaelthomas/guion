# Guion status

**Stage:** private pre-release development. The product rename and current source-facing Guion surfaces are in the canonical private repository. No public release has been published, and the final pre-release validation is not complete.

## What is established

- The default core CLI embeds a self-hosted compiler and runs on the Rust runtime. It can create, check, format, lint, test, run, build, package, verify, install, inspect, and document Guion programs.
- The current language includes typed functions and records, modules, collections, `Option`/`Result`, JSON, bounded file and HTTP operations, SQLite/TOML services, Unicode helpers, and bounded parallel work. The [language contract](../language/spec.md) is authoritative about what syntax works and what remains unsupported.
- New source, artifact, manifest, lock, and local-state names are `.gu`, `.gub`, `guion.toml`, `guion.lock`, and `.guion/`. A limited set of old Veritas project names remains readable during migration; see [compatibility](compatibility-and-migrations.md).
- The previously validated release targets are macOS ARM64 and Debian 12-compatible GNU/Linux ARM64 with glibc. Guion still needs the planned full target matrix and release checks under the renamed product identity.

## Current evidence and its limits

- Task 4 produced 18 Guion-named application reports across nine retained regression applications and initial/follow-up variants. All 18 report `passed: true`. The preserved inputs and original Veritas reports were not rewritten.
- The complete installed-performance and builder records under `evaluation/completion/results/` belong to the earlier Veritas-named build. They contain matched measurements, raw samples, intervals, correctness checks, and unfavorable outcomes. They are useful historical evidence, but they are not a finished Guion-identity performance rerun. See the [evidence index](history/evidence-index.md) for the separate campaign identities and exact report links.
- The historical Veritas run found faster results on specified installed compute/data workloads. It also recorded ties or regressions in the broader suite, and the free-threaded Python comparison was unavailable. These results support only those named workloads and conditions; they do not show that Guion is faster than Python in general.
- The frozen independent recommendation campaign selected Python in all 18 choices for its older candidate. A later three-pair builder study recommended either language for two work classes and Veritas for one. These are separate studies with different questions; neither is a general adoption estimate.

## CI and release gates

GitHub Actions runs formatting, workspace tests, and schema checks on Ubuntu 24.04 x86_64. That job is a source check only; it does not certify Guion's supported release targets. The release gates remain separate and require macOS ARM64 and Debian 12-compatible GNU/Linux ARM64 with glibc. A target that did not run is reported as incomplete.

## Remaining pre-release work

Full Guion-identity validation, continuous integration, and the handoff report remain to be completed. The [evidence index](history/evidence-index.md) records the historical Veritas cohorts separately from Guion rename acceptance. Required macOS and Debian ARM64 checks must be reported individually; a skipped target cannot be marked green. See the [approved migration plan](superpowers/plans/2026-10-24-veritas-complete-product.md) for the complete task gates.

This status page is a current summary, not the test record. The [historical index](history/README.md) links to the original completion report, checkpoints, campaigns, and preserved results.
