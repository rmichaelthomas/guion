# Guion and Veritas evidence index

This index keeps evidence cohorts separate. The current Guion pre-release validation and installed-speed results are recorded in a new Guion cohort below. Historical Veritas reports keep their original names, values, and product identities; no report is renamed, rescored, or assigned a different identity.

## Product identities and primary summaries

| Evidence | Product identity | Exact record |
|---|---|---|
| Final historical completion summary | Veritas 0.1.0, artifact format 39; source commit `cb5587b1f4d48a72b955ad2e6e666cdef28f5373`; audited source digest `sha256:4d515d48ba1912119df9cb800cc73d6c1a8ddf02ca25a66de4589b51c9258066` | [completion report](../../docs/completion-report.md), [closeout JSON](../../evaluation/completion/results/closeout.json) |
| Rename acceptance applications | Guion; each report records `product: Guion`, its relocated artifact hash and pass result | [Guion application reports](../../evaluation/completion/results/guion-applications/), [application acceptance aggregate](../../evaluation/completion/results/product-applications.json) |
| Current product scope and pre-release boundary | Guion 0.1.0, private pre-release candidate | [status](../status.md), [readiness report](../pre-release-readiness.md), [migration and pre-release history](migration-2026-10-25.md) |

## Guion final pre-release cohort

This cohort is the renamed product's final validation record. Its aggregates and per-target reports are retained under [`evaluation/completion/results/guion-final/`](../../evaluation/completion/results/guion-final/), with a cohort guide in [`README.md`](../../evaluation/completion/results/guion-final/README.md). The supported matrix passed on macOS ARM64 and Debian 12-compatible GNU/Linux ARM64. The [`closeout.json`](../../evaluation/completion/results/guion-final/closeout.json) records the final closeout audit. The builder study remains historical Veritas evidence and is identified as such; it was not relabeled or repeated under Guion.

The Guion installed benchmark is [`performance-guion.json`](../../evaluation/completion/results/guion-final/performance-guion.json); [`performance.json`](../../evaluation/completion/results/guion-final/performance.json) records invocation and outcomes. Ratios are Guion installed-artifact median divided by pinned CPython 3.14.7 median, so values below 1 favor the Guion artifact.

| Workload and comparison | Approximate median ratio |
|---|---:|
| Richards scheduler | 0.92 |
| N-body numeric | 0.68 |
| Mandelbrot image | 0.37 |
| Parallel CPU, 20,000 items vs Python serial | 0.19 |
| Parallel CPU, 200,000 items vs Python serial | 0.18 |
| JSONL summary data | 0.23 |

For 200,000 parallel items, the Guion artifact also beat measured Python threads and processes. For JSONL, it also beat the measured SQLite path. Exact medians, intervals, and both parallel comparison ratios are in the linked structured report. The comparison used macOS ARM64 and GIL-enabled CPython 3.14.7; free-threaded Python was unavailable. Guion source-run was slower on Richards, N-body, and Mandelbrot. Treat these as results for the named workloads and conditions, not a general Python performance claim.

The Veritas completion cohort records artifact schema, compiler seed and manifest, runtime, and standard-library hashes in each structured report. Its final release and performance reports are bound to the Veritas source commit and audited digest shown above. The Guion rename did not rewrite those reports.

## Historical independent recommendation campaigns

These campaigns answer different questions and must not be combined into one score:

| Cohort | Product identity and result | Exact records and caveat |
|---|---|---|
| Independent v1 | Initial Veritas candidate packet; **invalidated before formal collection**. It has no valid scored result. | [campaign note](../../evaluation/results/independent-v1/README.md), [freeze record](../../evaluation/results/independent-v1/freeze-record.json) |
| Independent v2 | Veritas artifact format 30; eight partial implementation sessions were started. No official recommendation choices or runner results were recorded. The bulky raw archive was removed during cleanup, so byte-for-byte replay is unavailable. | [campaign note](../../evaluation/results/independent-v2/README.md), [preserved inputs](../../evaluation/results/independent-v2/preserved-inputs/README.md), [migration retention manifest](migration-retention-manifest.json) |
| Independent v3 | Veritas 0.1.0, artifact format 33, Linux ARM64 candidate; **18 valid blinded recommendations, all 18 chose Python**. This is a task-specific recommendation result, not a performance score or a judgment on format 39/Guion. | [campaign note](../../evaluation/results/independent-v3/README.md), [choice records](../../evaluation/results/independent-v3/recommendations/), [choice evidence](../../evaluation/results/independent-v3/choice-evidence/) |

The independent-v3 speed calibration justified running the recommendation campaign; it was not the completion performance suite. The campaign README records its narrow scope and the protocol corrections that make the first two build pairs diagnostic until rerun.

## Nine known regression applications

