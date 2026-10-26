# Guion final validation cohort

This directory contains the Guion-identity release-candidate evidence. The current three-target matrix validated Guion 0.1.0 / artifact format 39 on macOS ARM64 and Debian 12 GNU/Linux on ARM64 and x86_64, all with glibc. The complete evidence from [GitHub Actions run 40360446044](https://github.com/rmichaelthomas/guion/actions/runs/40360446044) is retained under [`target-runs/three-target-cohort-run-40360446044/`](target-runs/three-target-cohort-run-40360446044/). Its target reports are bound to source commit `0fecfbf1694474b5e85bebb8bc4f87db29275f53`, with no changed product paths (`working_tree_digest: sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`). The installed-performance reports below remain the earlier macOS ARM64 cohort; this target run adds no Linux performance claim. The Veritas-era reports elsewhere in `evaluation/completion/results/` remain unchanged historical evidence.

| Target | Tested host | Archive size | Archive SHA-256 |
|---|---|---:|---|
| macOS ARM64 | macOS 15.7.9 | 5,355,253 bytes | `682c5102263392b626ceec89ab69983568905d1ca0e2e93e3e24c6cc91f99994` |
| Debian ARM64 | Debian 12.15, glibc 2.36 | 5,890,549 bytes | `219bc1406b94120e08e104061743ac7944ea881155ba7fd378f18fc5aa7d0264` |
| Debian x86_64 | Debian 12.15, glibc 2.36 | 6,503,721 bytes | `50e9ed378ca1a3e9d4fdccad9605023435d7f093c643c8d8d5debb8756aca266` |

| Embedded identity | SHA-256 |
|---|---|
| Artifact schema | `7c91c82c3ff2a1351fe30dc9bd3350eb1d88dc914b59f3a34cc288858a94958f` |
| Compiler seed | `e168320c43f05d8d5814fea6133564f1600815839196953d1fd2d311d0ee05ef` |
| Compiler seed manifest | `456431e2825641b827f7a9f61f34984403f6cd1edf4ba30bf178f7d021dfaa0f` |
| Runtime | `61f5df917e86d9304fdd7ab3749eb0c81157d69ed0b160aaee5059b5f24a8825` |
| Standard library | `e2031b25149d6e4a07bd16d13e7a85487bab254c35a9f5d52bbe84ad701bf538` |

The target matrix used rustc/cargo 1.97.1 on all three hosts. The Ubuntu x86_64 source-check job passed formatting, strict Clippy, the full workspace tests, report-schema validation, generated agent-reference validation, and switch-condition ledger validation. The release-target jobs passed the three-stage compiler bootstrap, conformance, standard-library, runtime, dependency, lifecycle, application, and release checks on all three targets. The application gate produced 18 passing reports per target: 54 of 54 overall.

## Aggregate reports

| Gate | Aggregate report | Detailed evidence |
|---|---|---|
| Self-hosted bootstrap | [`bootstrap.json`](bootstrap.json) | Compiler identity and stage evidence in the aggregate |
| Language conformance | [`conformance.json`](conformance.json) | [`conformance-inventory.json`](conformance-inventory.json), run-stamped target records under [`target-runs/`](target-runs/) and full logs in the current cohort archive |
| Standard library | [`stdlib-data.json`](stdlib-data.json), [`stdlib-services.json`](stdlib-services.json) | Checks and counts in each report |
| Runtime execution, HTTP, lifecycle | [`runtime.json`](runtime.json) | all three targets under `target-runs/` and the current cohort archive |
| Python-free core dependencies | [`product-dependencies.json`](product-dependencies.json) | Native package and dependency checks |
| Installed product lifecycle | [`product-lifecycle.json`](product-lifecycle.json) | all three targets under `target-runs/` |
| Nine regression applications, initial and follow-up | [`product-applications.json`](product-applications.json) | 54 application reports in the per-target `applications/` directories in the current cohort archive |
| Release archives and relocation | [`release.json`](release.json) | Three archive hashes and installation audits under `target-runs/` and the current cohort archive |
| Installed performance | [`performance-guion.json`](performance-guion.json), [`performance.json`](performance.json) | Paired raw samples, intervals, correctness and resource measurements |
| Earlier closeout snapshot | [`closeout.json`](closeout.json) | Schema result in [`closeout-schema.json`](closeout-schema.json); this records the prior single-target closeout, not the current three-target matrix |

## Target interpretation

The runtime, application, product-lifecycle, and release aggregates passed with all three required targets completed and no skipped target. Each run-stamped target report records one target and may therefore mark itself incomplete; the aggregate is the combined result. Bootstrap's three compiler stages and compiler-stage identity were checked across the three targets. The older `closeout.json` remains a separate, earlier handoff snapshot and does not describe this matrix.

The installed-performance cohort was measured on macOS ARM64 against pinned, GIL-enabled CPython 3.14.7. It completed 15 paired compute trials and 16 data trials per measured comparison, with two warmups, five warm repetitions, and 20,000 seeded percentile-bootstrap resamples. Free-threaded Python was unavailable and remains unverified. The exact figures are in `performance-guion.json`; the summary in [`docs/pre-release-readiness.md`](../../../../docs/pre-release-readiness.md) is intentionally rounded. The three-target validation above establishes no Linux performance result.

The final installed-artifact median ratios (Guion/Python; exact percentile 95% intervals) were: Richards 0.9578 [0.9172, 0.9664]; N-body 0.6963 [0.6904, 0.7114]; Mandelbrot 0.3755 [0.3702, 0.3834]; parallel CPU, 20,000 items versus Python serial 0.1955 [0.1863, 0.2020]; parallel CPU, 200,000 items versus Python serial 0.1984 [0.1890, 0.2114]; and JSONL summary 0.2312 [0.2274, 0.2333]. For 200,000 parallel items, the ratios were 0.1897 [0.1862, 0.2086] versus Python threads and 0.0994 [0.0927, 0.1106] versus Python processes. JSONL versus SQLite was 0.2302 [0.2244, 0.2325].

The retained builders report outside this directory is historical Veritas evidence. It is not a Guion-identity builder cohort and must not be described as one.

## Repository size check

The refreshed outgoing Git tree is 47,628,458 bytes (45.42 MiB), about 20.42 MiB over the plan's soft 25 MiB aim. It includes the full per-target Guion applications, raw performance samples, and final gate reports; the new three-target evidence bundle is 3.90 MiB. The largest individual file is 3,125,741 bytes (2.98 MiB), below the 50 MiB per-file ceiling. The migration ledger records the decision to retain directly readable evidence instead of opaque build products.
