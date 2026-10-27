# Guion pre-release readiness

**Status: private pre-release candidate; not a public release.** This handoff records the Guion-identity validation completed for version 0.1.0 / artifact format 39. The GitHub repository remains private, and the retained host packages are validation evidence rather than public downloads.

## What passed

The earlier three-target validation cohort lives under [`evaluation/completion/results/guion-final/`](../evaluation/completion/results/guion-final/). Its [`README`](../evaluation/completion/results/guion-final/README.md) maps each aggregate to its detailed target records. That matrix passed on macOS ARM64 and Debian 12 GNU/Linux on ARM64 and x86_64, all with glibc:

- Three-stage self-hosted compiler bootstrap and compiler-stage identity checks across all three targets, alongside the full language conformance corpus.
- The full 195-case language conformance corpus, runtime execution/HTTP/lifecycle checks, and standard-library data and service suites.
- All nine retained regression applications in initial and follow-up forms: 18 of 18 reports passed on each target, or 54 of 54 individual reports in total. No application report failed.
- Python-free core dependency checks, dependency and installed-product lifecycle checks, release archive contents and license inventory, clean-room installation, relocation, integrity rejection, and dynamic-library audits.
- A Guion-identity installed-artifact performance cohort with correctness checks, paired randomized samples, and percentile intervals.
- Formatting, strict Clippy, Rust unit/integration suites, schema validation, CI workflow validation, exact outgoing-tree secret scan, and package/license audits.

That cohort's runtime, application, lifecycle, and release aggregates list all three targets as completed with no skipped target. Individual target-attempt JSON files can say `incomplete` because each one contains only one side of the aggregate; use the matching aggregate to read the combined gate. The detailed target evidence is preserved under [`target-runs/`](../evaluation/completion/results/guion-final/target-runs/).

## Performance result and boundary

The later post-VM-core [real-task and runtime sprint](../evaluation/completion/results/real-task-sprint/summary.md) supersedes the earlier Richards source measurement on macOS: 45.67 ms for Guion versus 70.16 ms for CPython 3.14.7 in the named fresh-source cohort. It also records 127 passing cases in each language across nine applications, a validated AllHereHTML regression, text-allocation improvements, and the current macOS installation check. The three-target records below describe the earlier validation cohort; they do not by themselves establish that a later build passed on all three targets.

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

These results cover the named workloads, host, and pinned Python version only. An earlier source-run comparison found Richards slower than Python while N-body and Mandelbrot were faster; the later runtime-efficiency cohort below reran all three with fresh uncached source. The free-threaded Python comparison was not run: this host had no `python3.14t` or `python3t` executable. It therefore has no pass or fail result. This evidence does not establish that Guion generally outperforms Python or has replaced it.

The retained three-pair builder study is from the historical Veritas identity, not a Guion-identity study. The old scored outcomes remain preserved as history; they should not be represented as Guion recommendations. Exact byte-for-byte replay of the deleted raw Veritas v2 session archive is unavailable, as recorded in the [migration retention manifest](history/migration-retention-manifest.json).

## Source-run and launch speed sprint (2026-10-25)

The source-run cache and repeated-run results are in the [final source-loop comparison](../evaluation/completion/results/source-loop/final-comparison-20261026.json) and its [full dated run](../evaluation/completion/results/source-loop/20261026T052344191561Z.json). This is a separate, newer source-loop cohort; it does not rewrite the frozen installed-artifact report above.

On macOS ARM64 with CPython 3.14.7, seven repeated source runs all hit the project cache. Median source versus prebuilt-artifact times were Richards 73.9 ms vs 67.4 ms, N-body 161.5 ms vs 148.7 ms, and Mandelbrot 18.9 ms vs 15.5 ms. Each source median met the plan's allowance of 20 ms or 25% over its corresponding artifact. Source-run beat Python on N-body and Mandelbrot, but Richards was 73.9 ms vs Python's 72.6 ms, so the required warm-source Python parity gate remains incomplete.

The one uncached sample per workload was 966.8 ms for Richards and 352.1 ms for N-body. These are single observations, not medians, and do not meet the required 50% first-run reduction; the self-hosted compiler remains the dominant cost. Tiny CLI launch measured 10.25 ms median and 11.35 ms p95. Two application cases recorded peak memory and cache size in the detailed report. The Debian 12-compatible ARM64 check built the CLI and ran a source example; this sprint did not rerun the full Debian runtime, release, and 18-application matrix. Prior full Guion target evidence remains linked above.

The full Rust, 195-case language, formatting, strict Clippy, 16 Python harness tests, macOS app acceptance, and Debian smoke checks passed. No release was published.

## Runtime efficiency pass (2026-10-26)

The later [runtime-efficiency summary](../evaluation/completion/results/runtime-efficiency/2026-10-26-summary.md) reports before/after measurements from the merged macOS ARM64 CLI and a focused Rust runtime map-update optimization. Raw reports retain all per-sample outputs and commands: [merged baseline](../evaluation/completion/results/runtime-efficiency/2026-10-26-baseline.json), [candidate](../evaluation/completion/results/runtime-efficiency/2026-10-26-candidate.json), and [five short-source gates](../evaluation/completion/results/runtime-efficiency/2026-10-26-first-source.json).

Fresh uncached source beat the Python reference in each of the seven suite workloads. On 200,000-row JSONL, the uncached-source median decreased from 4.160 to 4.025 seconds (3.2%); the independent-sample bootstrap interval for the candidate/baseline ratio was [0.941, 0.983]. The candidate's measured source/Python ratio was 0.229 [0.226, 0.231]. The prebuilt-artifact median decreased from 4.247 to 4.030 seconds. Phase timing attributes about 4.0 seconds to program execution and about 4 milliseconds to compilation, so further gains on this task require runtime/data-path improvements. The 20,000-row JSONL source median decreased by 5.1%. The five 21-pair first-source gates all passed again.

The optimization did not materially change measured peak RSS: uncached source observed 13.45 MiB before and 13.55 MiB after, while artifact observations were 12.17 and 12.16 MiB. `ru_maxrss` was sampled in untimed direct-child runs on macOS; it does not describe a simultaneous process tree. These are `--no-cache` source runs, not cold-disk tests. The before/after source cohorts were sampled separately, and their direct comparison uses independent resampling, not paired trials. The performance evidence remains specific to these workloads, this host, and CPython 3.14.7; free-threaded Python and broader general-purpose performance remain unverified.

## Supported and unverified systems

Validated release targets:

- macOS on ARM64.
- Debian 12 GNU/Linux on ARM64 with glibc.
- Debian 12 GNU/Linux on x86_64 with glibc.

Other operating systems, CPU architectures, Linux libc variants, free-threaded Python, and package-manager integrations are unverified or out of scope. The Ubuntu x86_64 GitHub Actions job checks source formatting, lint, tests, schemas, the generated agent reference, and the switch-condition ledger; it is not a release-target certification.

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

This repository is organized and validated for a separate pre-release decision. Publishing a release, making the repository public, and changing project-level `AGENTS.md` remain outside this handoff. The original Veritas checkout was retired after its unique small records were preserved.