These nine briefs are retained regression applications. They measure product correctness and maintenance behavior; they are not unfamiliar fresh-builder evidence. Each app has original Veritas results and a separately labeled post-rename Guion report. Generated oracle cases are in the matching `*-generated.json` file.

| Application | Brief | Veritas result | Guion result |
|---|---|---|---|
| SLA report | [brief](../../evaluation/briefs/independent/01-support-sla-report.md) | [report](../../evaluation/completion/results/applications/sla-report.json) | [report](../../evaluation/completion/results/guion-applications/sla-report.json) |
| JSONL summary | [brief](../../evaluation/briefs/independent/02-jsonl-retention.md) | [report](../../evaluation/completion/results/applications/jsonl-summary.json) | [report](../../evaluation/completion/results/guion-applications/jsonl-summary.json) |
| Release notes | [brief](../../evaluation/briefs/independent/03-release-notes.md) | [report](../../evaluation/completion/results/applications/release-notes.json) | [report](../../evaluation/completion/results/guion-applications/release-notes.json) |
| Calendar conflicts | [brief](../../evaluation/briefs/independent/04-calendar-conflict-report.md) | [report](../../evaluation/completion/results/applications/calendar-conflicts.json) | [report](../../evaluation/completion/results/guion-applications/calendar-conflicts.json) |
| Webhook replay | [brief](../../evaluation/briefs/independent/05-webhook-replay.md) | [report](../../evaluation/completion/results/applications/webhook-replay.json) | [report](../../evaluation/completion/results/guion-applications/webhook-replay.json) |
| Health digest | [brief](../../evaluation/briefs/independent/06-service-health-digest.md) | [report](../../evaluation/completion/results/applications/health-digest.json) | [report](../../evaluation/completion/results/guion-applications/health-digest.json) |
| Config migrator | [brief](../../evaluation/briefs/independent/07-config-migrator.md) | [report](../../evaluation/completion/results/applications/config-migrator.json) | [report](../../evaluation/completion/results/guion-applications/config-migrator.json) |
| Documentation index | [brief](../../evaluation/briefs/independent/08-documentation-link-index.md) | [report](../../evaluation/completion/results/applications/doc-index.json) | [report](../../evaluation/completion/results/guion-applications/doc-index.json) |
| Lockfile auditor | [brief](../../evaluation/briefs/independent/09-lockfile-auditor.md) | [report](../../evaluation/completion/results/applications/lockfile-auditor.json) | [report](../../evaluation/completion/results/guion-applications/lockfile-auditor.json) |

The post-rename Guion reports cover initial and follow-up behavior and include seeded generated cases; the retained Guion report set records 18 of 18 passing. The Veritas completion cohort also retains target-specific application results for macOS ARM64 and Debian-compatible ARM64 under `applications-debian-aarch64/` and the aggregate `product-applications.json`.

## Fresh matched builders

These are three task-specific Python/Veritas pairs. The final maintainable-app pair is the fresh **Field Kit Log** reevaluation after Veritas gained the local-date clock API. The older Bookloom pair and pre-clock assessment remain useful history but are not substituted for that final pair.

| Class | Fresh pair | Final recommendation | Assessment |
|---|---|---|---|
| Small automation CLI | [Invoice Sentinel: Python](../../evaluation/completion/builder/reports/invoice-sentinel-python.json), [Veritas](../../evaluation/completion/builder/reports/invoice-sentinel-veritas.json) | Either | [assessment](../../evaluation/completion/builder/reports/assessment-small-automation-cli.json) |
| Concurrent network tool | [Quote Merge: Python](../../evaluation/completion/builder/reports/quote-merge-python.json), [Veritas](../../evaluation/completion/builder/reports/quote-merge-veritas.json) | Veritas for this brief | [assessment](../../evaluation/completion/builder/reports/assessment-concurrent-network.json) |
| Maintainable local app | [Field Kit Log: Python](../../evaluation/completion/builder/reports/field-kit-log-python.json), [Veritas](../../evaluation/completion/builder/reports/field-kit-log-veritas.json) | Either | [final assessment](../../evaluation/completion/builder/reports/assessment-maintainable-local-app.json) |

The assessment documents correctness, follow-up behavior, relocation, setup and delivery observations, material regressions, and recommendation rationale. The cohort is exploratory and too small to estimate general adoption. The [assignment record](../../evaluation/completion/builder/assignments.json), [protocol](../../evaluation/completion/builder/protocol.md), [build evidence directory](../../evaluation/completion/builder/reports/) and aggregate [`builders.json`](../../evaluation/completion/results/builders.json) retain provenance. The `bookloom-veritas-initial-attempt.json` and `assessment-maintainable-local-app-before-clock.json` files preserve earlier attempts and are not hidden.

