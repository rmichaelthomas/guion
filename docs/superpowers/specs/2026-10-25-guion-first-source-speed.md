# Guion first-source execution speed

## Intent

Make a normal `guion run program.gu` faster than launching and running the equivalent Python program on representative small, useful tasks, including the first run when no compiled Guion result exists. Guion's north star is the outcome: people and agents can get useful work done quickly with plain, approachable source code.

The existing source-loop report shows why this matters. Warm source runs are near artifact speed, but uncached Richards and N-body runs take about 967 ms and 352 ms in the latest one-sample observations. Phase records attribute about 843 ms of a Richards sample to executing the self-hosted compiler inside the VM. The CLI workspace already contains a complete Rust reference compiler and a 195-case compiler parity suite. The normal CLI currently runs the self-hosted compiler instead.

## Goals

- Measure at least five short, representative programs end to end: process startup, fresh source compilation, and execution.
- Use paired Guion and pinned Python 3.14.7 runs with identical inputs and verified output.
- Make Guion's median first-source run faster than Python for every primary workload, with a paired 95% interval supporting the win.
- Preserve compiler correctness, diagnostics, imports, runtime grants, source cache validation, self-host compiler bootstrap, and artifact compatibility.
- Keep all build, profiling, fixture, and test scratch under `target/guion-first-source-speed/`; reuse one Cargo target directory and clean it once after retaining compact reports.
- Avoid installing benchmark dependencies or starting containers.

## Architecture direction

Benchmark the existing self-hosted CLI path against the in-process Rust compiler already used for bootstrap and conformance. The likely speed path is to make the optimized Rust compiler the normal source compiler inside the shipped Guion executable; users must not need a separate Rust installation. The Guion-authored compiler remains source-controlled, reproducibly bootstrapped, and covered by compiler-stage parity tests. Keep the previous self-hosted CLI behavior available during implementation long enough to measure it against the new path.

Do not claim a result from old Richards/N-body/Mandelbrot workloads alone. They stay as long-workload regressions alongside small tasks that reflect actual CLI use: tiny output, arithmetic, text/file handling, JSON processing, imported helper code, and SQLite/TOML use. Existing agent examples may supply the file and SQLite tasks.

## Measurement contract

A fresh-source run invokes the CLI with `--no-cache` in a fresh process. Python also starts in a fresh process. Trial order is randomized and paired. Record 21 trials per task, every sample, median, nearest-rank p95, paired median ratio and bootstrap 95% interval, exact commands, binary SHA-256, Python version, platform, peak memory when measurable, and correctness result. The source tree and input fixture exist before timing. Do not include build, fixture setup, or report writing in measured time. The OS file cache is left as-is and reported as uncontrolled; do not label these repeated no-source-cache samples as a cold operating-system cache.

Primary speed gate: every short task's median paired Guion/Python ratio is below 1.0 and its bootstrap 95% upper bound is below 1.0. Any primary workload that misses the gate remains an active optimization target. This pass reruns the three preserved compute workloads (Richards, N-body, and Mandelbrot) as source, artifact, and Python. The older six-workload installed-release report remains historical reference evidence; its parallel CPU and 20,000-row JSONL workloads are not rerun in this focused first-source pass. The new short suite independently covers file/JSON work and SQLite/TOML work. This scope leaves current-build evidence for those two older workloads open.

## Constraints and anti-goals

- Do not weaken source, artifact, seed, package-root, grant, or path validation to gain speed.
- Do not silently use the source-run cache in the fresh-source lane.
- Do not use Python or a separately installed Rust compiler to compile user programs at runtime.
- Do not add third-party benchmark dependencies.
- Do not claim Guion generally beats Python from this finite workload set.
- Keep the Rust stage-zero tool in the build/bootstrap role; any change to the default compiler path must pass the full language parity and bootstrap checks.

## Verification

The final PR must include the workload definitions and exact report, correctness and compiler-stage tests, the 195-case language corpus, source-cache invalidation checks, formatting and strict Clippy, and a clean status after the one repository-local scratch tree is removed. The PR description states any speed target that remains unmet without calling it achieved.
