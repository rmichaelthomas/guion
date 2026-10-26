# Guion pre-release readiness

**Status: private pre-release candidate; not a public release.** This handoff records the Guion-identity validation completed for version 0.1.0 / artifact format 39. The GitHub repository remains private, and the retained host packages are validation evidence rather than public downloads.

## What passed

The final validation cohort lives under [`evaluation/completion/results/guion-final/`](../evaluation/completion/results/guion-final/). Its [`README`](../evaluation/completion/results/guion-final/README.md) maps each aggregate to its detailed target records. The matrix passed on macOS ARM64 and Debian 12-compatible GNU/Linux ARM64 with glibc:

- Three-stage self-hosted compiler bootstrap on Debian ARM64; compiler-stage identity and language conformance were checked across both required targets.
- The full 195-case language conformance corpus, runtime execution/HTTP/lifecycle checks, and standard-library data and service suites.
- All nine retained regression applications in initial and follow-up forms: 18 of 18 reports passed across both targets.
- Python-free core dependency checks, dependency and installed-product lifecycle checks, release archive contents and license inventory, clean-room installation, relocation, integrity rejection, and dynamic-library audits.
- A Guion-identity installed-artifact performance cohort with correctness checks, paired randomized samples, and percentile intervals.
- Formatting, strict Clippy, Rust unit/integration suites, schema validation, CI workflow validation, exact outgoing-tree secret scan, and package/license audits.

The macOS and Debian runtime, application, lifecycle, and release aggregates list both targets as completed with no skipped target. Individual target-attempt JSON files can say `incomplete` because each one contains only one side of the aggregate; use the matching aggregate to read the combined gate.

## Performance result and boundary

The performance report compares the installed Guion artifact with pinned, GIL-enabled CPython 3.14.7 on macOS ARM64. Each reported ratio is the median of paired per-trial ratios (Guion time divided by Python time); below 1.0 means the Guion artifact was faster for that named comparison.

| Workload | Approximate result |
|---|---|
| Richards scheduler | Slightly less time than Python |
| N-body numeric | About 30% less time |
| Mandelbrot image | About 60% less time |
| Parallel CPU, 20,000 items vs Python serial | About 80% less time |
| Parallel CPU, 200,000 items vs Python serial | About 80% less time |
| JSONL summary data | About 80% less time |

For 200,000 parallel items, the installed artifact was also faster than Python threads and processes. For JSONL, it was faster than the measured SQLite path. The exact medians, ratios, intervals, all observations, source-run timings, correctness outputs, memory, artifact sizes, and hashes are in [`performance-guion.json`](../evaluation/completion/results/guion-final/performance-guion.json) and [`performance.json`](../evaluation/completion/results/guion-final/performance.json).

These results cover the named workloads, host, and pinned Python version only. Guion source-run was slower than Python on Richards, N-body, and Mandelbrot. A free-threaded Python executable was unavailable, so that comparison remains unverified. This evidence does not establish that Guion generally outperforms Python or has replaced it.

The retained three-pair builder study is from the historical Veritas identity, not a Guion-identity study. The old scored outcomes remain preserved as history; they should not be represented as Guion recommendations. Exact byte-for-byte replay of the deleted raw Veritas v2 session archive is unavailable, as recorded in the [migration retention manifest](history/migration-retention-manifest.json).

## Supported and unverified systems

Validated release targets:

- macOS on ARM64.
- Debian 12-compatible GNU/Linux on ARM64 with glibc.

Other operating systems, CPU architectures, Linux libc variants, free-threaded Python, and package-manager integrations are unverified or out of scope. The Ubuntu x86_64 GitHub Actions job checks source formatting, lint, tests, and schemas; it is not a release-target certification.

## Installing, updating, and removing a local package

The project has no public download yet. For source checkout development, follow [Getting started](getting-started.md). For a locally available host archive, extract it and run `sh install.sh`; by default this installs under `~/.local`. Pass a prefix, such as `sh install.sh /chosen/prefix`, to select another location. Add that prefix's `bin` directory to `PATH` if you want to run `guion` from any directory. The installer does not change shell startup files.

To update, install a newer version from its extracted archive using the same prefix. Each version gets its own directory under `share/guion/`; the managed launcher points to the newest installation, and previous versions remain available. The installer refuses to overwrite the same version directory. To remove Guion, use the prefix chosen at install time, remove only the Guion-managed `bin/guion` launcher and the specific version directory under `share/guion/`, and leave unrelated files in that prefix alone. If another program replaced the launcher, do not remove it as Guion's launcher.

This installer supports the packaged host only. The archives and their exact SHA-256 values and sizes are recorded by [`release.json`](../evaluation/completion/results/guion-final/release.json) and the per-target release records. Third-party license texts, including the retained libical LGPL text, are included in the distribution and indexed by [`THIRD-PARTY-NOTICES.md`](../THIRD-PARTY-NOTICES.md).

## Identity and retained records

The final release reports record the compiler seed, runtime, standard-library, and artifact-schema hashes. The self-hosted seed is format 39 with host ABI 1. The Rust workspace and embedded compiler/runtime are source; Rust is not required by an installed core distribution, and Python interoperability is optional.

Historical Veritas checkpoints, plans, evaluation outcomes, and original reports keep their original identity. The Guion results are a separate cohort. The migration record explains what was retained, what was regenerated, and which bulky raw sessions were removed.

## Cold-start handoff for another agent

Start with the repository alone; do not rely on this chat. Read these files in order:

1. [`docs/agents/start-here.md`](agents/start-here.md)
2. [`docs/agents/index.json`](agents/index.json)
3. [`language/spec.md`](../language/spec.md)
4. [`docs/status.md`](status.md) and this readiness report
5. The relevant schema and runnable example linked by the agent index

Use this prompt when resuming implementation:

```text
You are working in the private Guion programming-language repository. First read docs/agents/start-here.md, docs/agents/index.json, language/spec.md, docs/status.md, and docs/pre-release-readiness.md. Treat the language specification and checked-in schemas as authoritative. Keep claims tied to retained reports, preserve historical Veritas evidence without relabeling it, and run the repository's documented checks for any behavior change. Do not publish a release or make the repository public without explicit authorization.
```

## Pre-release decision boundary

This repository is organized and validated for a separate pre-release decision. Publishing a release, making the repository public, removing the original Veritas source checkout, and changing project-level `AGENTS.md` remain outside this handoff.