## Installed performance suite

The complete report is [performance-host.json](../../evaluation/completion/results/performance-host.json), with the structured command record in [performance.json](../../evaluation/completion/results/performance.json). It measures the installed Veritas artifact against the pinned, GIL-enabled **CPython 3.14.7** on the report's macOS ARM64 host. The ratio below is `Veritas artifact median / Python median`; a value below 1 favors the Veritas artifact. The report retains raw samples, paired order, percentile 95% intervals, correctness outputs, memory, artifact sizes and target binary hashes.

| Required advertised workload | Python comparison | Median ratio | 95% interval |
|---|---|---:|---:|
| JSONL summary data | Python data path | 0.2382 | 0.2356–0.2400 |
| Mandelbrot image | Python reference | 0.3848 | 0.3776–0.3958 |
| N-body numeric | Python reference | 0.6972 | 0.6921–0.7010 |
| Parallel CPU, 200,000 items | Python processes | 0.1100 | 0.1014–0.1158 |
| Parallel CPU, 200,000 items | Python serial | 0.2125 | 0.1972–0.2146 |
| Parallel CPU, 200,000 items | Python threads | 0.2090 | 0.1964–0.2196 |
| Richards scheduler | Python reference | 0.9502 | 0.9363–0.9652 |

The required installed-artifact comparisons all passed the report's speed-win gate. That is evidence for these measured workloads and conditions only. It does not establish that Guion is broadly faster than Python; a Guion-identity rerun is a separate remaining pre-release task.

### Regressions, ties, and limits

- The same report keeps source-run and warm-artifact timings separate from installed-artifact timings. Source-run Veritas was slower than Python on Mandelbrot (median ratio 1.5685; 15 tie/regression observations), N-body (1.7379; 15), and Richards scheduler (12.9125; 15). This is why source-run and installed-package numbers must not be blended.
- The smaller parallel CPU workload (20,000 items) is also retained. Its installed artifact beat the measured Python serial, threads, and process comparisons; source-run recorded one tie/regression observation against Python serial. All per-comparison observations remain in the report.
- The report records a 5,414,452-byte release archive and a 15,428,255-byte extracted package for this Veritas build. It records no universal memory ceiling; memory samples and per-workload values are retained in the JSON.
- A free-threaded CPython 3.14 executable was not installed. The free-threaded Python concurrency comparison is **unverified**. The measured CPython 3.14.7 baseline had the GIL enabled.
- The validated release hosts are macOS ARM64 and Debian 12-compatible GNU/Linux ARM64 with glibc. Other operating systems, architectures, and libc variants remain unverified or out of scope.

## Full completion report map

All retained structured completion JSON lives under [`evaluation/completion/results/`](../../evaluation/completion/results/). Its [file-by-file index](../../evaluation/completion/results/README.md) maps each report family. The principal gates are:

| Gate | Report |
|---|---|
| Language contract inventory and conformance | [inventory](../../evaluation/completion/results/conformance-inventory.json), [aggregate result](../../evaluation/completion/results/conformance.json), [target-attempt records](../../evaluation/completion/results/target-runs/) |
| Three-stage compiler bootstrap | [bootstrap](../../evaluation/completion/results/bootstrap.json) |
| Data and service standard library | [data](../../evaluation/completion/results/stdlib-data.json), [services](../../evaluation/completion/results/stdlib-services.json) |
| Runtime behavior and lifecycle memory | [aggregate runtime](../../evaluation/completion/results/runtime.json), [macOS ARM64](../../evaluation/completion/results/runtime-macos-aarch64.json), [Debian ARM64](../../evaluation/completion/results/runtime-debian-aarch64.json), [memory records](../../evaluation/completion/results/) |
| Python-free core and dependency/package lifecycle | [dependencies](../../evaluation/completion/results/product-dependencies.json), [lifecycle](../../evaluation/completion/results/product-lifecycle.json) |
| Both release archives and relocatable installations | [release aggregate](../../evaluation/completion/results/release.json), [macOS target](../../evaluation/completion/results/target-runs/release-macos-aarch64.json), [Debian target](../../evaluation/completion/results/target-runs/release-debian-aarch64.json) |
| Completion closeout | [closeout](../../evaluation/completion/results/closeout.json); schema-only validation is [closeout-schema.json](../../evaluation/completion/results/closeout-schema.json) |

Aggregate reports record their own status, required/completed/skipped targets, source revision, working-tree digest, artifact hashes, checks, and command output. Some files under `target-runs/` are partial-attempt records with `status: incomplete`; preserve and read those as such, not as passing target runs. The aggregate reports and original completion report are the authorities for the full historical Veritas cohort. The final Guion-identity target matrix remains to be run and recorded in Task 8 of the migration plan.
