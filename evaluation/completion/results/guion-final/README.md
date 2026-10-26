# Guion final validation cohort

This directory contains the Guion-identity release-candidate evidence. Product code and documentation were validated as Guion 0.1.0 / artifact format 39 on macOS ARM64 and Debian 12-compatible GNU/Linux ARM64 with glibc. The Veritas-era reports elsewhere in `evaluation/completion/results/` remain unchanged historical evidence.

Final release and installed-performance evidence is bound to source commit `ab4cdf95604c75be334c8aa5ce9f491758c0ff28`, with no uncommitted product paths (`working_tree_digest: sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`). Both reports carry the same source revision, empty working-tree digest, compiler seed, runtime, and standard-library identities.

| Target | Tested host | Archive size | Archive SHA-256 |
|---|---|---:|---|
| macOS ARM64 | macOS 27.0.1 | 5,429,536 bytes | `bf24981b70f9c96833b7c5a1e8452b78acb6a4ec0ca11fb465d4df541141ec9c` |
| Debian ARM64 | Debian 12.15, glibc | 5,880,206 bytes | `67de0ee99b613891d1223965680c6445df849cb24ab3c86ba87b34ff92def7ad` |

| Embedded identity | SHA-256 |
|---|---|
| Artifact schema | `7c91c82c3ff2a1351fe30dc9bd3350eb1d88dc914b59f3a34cc288858a94958f` |
| Compiler seed | `cdcdaf6baef1657bee9500393db9b67f3a3f810b15403dea63c59be3cc36f001` |
| Compiler seed manifest | `fb26fc800f1c263c22a1704a601aeaf734b5337124f5c5356ec6e47a66e6f6f7` |
| Runtime | `ab68e378d1cadca5683aefcc842e1fb4e585fa7145a56ee98eee7b0bd9b4c3ab` |
| Standard library | `71259e69ef34dcc5d05d50bd3537a5d0f00101069cdcf1df7456941cdfd14833` |

The Rust toolchain was rustc/cargo 1.97.1 on both target hosts. The performance baseline was GIL-enabled CPython 3.14.7 on macOS ARM64. The full Rust workspace command `cargo test --workspace --locked --offline` passed 316 tests; its app-integration run needed local loopback permission because the sandbox denied the fixture server on the first attempt. Formatting, strict Clippy, two Python performance-runner tests, schema checks, and CI workflow validation also passed.

## Aggregate reports

| Gate | Aggregate report | Detailed evidence |
|---|---|---|
| Self-hosted bootstrap | [`bootstrap.json`](bootstrap.json) | Compiler identity and stage evidence in the aggregate |
| Language conformance | [`conformance.json`](conformance.json) | [`conformance-inventory.json`](conformance-inventory.json), per-target records under [`target-runs/`](target-runs/) |
| Standard library | [`stdlib-data.json`](stdlib-data.json), [`stdlib-services.json`](stdlib-services.json) | Checks and counts in each report |
| Runtime execution, HTTP, lifecycle | [`runtime.json`](runtime.json) | macOS and Debian attempts under `target-runs/` |
| Python-free core dependencies | [`product-dependencies.json`](product-dependencies.json) | Native package and dependency checks |
| Installed product lifecycle | [`product-lifecycle.json`](product-lifecycle.json) | macOS and Debian attempts under `target-runs/` |
| Nine regression applications, initial and follow-up | [`product-applications.json`](product-applications.json) | 18 application reports under `applications/` and target attempts under `target-runs/` |
| Release archives and relocation | [`release.json`](release.json) | Target archives, hashes, and installation audits under `target-runs/` |
| Installed performance | [`performance-guion.json`](performance-guion.json), [`performance.json`](performance.json) | Paired raw samples, intervals, correctness and resource measurements |
| Closeout | [`closeout.json`](closeout.json) | Schema result in [`closeout-schema.json`](closeout-schema.json) |

## Target interpretation

The runtime, application, product-lifecycle, and release aggregates passed with both required targets completed and no skipped target. Each target-run file records only one target and may therefore mark itself incomplete; the aggregate is the combined result. Bootstrap's three compiler stages were verified on Debian ARM64, with compiler stage equality covered by the conformance/bootstrap evidence.

The final installed performance cohort was measured on macOS ARM64 against pinned, GIL-enabled CPython 3.14.7. It completed 15 paired compute trials and 16 data trials per measured comparison, with two warmups, five warm repetitions, and 20,000 seeded percentile-bootstrap resamples. Free-threaded Python was unavailable and remains unverified. The exact figures are in `performance-guion.json`; the summary in [`docs/pre-release-readiness.md`](../../../../docs/pre-release-readiness.md) is intentionally rounded.

The final installed-artifact median ratios (Guion/Python; exact percentile 95% intervals) were: Richards 0.9578 [0.9172, 0.9664]; N-body 0.6963 [0.6904, 0.7114]; Mandelbrot 0.3755 [0.3702, 0.3834]; parallel CPU, 20,000 items versus Python serial 0.1955 [0.1863, 0.2020]; parallel CPU, 200,000 items versus Python serial 0.1984 [0.1890, 0.2114]; and JSONL summary 0.2312 [0.2274, 0.2333]. For 200,000 parallel items, the ratios were 0.1897 [0.1862, 0.2086] versus Python threads and 0.0994 [0.0927, 0.1106] versus Python processes. JSONL versus SQLite was 0.2302 [0.2244, 0.2325].

The retained builders report outside this directory is historical Veritas evidence. It is not a Guion-identity builder cohort and must not be described as one.

## Repository size check

The exact outgoing Git tree was 27,277,121 bytes (26.01 MiB), about 1.01 MiB over the plan's soft 25 MiB aim. It includes the full per-target Guion applications, raw performance samples, and final gate reports. The largest individual file was 3,125,741 bytes (2.98 MiB), below the 50 MiB per-file ceiling. The migration ledger records the decision to retain this complete, directly readable evidence set.
