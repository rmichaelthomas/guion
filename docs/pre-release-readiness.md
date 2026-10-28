# Guion validation and readiness

**Validation record:** The three-target cohort documented below used Guion 0.1.0 / artifact format 39. Current merged main uses artifact format 44; the latest lockfile scale, speed, and RAM result is recorded in the [October 27 lockfile report](../evaluation/completion/results/real-task-sprint/2026-10-27-lockfile-scale-ram-package-limit.md). The older three-target matrix does not by itself validate format 44.

## What passed

The earlier three-target validation cohort lives under [`evaluation/completion/results/guion-final/`](../evaluation/completion/results/guion-final/). Its [`README`](../evaluation/completion/results/guion-final/README.md) maps each aggregate to its detailed target records. That matrix passed on macOS ARM64 and Debian 12 GNU/Linux on ARM64 and x86_64, all with glibc:

- Three-stage self-hosted compiler bootstrap and compiler-stage identity checks across all three targets, alongside the full language conformance corpus.
- The full 195-case language conformance corpus, runtime execution/HTTP/lifecycle checks, and standard-library data and service suites.
- All nine retained regression applications in initial and follow-up forms: 18 of 18 reports passed on each target, or 54 of 54 individual reports in total. No application report failed.
- Python-free core dependency checks, dependency and installed-product lifecycle checks, release archive contents and license inventory, clean-room installation, relocation, integrity rejection, and dynamic-library audits.
- A Guion-identity installed-artifact performance cohort with correctness checks, paired randomized samples, and percentile intervals.
- Formatting, strict Clippy, Rust unit/integration suites, schema validation, CI workflow validation, exact outgoing-tree secret scan, and package/license audits.

That cohort's runtime, application, lifecycle, and release aggregates list all three targets as completed with no skipped target. Individual target-attempt JSON files can say `incomplete` because each one contains only one side of the aggregate; use the matching aggregate to read the combined gate. The detailed target evidence is preserved under [`target-runs/`](../evaluation/completion/results/guion-final/target-runs/).

## Current format-44 managed native-wheel package (2026-10-27)

On macOS ARM64, the focused CLI integration test builds a format-44 source-free package with managed CPython 3.14.8, uv 0.13.4, NumPy 2.5.4, orjson 3.13.0, and requests 2.32.5. It checks dependency licenses and native-image attribution, verifies integrity failures, relocates the package into a path with spaces, and launches twice with host Python and uv settings removed. The embedded Python runtime is 68,121,972 bytes and the offline wheel cache is 23,433,579 bytes. The exact gate and limits are recorded in the [package report](../evaluation/completion/results/real-task-sprint/2026-10-27-current-format-managed-python-package.md).

The separate 100,000-row NumPy/orjson analysis passes as a managed format-44 package on macOS ARM64 and Debian 12 ARM64 and x86_64. The input produced 13 groups and total 2,500,116,287; each 1,256-byte report matched the standard-library reference byte-for-byte. All three source-free package directories relocated to a path with spaces and Unicode and launched twice with external Python/uv settings removed. The macOS directory measured 157,585,367 bytes; Debian ARM64 measured 244,199,858 bytes; Debian x86_64 measured 274,853,607 bytes. The x86_64 bundle contains a 101,961,520-byte CPython runtime and a 59,505,505-byte offline cache. The [macOS result](../evaluation/completion/results/real-task-sprint/2026-10-27-current-format-managed-package-analysis.md), [Debian ARM64 result](../evaluation/completion/results/real-task-sprint/2026-10-27-current-format-managed-package-analysis-debian-aarch64.md), and [Debian x86_64 result](../evaluation/completion/results/real-task-sprint/2026-10-27-current-format-managed-package-analysis-debian-x86_64.md) include hashes, timings, and cleanup details. An initial x86_64 attempt stopped during QEMU `ldd` inspection; the accepted retry used Rosetta in the same VM, as recorded in the [attempt report](../evaluation/completion/results/real-task-sprint/2026-10-27-current-format-managed-package-analysis-debian-x86_64-attempt.md). The earlier three-target cohort used format 39.

## Current-format Feed Digest packages (2026-10-27 and 2026-10-28)

