# Real-task and runtime sprint — macOS ARM64

The source baseline is the shared VM-core merge `9d57434`. Results below compare the named implementations on one Apple Silicon Mac using CPython 3.14.7. A fresh Guion run bypasses Guion's source cache; the operating-system file cache was not controlled.

## Correctness and real tasks

The nine existing Guion applications and the author-calibrated Python reference each passed **127/127 frozen cases**. The acceptance results include error and preservation cases. The seven-pair timing below uses the first initial case for each application, with randomized order and every output checked; it is not a timing of the entire 127-case suite. Times include the case runner's output checks. The Python reference is one shared program with broad imports and a shared argument parser, so these are measurements of these implementations, not the fastest possible Python program for each task. Network fixtures and local machine conditions affect the two network cases.

| Application | Guion, ms | Python, ms | Guion/Python | Guion peak, MiB | Python peak, MiB |
| --- | ---: | ---: | ---: | ---: | ---: |
| SLA report | 16.65 | 101.72 | 0.163 | 8.2 | 31.8 |
| JSONL summary | 16.01 | 98.24 | 0.163 | 8.6 | 31.8 |
| Release notes | 24.55 | 96.64 | 0.251 | 11.6 | 31.6 |
| Calendar conflicts | 235.76 | 365.32 | 0.648 | 12.4 | 41.7 |
| Webhook replay | 144.26 | 818.26 | 0.176 | 15.8 | 37.1 |
| Service health | 531.91 | 628.83 | 0.846 | 10.8 | 37.5 |
| Config migrator | 14.06 | 94.61 | 0.147 | 6.1 | 31.9 |
| Document index | 51.88 | 282.60 | 0.181 | 14.0 | 32.7 |
| Lockfile audit | 16.37 | 94.05 | 0.169 | 10.1 | 32.0 |

Peak memory is one untimed, output-checked observation for the selected case, reported as the largest individual step's child high-water RSS. It is not a simultaneous process-tree peak. The Python calendar reference uses `icalendar==6.3.1`; the dependency and its supporting packages were installed in repository-local scratch from the [pinned requirements](../../requirements-real-task.txt). The Guion programs used no Python bridge. Full cases, samples, confidence intervals, and scope are in [acceptance](nine-task-final-acceptance.json), [timings](nine-task-timings-final.json), and [memory](nine-task-memory.json).

A separate seven-pair comparison checked uncached source, a prewarmed source cache, and a verified source-free application package for each selected case. The cache was also tested after copying it to a new project directory for all nine apps. For example, lockfile auditing took median **17.5 ms uncached**, **17.2 ms cached**, and **27.2 ms packaged**; webhook replay took **141.2 ms**, **137.8 ms**, and **201.5 ms** respectively. The package contains its own runtime and adds launch overhead; webhook replay starts multiple processes in its case. The [delivery-mode report](delivery-modes.json) contains all four lanes and [cache verification](cache-relocation.json).

## Richards

In 15 randomized, output-checked fresh-source pairs, Guion's new build took a median **45.67 ms** versus **70.16 ms** for Python. The median paired Guion/Python ratio was **0.653**, with a 95% interval of **0.649–0.656**. Against the post-VM-core Guion baseline in the same cohort, the ratio was **0.966**, about **3.4% less time** (interval 0.952–0.973). The stretch target of 0.60 was not reached. [Full result](richards-final.json).

The change groups the small integer-list descriptors used by generated code into fixed-capacity chunks. Existing list values keep stable addresses and copy-on-write semantics. Comparing the same two binaries across 25 Richards executions per process, the new/old median ratio was **0.873**, about **12.7% less time** (95% interval 0.859–0.881). That controlled change supports the conclusion that allocation of small list descriptors is a material runtime cost; it does not measure branch-misprediction costs. [Repeated-run result](richards-repeated-final.json).

The median peak RSS increased **1.5%** for fresh source and **0.5%** for the 25-execution lane, both under the 5% guard. All five established short-source Python comparisons passed for the final binary. [Memory](richards-final.json) and [short-source samples](short-source-final.json).

## AllHereHTML and text

The pinned Guion AllHereHTML builder passes its 19 tests, reproduces the expected page byte-for-byte, and preserves the old output when the template is missing or contains a script. It is now a CI regression. Before the text change, splitting a long line and joining a roughly doubled page exhausted the default cumulative allocation budget. Guion now combines text pieces in balanced groups. The same long-line and many-field regression passes with Unicode and empty fields, and the original site still matches its expected output.

