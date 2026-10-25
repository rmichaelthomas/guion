# Completion result files

This directory contains the complete structured completion report set for the final Veritas 0.1.0, artifact format 39 build. These files are historical Veritas evidence. They are not retitled or rewritten as Guion evidence. The separately labeled post-rename application acceptance reports are in [`guion-applications/`](guion-applications/).

For the conclusions, precise product identities, benchmark ratios and intervals, frozen campaigns, builder studies, and known limitations, read the [evidence index](../../../docs/history/evidence-index.md). The original methodology and required report policy are in [`../README.md`](../README.md).

## Report map

- `conformance-inventory.json`: language-contract case inventory.
- `conformance.json`, `target-runs/conformance-*.json`: aggregate conformance and retained target attempts.
- `bootstrap.json`: three-stage compiler bootstrap and identity checks.
- `stdlib-data.json`, `stdlib-services.json`: data and service library checks.
- `runtime.json`, `runtime-macos-aarch64.json`, `runtime-debian-aarch64.json`: aggregate and target runtime checks.
- `runtime-memory-*.json`: lifecycle memory samples and plateau observations per target.
- `product-dependencies.json`, `product-lifecycle.json`, `target-runs/product-lifecycle-*.json`: dependency and project lifecycle checks.
- `product-applications.json`, `applications/`, `applications-debian-aarch64/`: nine known regression applications, initial and follow-up results, generated cases, and target-specific evidence.
- `guion-applications/`: separately labeled post-rename Guion application acceptance reports and generated oracle cases.
- `release.json`, `target-runs/release-*.json`: release checks and per-target attempt reports.
- `performance.json`, `performance-host.json`: installed performance command record and complete raw host measurements, samples, intervals, correctness checks, memory, and package sizes.
- `builders.json`: fresh matched builder cohort summary; individual reports and source/delivery evidence are under [`../builder/`](../builder/).
- `closeout-schema.json`: schema-only check; it does not claim product closeout.
- `closeout.json`: final historical Veritas completion audit.

Reports under `target-runs/` record individual attempts. Some are explicitly incomplete; do not use them alone to claim a full target gate passed. Consult the aggregate report and the evidence index for the correct cohort interpretation.
