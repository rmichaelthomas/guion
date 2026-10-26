# Guion status

**Stage:** private pre-release candidate. The Guion rename, source-facing product surfaces, documentation, and validation records are in the canonical private repository. The declared validation matrix has passed on macOS ARM64 and Debian 12-compatible GNU/Linux ARM64 with glibc. No public release has been published.

## What is established

- The default core CLI embeds a self-hosted compiler and runs on the Rust runtime. It can create, check, format, lint, test, run, build, package, verify, install, inspect, and document Guion programs.
- The current language includes typed functions and records, modules, collections, `Option`/`Result`, JSON, bounded file and HTTP operations, SQLite/TOML services, Unicode helpers, and bounded parallel work. The [language contract](../language/spec.md) is authoritative about what syntax works and what remains unsupported.
- New source, artifact, manifest, lock, and local-state names are `.gu`, `.gub`, `guion.toml`, `guion.lock`, and `.guion/`. A limited set of old Veritas project names remains readable during migration; see [compatibility](compatibility-and-migrations.md).
- The Guion-identity pre-release matrix passed on macOS ARM64 and Debian 12-compatible GNU/Linux ARM64 with glibc. This does not establish support for other operating systems, CPU architectures, or libc variants.

## Current evidence and its limits

- Task 4 produced 18 Guion-named application reports across nine retained regression applications and initial/follow-up variants. All 18 report `passed: true`. The preserved inputs and original Veritas reports were not rewritten.
- The final Guion installed-performance report is [`performance-guion.json`](../evaluation/completion/results/guion-final/performance-guion.json), with command, correctness, and comparison records in [`performance.json`](../evaluation/completion/results/guion-final/performance.json). It measures the Guion ARM64 installed artifact against pinned, GIL-enabled CPython 3.14.7 on macOS. The report retains raw paired samples and percentile intervals.
- The earlier Guion package-performance report measured the installed CLI. The newer runtime-efficiency cohort separately measured fresh uncached source and prebuilt `.gub` program artifacts through the release CLI; both were faster than pinned CPython 3.14.7 on each named workload. On 200,000-row JSONL, uncached source took a 4.025-second median versus 17.670 seconds for the Python data reference; the prebuilt program artifact median was 4.030 seconds. A focused runtime change reduced the uncached-source median from the merged baseline's 4.160 seconds by 3.2%; the candidate/baseline median-ratio bootstrap interval was [0.941, 0.983]. Peak RSS was effectively unchanged. The five 21-pair short-source gates passed again. See the [runtime-efficiency summary](../evaluation/completion/results/runtime-efficiency/2026-10-26-summary.md) and its linked raw reports. These results cover named programs and conditions only; they do not show that Guion generally outperforms or replaces Python. Free-threaded Python remains unmeasured because it was unavailable.
- The frozen independent recommendation campaign selected Python in all 18 choices for its older candidate. A later three-pair builder study recommended either language for two work classes and Veritas for one. These are separate historical studies with different questions; neither is a general adoption estimate.

- The 2026-10-25 source-run sprint added a bounded source compilation cache. Its results are historical and remain in the [dated comparison](../evaluation/completion/results/source-loop/final-comparison-20261026.json) and [readiness section](pre-release-readiness.md#source-run-and-launch-speed-sprint-2026-10-25). The later 2026-10-26 cohort above reran uncached source and supersedes that sprint's source-versus-Python comparisons for the named workloads.

The new Guion evidence is indexed separately from the historical Veritas cohorts in the [evidence index](history/evidence-index.md). The complete matrix, package details, and current limitations are in the [pre-release readiness report](pre-release-readiness.md).

## CI and release gates

GitHub Actions runs formatting, strict Clippy, workspace tests, and schema checks on Ubuntu 24.04 x86_64. That job is a source check only; it does not certify Guion's supported release targets. The release gates remain separate and require macOS ARM64 and Debian 12-compatible GNU/Linux ARM64 with glibc. A target that did not run is reported as incomplete.

## Remaining work

The pre-release candidate has no public distribution. Broader ecosystem use, additional target platforms, and a free-threaded Python comparison remain future work. The retained performance suite supports only its named benchmarks; the historical Veritas builder study is not a new Guion builder cohort. See the [approved migration plan](superpowers/plans/2026-10-24-veritas-complete-product.md) and [readiness report](pre-release-readiness.md) for the exact handoff boundary.

This status page is a current summary, not the test record. The [historical index](history/README.md) links to the original completion report, checkpoints, campaigns, and preserved results.

## 2026-10-25 closeout clarification

The final application evidence contains 18 passing reports per required target, 36 passing reports in total and zero failures. Free-threaded Python was not installed for the comparison, so that comparison was not run and has no result. The original Veritas checkout was retired after its unique small working records were retained in `docs/history/veritas-working-sessions-2026-10-25/`. The idle Colima VM was stopped gracefully, closing its deleted build-cache handles.