For the original site, separately instrumented runs recorded estimated guest allocation-budget charges of **121.6 MB before** and **66.3 MB after**, a reduction of about **45.5%**. These are Guion accounting estimates, not measured physical allocations or RSS. A scaling run on the optimized build generated and checked 42 KB, 85 KB, and 170 KB templates in median fresh-source times of approximately **0.34 s, 0.65 s, and 1.28 s**, respectively. [Profile counters](allhere-runtime-profile.json) and [scaling samples](allherehtml-scaling-final.json).

A later runtime follow-up cached immutable ASCII scalar text values. Against the immediately preceding build, the 20,000-row JSONL workload improved from **364.5 ms to 352.8 ms** (paired ratio 0.969; 95% interval 0.967–0.971; seven of seven pairs faster), and AllHereHTML improved by **1.9–2.1%** across the same three input sizes. The complete application acceptance rerun passed **127/127 cases** in both Guion and Python. The detailed [ASCII scalar report](../runtime-efficiency/2026-10-27-ascii-text-scalar-cache.md) includes direct nine-task comparisons and limitations.

A map update follow-up avoids rebuilding the stored string key when `map_set` replaces an existing value. In a nine-app direct comparison, release-notes improved by **4.6%** (paired ratio 0.954; 95% interval 0.926–0.984; seven of seven wins), while the other app results—including the 20,000-row JSONL workload—showed no reliable change. Every run passed its output check, and all **127/127** acceptance cases passed for both implementations. A fresh post-commit comparison remained faster than Python on all nine selected cases (Guion/Python ratios **0.136–0.837**). Three fresh agent evaluations again chose Guion (**3/3**), unchanged at the maximum score. See the [map-key report](../runtime-efficiency/2026-10-27-map-key-reuse.md), [post-commit timings](2026-10-27-map-key-reuse-postcommit-nine-task-python-comparison.json), and [preference-study summary](agent-choice/summary.json).

## Installation

The final macOS ARM64 core archive measured **5,442,799 bytes** compressed. The local release checks passed installation, relocated execution, standard-library discovery, integrity rejection, and operation with a PATH that did not provide Python or uv. The Linux targets need their own current-build runs; the earlier three-target validation does not prove this new build passed them. [macOS checks](macos-release-checks.json).

In one local setup observation, extracting the final core archive took **0.23 seconds** and installing it took **0.17 seconds**; its installed files totaled **15.1 MB**. Creating a Python virtual environment from an already installed interpreter took **1.89 seconds**; installing the pinned calendar dependencies from already downloaded wheels took another **0.56 seconds**. These are single warm-cache observations; they exclude downloading the interpreter or packages and are not a complete cross-platform install comparison. Each verified Guion application package measured about **12.4–12.5 MB**, including its runtime. [Setup, staging, and per-application sizes](task-setup.json).

## Browser VM

The merged shared VM built for WebAssembly. A Node WebAssembly smoke test used the current AllHereHTML browser compiler to compile a program and execute it on the freshly built Guion VM; it also checked rejection of project imports and unsupported filesystem access. This is an ABI and execution check, not a graphical-browser test. [Smoke result](wasm-browser-smoke.json).

## Agent choice after use

After commit `709b2d5`, three sequential, ephemeral Codex evaluations reran the matched health-digest, lockfile-auditor, and webhook-replay acceptance cases in both languages, inspected the current timing data, and chose a language for a related new task. All three again chose **Guion**, so the result remains **3/3**, unchanged from the previous run and already at the score ceiling. The new acceptance runs passed all **10**, **17**, and **9** cases, respectively. Guion measured about **1.2×**, **5.5×**, and **5.1×** faster than the Python reference on the selected named cases. The study is exploratory and unblinded; these three related evaluations do not represent a broad adoption sample. The [latest study record](agent-choice/summary.json) includes current timing inputs and saved decisions for [health](agent-choice/health-digest-choice.json), [lockfile audit](agent-choice/lockfile-auditor-choice.json), and [webhook replay](agent-choice/webhook-replay-choice.json).

## Interpretation

These are meaningful gains on named programs and conditions. They do not establish that Guion is faster than Python on arbitrary tasks, that the Python alternatives are individually optimized, or that agents generally will choose Guion.