The current-format `evaluation/development/guion_feed_digest` app uses managed pure-Python feedparser 6.0.14 and feedparser-sgmllib 2.1.0. On macOS ARM64, Debian 12 ARM64, and Debian 12 x86_64, format-44 packages were verified before and after relocation to paths with spaces and Unicode. The uncompressed directories measure 172,422,430 bytes on macOS, 128,874,649 bytes on Debian ARM64, and 158,943,069 bytes on Debian x86_64. Each includes managed CPython 3.14.8, uv 0.13.4, complete dependency license records, and no native extensions from these Python packages. Six source cases and six relocated-package cases per target matched the independent Python implementation byte-for-byte. Debian ARM64 ran natively in the existing Colima VM without Rosetta; Debian x86_64 used Rosetta and did not use QEMU. The suites ran with dependency resolution offline; their only network traffic was to the loopback fixture server. See the [macOS report](../evaluation/completion/results/real-task-sprint/2026-10-27-feed-digest-managed-package.md) and [raw JSON](../evaluation/completion/results/real-task-sprint/2026-10-27-feed-digest-managed-package.json), the [Debian ARM64 report](../evaluation/completion/results/real-task-sprint/2026-10-28-feed-digest-managed-package-debian-aarch64.md) and [raw JSON](../evaluation/completion/results/real-task-sprint/2026-10-28-feed-digest-managed-package-debian-aarch64.json), and the [Debian x86_64 report](../evaluation/completion/results/real-task-sprint/2026-10-28-feed-digest-managed-package-debian-x86_64.md) and [raw JSON](../evaluation/completion/results/real-task-sprint/2026-10-28-feed-digest-managed-package-debian-x86_64.json). This extends package evidence to two Linux architectures for one Python library; it does not establish broad package compatibility or a speed result.

## Performance result and boundary

The [real-task and runtime sprint](../evaluation/completion/results/real-task-sprint/summary.md) contains several historical cohorts. Its earlier post-VM-core Richards result was 45.67 ms for Guion versus 70.16 ms for CPython 3.14.7; the current-main measurement below supersedes that result. The older three-target records do not by themselves establish that a later build passed on all three targets.

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

## Current-main performance and application checkpoint (2026-10-27)

The current merged main at commit `6b5e5aba` was rebuilt with artifact format 44 and compared on macOS ARM64 with CPython 3.14.7. Richards uncached source measured **27.46 ms vs. 72.92 ms**, ratio **0.375** (95% interval **0.370–0.379**), passing the Richards 0.60 target. All five 21-pair short-source gates passed. The output-checked 20k/200k JSONL and CPU cases passed, as did **127/127** frozen application cases in both languages. Seven-pair comparisons favored Guion across all nine selected everyday applications. The detailed [checkpoint report](../evaluation/completion/results/real-task-sprint/2026-10-27-current-main-outcome-checkpoint.md) links raw samples and records memory and measurement limits. This is current-main macOS evidence; it does not update the earlier three-target format-39 cohort.

## Python bridge serialization pass (2026-10-27)

A same-host comparison interleaved pre-change and candidate release binaries on the 100,000-row NumPy/orjson app. Across 15 randomized pairs, the candidate/pre-change ratio was **1.012** (95% interval **0.985–1.025**), so the run did not establish a speed improvement or regression. The candidate remains **1.129×** slower than the same package function run directly in Python and **1.406×** slower than the standard-library reference. Output matched byte-for-byte on every sample. The implementation reduces intermediate copies, but peak memory was not measured. See the [report](../evaluation/completion/results/real-task-sprint/2026-10-27-python-bridge-serialization.md) and [raw samples](../evaluation/completion/results/real-task-sprint/2026-10-27-python-bridge-serialization.json).

## Python bridge attachment allocation follow-up (2026-10-28)

The binary frame parser now extracts request attachments through a zero-copy memory view and makes one immutable `bytes` copy. A source-level allocation regression first failed at a 10,202,573-byte peak for the 5,100,867-byte attachment; after the change it stays under 7,651,300 bytes and preserves the Python `bytes` type. Fifteen randomized fresh-process pairs passed exact output checks. Guion `Bytes` measured 138.49 ms versus 136.05 ms for the same package-backed Python function; the paired ratio is **1.020** (95% interval **0.988–1.031**), which does not establish a winner. The standard-library reference remains faster at ratio **1.199** (95% interval **1.177–1.225**). Host CPU attribution and whole-process peak memory remain unmeasured. See the [plain-English report](../evaluation/completion/results/real-task-sprint/2026-10-28-python-bridge-single-copy.md) and [raw samples](../evaluation/completion/results/real-task-sprint/2026-10-28-python-bridge-single-copy.json).

## Python bridge no-op call with a 5.1 MB payload (2026-10-28)

A four-lane no-op `Bytes` control imports NumPy and orjson and returns only the input length. Across 15 randomized fresh-process pairs, Guion and Python were at parity for an empty value (ratio **0.993**, 95% interval **0.979–1.019**); for the 5,100,867-byte value Guion measured 83.15 ms vs. Python at 80.68 ms (ratio **1.043**, **1.004–1.050**). The relative payload penalty was **1.045** (**1.001–1.088**); every output matched. This locates the difference in payload-scaled work but does not attribute CPU among file reads, pipe transfer, frame parsing, and attachment conversion. See the [control report](../evaluation/completion/results/real-task-sprint/2026-10-28-python-bridge-startup-sizes.md) and [raw samples](../evaluation/completion/results/real-task-sprint/2026-10-28-python-bridge-startup-sizes.json). The earlier [single-size probe](../evaluation/completion/results/real-task-sprint/2026-10-28-python-bridge-startup-5mb.md) remains a separate cohort.

## Python bridge frame-reader allocation follow-up (2026-10-28)

Changing the sidecar reader to preallocate the known frame size and use `readinto` reduced the isolated peak for a 5,100,867-byte frame from **10,201,824 bytes** to **5,101,732 bytes**. The change preserves short-read, clean EOF, and truncated-frame behavior. The full application measured a **1.019** Guion/Python ratio (95% interval **0.995–1.046**) and the no-op payload penalty measured **1.031** (**1.019–1.038**). Independent-cohort bootstrap comparisons show no measurable runtime change. See the [frame-reader report](../evaluation/completion/results/real-task-sprint/2026-10-28-python-bridge-readinto.md) and its [application](../evaluation/completion/results/real-task-sprint/2026-10-28-python-bridge-readinto-application.json) and [no-op](../evaluation/completion/results/real-task-sprint/2026-10-28-python-bridge-readinto-startup.json) samples.

## Python bridge direct-read experiment (2026-10-28)

A direct `read()` fast path for complete frames did not show a measurable speed change from the `readinto` version. The full application candidate/readinto ratio was **1.018** (95% interval **0.967–1.043**); the no-op payload-penalty ratio was **1.003** (**0.975–1.072**). Peak allocation differed by only 520 bytes, so the extra branch was reverted. The next measured target is streaming attachment extraction; see the [experiment report](../evaluation/completion/results/real-task-sprint/2026-10-28-python-bridge-direct-read.md).

## Source-run and launch speed sprint (2026-10-25)

The source-run cache and repeated-run results are in the [final source-loop comparison](../evaluation/completion/results/source-loop/final-comparison-20261026.json) and its [full dated run](../evaluation/completion/results/source-loop/20261026T052344191561Z.json). This is a separate, newer source-loop cohort; it does not rewrite the frozen installed-artifact report above.

On macOS ARM64 with CPython 3.14.7, seven repeated source runs all hit the project cache. Median source versus prebuilt-artifact times were Richards 73.9 ms vs 67.4 ms, N-body 161.5 ms vs 148.7 ms, and Mandelbrot 18.9 ms vs 15.5 ms. Each source median met the plan's allowance of 20 ms or 25% over its corresponding artifact. Source-run beat Python on N-body and Mandelbrot, but Richards was 73.9 ms vs Python's 72.6 ms, so the required warm-source Python parity gate remains incomplete.

The one uncached sample per workload was 966.8 ms for Richards and 352.1 ms for N-body. These are single observations, not medians, and do not meet the required 50% first-run reduction; the self-hosted compiler remains the dominant cost. Tiny CLI launch measured 10.25 ms median and 11.35 ms p95. Two application cases recorded peak memory and cache size in the detailed report. The Debian 12-compatible ARM64 check built the CLI and ran a source example; this sprint did not rerun the full Debian runtime, release, and 18-application matrix. Prior full Guion target evidence remains linked above.

The full Rust, 195-case language, formatting, strict Clippy, 16 Python harness tests, macOS application acceptance, and Debian smoke checks passed for that earlier cohort.

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

For source checkout development, follow [Getting started](getting-started.md). For a locally available host archive, extract it and run `sh install.sh`; by default this installs under `~/.local`. Pass a prefix, such as `sh install.sh /chosen/prefix`, to select another location. Add that prefix's `bin` directory to `PATH` if you want to run `guion` from any directory. The installer does not change shell startup files.

To update, install a newer version from its extracted archive using the same prefix. Each version gets its own directory under `share/guion/`; the managed launcher points to the newest installation, and previous versions remain available. The installer refuses to overwrite the same version directory. To remove Guion, use the prefix chosen at install time, remove only the Guion-managed `bin/guion` launcher and the specific version directory under `share/guion/`, and leave unrelated files in that prefix alone. If another program replaced the launcher, do not remove it as Guion's launcher.

This installer supports the packaged host only. The archives and their exact SHA-256 values and sizes are recorded by [`release.json`](../evaluation/completion/results/guion-final/release.json) and the per-target release records. Third-party license texts, including the retained libical LGPL text, are included in the distribution and indexed by [`THIRD-PARTY-NOTICES.md`](../THIRD-PARTY-NOTICES.md).

## Identity and retained records

The earlier three-target reports record the compiler seed, runtime, standard-library, and artifact-schema hashes for artifact format 39 with host ABI 1. Current merged main uses artifact format 44. The Rust workspace and embedded compiler/runtime are source; Rust is not required by an installed core distribution, and Python interoperability is optional.

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
You are working in the Guion programming-language repository. First read docs/agents/start-here.md, docs/agents/index.json, language/spec.md, docs/status.md, and docs/pre-release-readiness.md. Treat the language specification and checked-in schemas as authoritative. Keep claims tied to retained reports, preserve historical Veritas evidence without relabeling it, and run the repository's documented checks for any behavior change. Follow the project owner's direction on release and repository visibility.
```

## Validation handoff

This document records validated behavior and its evidence limits. The current product-outcome status is in [`docs/status.md`](status.md), and the original Veritas checkout was retired after its unique small records were preserved.
