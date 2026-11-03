# Task-class evidence and the dynamic-data gap

Branch `codex/guion-speed-closure`. The north star is to earn assistant recommendations over Python for specific task classes through comparative evidence, so results are reported per task class and not as one ratio.

## Per-class position

| Task class | Guion/Python ratio | Source | Notes |
| --- | ---: | --- | --- |
| Short-source launch, JSON file, imported helper, SQLite/TOML | 0.18–0.24 | [current-main checkpoint](../../completion/results/real-task-sprint/2026-10-27-current-main-outcome-checkpoint.md) | 21 randomized pairs each; macOS ARM64, format 44 |
| Compute (Richards, N-body, Mandelbrot) | 0.14–0.38 | same | CPython 3.14.7 |
| Parallel CPU (20k, 200k items) | 0.12–0.14 | same | serial Python reference |
| Nine everyday applications | 0.14–0.85 | same | 127/127 cases pass in both |
| JSONL summary, recorded corpus | 0.21 | same | different corpus and Python program than the sprint gate below; not comparable |
| **JSONL summary, sprint gate (frozen 200k-row corpus)** | **see the table below** | this branch, pinned Linux container, PGO CPython 3.14.7, 9 interleaved trials, `--services` filter, output checked against Python | the earlier 1.19 and 1.45 figures ran without the services filter and are not comparable |

The compute and everyday-application classes already favor Guion. The open gap is the dynamic-data class: per-row work made of `Json`, `Text`, `Map`, `Option` and `Result` values.

## Current position: plain reference counts (2026-11-03, build `547f07c`)

Values now count references with plain loads and stores while one thread holds them, and atomically only after they are shared with other threads (`crates/model/src/shared.rs`). The new [A/B workflow](../../../.github/workflows/speed-ab.yml) gated the branch and its base (`e1915f2`) one after the other on the same GitHub runner, 15 trials each, with profile-guided builds and the standalone CPython 3.14.7 ([raw reports](results/speed-ab-hybrid-refcount-20261103/)).

| Host | Change in Guion time | Worst 200k upper bound, base → branch | Gate |
|---|---|---|---|
| ARM64 Neoverse N2 (GitHub) | −10% to −15% on every lane | 1.090 → 0.945 | failed → **passes** |
| x86_64 AMD EPYC 7763 (GitHub) | −2% to +4%, within noise | 1.049 → 1.094 | fails either way |
| Apple M1 Pro (macOS, Homebrew Python) | none (against the earlier run) | 0.708 → 0.710 | passes either way ([report](results/speed-gate-macos-aarch64-hybrid-refcount-20261103.json)) |

That confirms the ARM64 profile: atomic reference counts were 15–20% of Guion's time there, and removing them closes the 200k gap on ARM servers. It does not move x86_64. On the EPYC, uncontended `lock`-prefixed updates are cheap, so the x86_64 gap at 200k rows (text and CSV about 1.03–1.09) comes from elsewhere.

**Allocation quota (build `f4ccd14`).** A per-source-line x86_64 profile ([raw](results/gate-profiles-lines-20261103/)) put the allocation-quota compare-and-swap at 3–8% of gate time. Python incurs about as many page faults as Guion on the text workload, so page faults are not the gap. The quota now uses plain loads and stores unless parallel workers share it. A second same-runner A/B against `e1915f2` covers both changes together ([raw](results/speed-ab-allocation-pool-20261103/)):

| Host | Change in Guion time | Worst 200k upper bound, base → branch | Gate |
|---|---|---|---|
| ARM64 Neoverse N2 | −10% to −19% (JSONL −18% to −19%) | 1.088 → 0.918 | failed → **passes** |
| x86_64 AMD EPYC 7763 | text and JSONL −1% to −2%, CSV +0% to +3% (noise) | 1.049 → 1.040 | fails |

On x86_64 the quota change recovered far less than its sampled share suggested; sampling attributes the cost of the instructions around a locked operation to it. The x86_64 gap that remains is 2–4% on text and CSV at 200k rows (JSONL now passes there, worst 0.971). The profile shows no single remaining hotspot: the interpreter loop's value pushes, drops and stores dominate. Text `len()` counts scalars on each call, which is about 3% of text time.

**Remembered scalar counts (build `6604bdd`).** Text now remembers its scalar count once counted, and ASCII text is indexed by byte. Walking a 120,000-character text by index, `while position < len(document)` with `document[position]`, dropped from 1.48 s to 0.01 s on the M1 (CPython 3.14.7: 0.03 s); before, each `len` and each index decoded the text from the start, so the loop was quadratic. The gate does not move: its text workload calls `len` once on each freshly trimmed field, so there is nothing to remember. A variant that also marked ASCII fields during split and trimmed them byte-wise showed no gain ([raw](results/speed-ab-text-scalar-count-20261103/)) and was removed. The same-runner A/B of the kept change against `e1915f2` ([raw](results/speed-ab-scalar-count-only-20261103/)) matches the previous one within run-to-run noise: ARM64 −10% to −20%, worst 200k upper bound 0.932 (passes); x86_64 text +1%, CSV −2% to +1%, JSONL −4% to −7%, worst 1.078 (text source; fails). The x86_64 base itself measured 1.057 on this run against 1.047 on the last, so x86_64 runners differ by about 1% between runs even on the same CPU model.

**Text operations in the register tier (builds `dc132b0`, `a57cd7d`, `65319dc`).** Timing the text workload's phases on the M1 at 200k rows, with startup removed (Guion 5 ms, CPython 24 ms), showed Guion was not ahead anywhere except replace and find (25 ms against 50 ms). It was behind building the lines (53 ms against 37 ms) and in the split-and-trim loop (62 ms against 53 ms). The cause: text `+`, `text_trim`, `text_split`, `text_find` and `list_push` had no register form, so each one left the register tier, rebuilt the stack, ran on the stack interpreter and re-entered, several times per row. They now run in the register tier, reading operands where they live and charging exactly what the plain instructions charge. Anything unusual (limits, errors, empty delimiters, `set s = s + piece` appends) still goes to the stack interpreter. A test sweeps instruction and allocation limits and requires identical results, traces and remaining budgets in both tiers. `text_slice` also walked the text from its start on every call: a 120,000-character sliding-window walk took 4.55 s and now takes 0.015 s (CPython 0.044 s), and text index and slice now index ASCII text by byte. Same-runner A/B of `dc132b0` against `e1915f2` ([raw](results/speed-ab-text-register-ops-20261103/)): text −8% on x86_64 (AMD EPYC 7763; 200k upper bound 1.04 to 0.96, now passing) and −21% on ARM64 (0.83); JSONL −2% to −4% on x86_64 and −13% to −15% on ARM64. The CSV workload, which uses none of these operations, got 3–5% slower on x86_64 (200k upper bound 1.088, failing) and gained about 6% less on ARM64 (0.996). Locally it was 2% slower, from the larger dispatch loop. `65319dc` moves each text operation into its own out-of-line function. Locally CSV is then within 1% of the earlier build (noise) and text keeps a 12.4% gain of the 13%. A same-runner A/B of `f4a67ec` against `e1915f2`, run after the repository went public, confirms the fix ([raw](results/speed-ab-text-ops-out-of-line-public-20261103/); public repositories get 4-CPU standard runners, and both sides ran on the same one). On x86_64 (AMD EPYC 7763), text is −10% (200k upper bound 0.957–0.972, passing), JSONL is −1% to −4% (0.973 worst, passing), and CSV is back to −1% to +2% of its base (1.028–1.041, still failing, as its base did at 1.019–1.035). On ARM64, every lane passes: text −21%, JSONL −15% to −17%, CSV −15% (worst 200k upper bound 0.926). **Position:** ARM64 meets the gate on all three workloads; x86_64 meets it on text and JSONL and misses on CSV at 200k by about 3–4%.

## Earlier position: other hosts and other Python builds (2026-11-02, build `e1915f2`)

This section qualifies the gate result below. All earlier gate runs used one machine: the pinned Linux container runs under Docker on an Apple M1 Pro, against CPython 3.14.7 built from source with PGO and LTO. The portable [`run_speed_gate.py`](run_speed_gate.py) builds the same text, JSONL and CSV fixtures from the repository. With the container's Python it reproduces the earlier container figures (200k upper bounds 0.763–0.905), so the harness is not the cause of the differences below. Every run used 15 interleaved, output-checked trials of the profile-guided binary.

**The gate holds at 20k rows on every host and Python build tried. At 200k rows it depends on the host and on the Python build.**

| Host | CPython 3.14.7 build | 20k, worst upper 95% | 200k, worst upper 95% (text / JSONL / CSV) | Gate |
| --- | --- | ---: | --- | --- |
| macOS, Apple M1 Pro | Homebrew (PGO+LTO) | 0.624 | 0.746 / 0.707 / 0.708 | met |
| Linux container on the M1 | source-built (PGO+LTO), the gate of record | (earlier: 0.689) | 0.905 / 0.795 / 0.886 | met |
| Linux container on the M1 | uv standalone build | not run | 0.932 / 0.889 / **1.042** | missed (CSV artifact) |
| GitHub `ubuntu-24.04`, x86_64, 4 CPUs | uv standalone build | 0.824 | **1.074** / 0.955 / **1.030** | missed |
| GitHub `ubuntu-24.04-arm`, ARM64, 4 CPUs | uv standalone build | 0.792 | **1.044** / **1.039** / **1.096** | missed |
| GitHub `ubuntu-24.04`, x86_64, Intel Xeon Platinum 8573C, 2 CPUs (second run) | uv standalone build | 0.921 | **1.428** / **1.116** / **1.223** | missed |
| GitHub `ubuntu-24.04-arm`, ARM64 Neoverse N2, 2 CPUs (second run) | uv standalone build | 0.779 | **1.044** / **1.036** / **1.087** | missed |

Two separate effects show up:

- **The Python build.** On the same container, the standalone CPython that `uv python install` provides ran the 200k workloads 6% (text), 11% (JSONL) and 15% (CSV) faster than the source-built one, while Guion's times stayed the same. The gate of record was therefore measured against a slower Python than most builders, and the recommendation campaign, would use. The campaign itself pins a uv-managed CPython. The difference is CPython 3.14's tail-calling interpreter: the standalone build is compiled with Clang and has it (`Py_TAIL_CALL_INTERP = 1`), while the container's GCC build and the Homebrew build on macOS do not.
- **The CPU.** On GitHub's x86_64 and ARM64 server runners, Guion's 200k lead disappears: median ratios are 0.92–1.06 on x86_64 and 1.02–1.09 on ARM64, against 0.85–0.90 on the M1 with the same Python build (CSV artifact aside). At 20k Guion still leads on those runners, with median ratios of 0.67–0.79. The two ARM64 runs agree within 0.01. The two x86_64 runs landed on different machines and disagree: the Intel Xeon run was 1.12–1.43 at 200k and up to 0.92 at 20k, so x86_64 results vary with the CPU model, and Intel is the weakest host measured. The cause has not been profiled on those hosts. It is not a larger memory footprint: at 200k rows Guion's peak resident memory is equal to or below Python's (text 67 MB against 70 MB, JSONL 20 MB against 73 MB, CSV 20 MB each, in the container).

**Profile on the server runners** ([`profile_gate_workloads.py`](profile_gate_workloads.py), sampled CPU time at 200k rows; GitHub's virtual machines expose no hardware counters; [raw profiles](results/gate-profiles-20261103/)). On ARM64 Neoverse N2, 15–20% of Guion's time is in `__aarch64_ldadd8_rel`, `__aarch64_ldadd8_relax` and `__aarch64_cas8_*`: text 20%, CSV 18%, JSONL 20%. These are the atomic reference-count updates behind every `Value` clone and drop, made through Rust's outlined-atomics helpers. On x86_64 the same `lock`-prefixed updates are inlined into the interpreter loop (65–68% of text and CSV time), so they don't show separately. CPython 3.14's default build updates reference counts without atomics. Atomic updates are cheap on the Apple M1 and cost more on these server cores. That fits Guion's lead holding on the M1 and shrinking on servers, and it points to non-atomic reference counts on values that stay on one thread as the next speed step. The text workload also spends about 10% of its time in kernel page-fault handling on x86_64.

Also open: in the container, the CSV artifact lane (0.103 s) ran slower than the source lane with `--no-cache` (0.093 s) under both Python builds. Earlier container runs of the old CSV script did not show this, and macOS doesn't either.

**Position:** on the M1, Guion beats Python on all three workloads at both sizes, against either Python build, except one CSV lane at 200k. On server-class CPUs it beats Python at 20k. At 200k it is 4–10% slower on ARM64 Neoverse N2, and up to 43% slower on an Intel Xeon. Before stating a speed claim to builders, the gate should run against the uv standalone CPython, and on at least one x86_64 host.

Raw results: [macOS](results/speed-gate-macos-aarch64-20261102.json), [container, source-built CPython](results/speed-gate-container-arm64-source-cpython-20261102.json), [container, standalone CPython](results/speed-gate-container-arm64-standalone-cpython-20261102.json), [GitHub x86_64](results/speed-gate-github-x86_64-20261102.json), [GitHub ARM64](results/speed-gate-github-arm64-20261102.json). [GitHub x86_64, Intel Xeon](results/speed-gate-github-x86_64-xeon-8573c-20261102.json), [GitHub ARM64, Neoverse N2](results/speed-gate-github-arm64-neoverse-n2-20261102.json). The first GitHub runs predate recording the CPU model in the report.

## Earlier position: register form and hashed maps

This section supersedes the ratios in the next two; the section above qualifies it for other hosts and Python builds. Pinned Linux ARM64 container, PGO CPython 3.14.7, profile-guided build of `190b624`; every run's output matched. Gate scripts ran 15 interleaved trials, the others as noted. This is one platform. The release matrix and macOS have not been re-run.

**Gate status: met on both workloads.** The paired 95% upper Guion/Python ratio is below 1.0 in the source and artifact lanes at 20k and 200k rows, on the programs as written.

| Workload | Rows | Lane | Previous build | This build | Upper 95% |
| --- | ---: | --- | ---: | ---: | ---: |
| JSONL summary, gate script | 20k | source | 0.877 | 0.724 | 0.734 |
| JSONL summary, gate script | 20k | artifact | 0.834 | 0.705 | 0.709 |
| JSONL summary, gate script | 200k | source | 1.035 | 0.859 | 0.880 |
| JSONL summary, gate script | 200k | artifact | 1.019 | 0.858 | 0.867 |
| CSV ledger, paired script | 20k | source | | 0.686 | 0.694 |
| CSV ledger, paired script | 20k | artifact | | 0.646 | 0.661 |
| CSV ledger, paired script | 200k | source | | 0.916 | 0.930 |
| CSV ledger, paired script | 200k | artifact | 1.790 | 0.893 | 0.906 |

The 1.790 CSV figure came from the interleaved comparison, not this script.

| Other workload | Previous build | This build | Guion/Python |
| --- | ---: | ---: | ---: |
| Integer loop kept in the interpreter by a list print (9 trials) | 1214 ms | 530 ms | 0.745 (0.738-0.746); was about 1.7 |
| Integer loop, native | 89 ms | 89 ms | 0.124 |
| Fused JSONL 200k, artifact | | 613 ms | 0.845 (upper 0.868) |

The full suite now reports `performance_status: passed` and `correctness_status: passed`. JSONL 200k against Python is 0.861 (0.845-0.874) from the artifact and 0.855 (0.850-0.867) from source; compute and parallel ratios stay between 0.06 and 0.53. 540 workspace tests pass.

Raw results: [JSONL gate](results/jsonl-register-pass-paired-15.json), [CSV](results/csv-register-pass-paired-15.json), [interleaved](results/register-pass-vs-previous-interleaved-9.json), [interpreter loop](results/vm-loop-register-pass-9.json), [suite](results/suite-format50-container-register-pass.json).

### What changed

- **Register form.** On first call, each function's stack bytecode is translated into register instructions that read locals and constants directly. Load, store and constant instructions were 47-49% of dispatches on these workloads, which is the cost [Shi et al.](https://usenix.org/legacy/events/vee05/full_papers/p153-yunhe.pdf) and [Lua 5](https://www.lua.org/doc/jucs05.pdf) describe removing. Each register instruction stands for an exact range of stack instructions and charges their exact fuel and allocation. Anything unusual changes nothing, rebuilds the stack and returns to the stack interpreter at the same instruction. Tests compare both paths at every instruction limit in a range and at stepped allocation limits. Parallel workers stay on the stack interpreter, and profiled runs use it too.
- **Specialization.** Guards and chains fixed at translation cover the common shapes: small-integer arithmetic and division, JSON field reads into integers and text, short integer parsing, field reads through `Unwrap`, and `Some`/`None` wrapper calls. This is a static form of [PEP 659](https://peps.python.org/659). **Adaptive quickening, which rewrites instructions from observed types at run time, was not built or measured**, because the static form closed the gates.
- **Hashed maps.** `Map` is a SipHash table with randomly seeded keys, as in Python, instead of a `BTreeMap`. Observable order stays sorted by key, so sorted iteration now sorts when a map is walked. A faster non-cryptographic hash was left out to keep protection against crafted collisions. The key hash is reused across consecutive lookups, and keys are shared, so copying a map copies no key bytes.
- **Host calls and limits.** Host operations run inside the register loop without flushing fuel, and a CSV row checks its limits from its known shape. The local fuel reservation grew from 32 to 4096 instructions.
- **`guion run` compiles only what `main` can reach.** Embedders still get every function, because they may invoke others after `main`.

### Regression checks against the previous build

| Program | Previous | This build | This/previous |
| --- | ---: | ---: | ---: |
| Richards (15 trials) | 25.5 ms | 25.2 ms | 0.987 (0.982-1.007) |
| Mandelbrot (15) | 5.8 ms | 5.6 ms | 0.978 (0.972-0.989) |
| N-body (15) | 30.8 ms | 30.7 ms | 0.993 (0.981-1.003) |
| Parallel CPU 20k (41) | 2.03 ms | 2.03 ms | 1.002 (0.983-1.035) |
| **Parallel CPU 200k (41)** | **6.11 ms** | **6.71 ms** | **1.112 (1.070-1.140)** |
| `print("hello")` from source (25) | 2.56 ms | 2.56 ms | |
| `print("hello")` artifact (25) | 0.71 ms | 0.70 ms | |
| 200-line print artifact (25) | 0.78 ms | 0.80 ms | |

**Parallel CPU 200k is slower, and the cause is not established.** A separate 61-trial bisect put the builds between the previous one and this one at 1.02-1.07, starting with the first register build and with no steady trend. The workers run the same native code as before, so code or data layout is a possibility, not a finding. The program still takes 0.117 of Python serial.

Eager translation at load did slow running from source, 3.35 to 5.59 ms, because it covered the whole embedded compiler. Translating on first call removed that.

### Map probes

New for this pass, 7 trials. Python runs `sorted` where Guion's order is observed.

| Probe | Previous | This build | This/previous | Guion/Python |
| --- | ---: | ---: | ---: | ---: |
| Iterate an 8-key map 200k times | 221 ms | 140 ms | 0.634 | 0.910 |
| Look up one key 3M times | 294 ms | 94 ms | 0.322 | 0.493 |
| Build a 300k-key map, then iterate | 288 ms | 225 ms | 0.783 | **1.763** |
| Copy a 2000-key map 3000 times | 115 ms | 81 ms | 0.693 | **2.806** |

Python is still faster at building large maps and at copying them. Neither got worse, and neither is in the gate.

## Current position: remaining-gaps pass

Superseded by the section above; kept as the record of the previous build. Same container, CPython and profile-guided build recipe; every run's output matched. Interleaved 9 trials except where noted.

| Workload | Previous build | This build | 95% interval or upper bound | Result |
| --- | ---: | ---: | --- | --- |
| Integer loop | 1.792 | 0.125 | 0.123-0.128 | Guion faster; now compiled natively |
| Integer loop kept in the interpreter by a list print | 1285 ms | 1219 ms | | about 1.7x Python; interpreter cost is still higher |
| CSV ledger 200k | 2.502 | 1.790 | 1.782-1.810 | Python faster |
| JSONL 200k, as written, artifact, gate script | 1.040 | 1.019 | upper 1.043 | Python faster; gate not met |
| JSONL 200k, as written, source, gate script | 1.052 | 1.035 | upper 1.050 | Python faster; gate not met |
| JSONL 200k, as written, artifact, suite | 1.058 | 1.034 | 1.029-1.042 | Python faster |
| JSONL 200k, fused accessors, artifact, gate script | 0.965 | 0.930 | upper 0.946 | Guion faster; was not proven |
| JSONL 200k, fused accessors, source, gate script | 0.959 | 0.934 | upper 0.945 | Guion faster |
| JSONL 20k, artifact, gate script | 0.902 | 0.834 | upper 0.855 | Guion faster |
| JSONL 20k, source, gate script | 0.928 | 0.877 | upper 0.902 | Guion faster |
| JSONL 200k vs Python with SQLite, artifact, suite | 0.773 | 0.749 | 0.741-0.754 | Guion faster |

Suite ratios for compute moved slightly the wrong way (Richards 0.516 to 0.524, Mandelbrot 0.216 to 0.226, parallel CPU 200k 0.104 to 0.125), so those programs were run directly against the previous build, with the same program in both arms. This build over the previous one: Richards 0.990 (0.967-1.022), Mandelbrot 0.999 (0.981-1.031), N-body 0.999 (0.987-1.010), 15 trials; parallel CPU 20k 1.018 (0.984-1.070) and 200k 1.013 (0.959-1.069), 41 trials. Every interval includes 1.0, so no regression is shown. These programs run for only 2 to 31 ms. `print("hello")` and a 200-line print program start as fast as before (0.72 and 0.78 ms). The suite still reports `performance_status: failed` because of JSONL 200k; `correctness_status` is `passed`. 536 workspace tests pass.

Raw results: [gate script](results/jsonl-next-pass-paired-9.json), [interleaved](results/next-pass-vs-previous-interleaved-9.json), [suite](results/suite-format50-container-next-pass.json).

**Gate status.** Still open at 200k rows on the program as written, by 2-5% at the upper bound. The fused form clears it in both lanes. Rewriting the original form into the fused one automatically is not a safe peephole: the program branches on the intermediate `Result` values, so a rewrite could change which error is reported.

### What changed, plain build medians

| Change | CSV 200k | JSONL 200k | Integer loop |
| --- | ---: | ---: | ---: |
| Previous build | 355 ms | 838 ms | 1652 ms |
| Native lowering of `print` of `Text`, in functions with a loop | | | 94 ms |
| `csv_next_row` and `text_parse_integer` become direct host operations | 282 ms | no change | |
| Fused map and list reads without clones, fused calls of `Some`/`None` wrappers | 272 ms | about 1-2% | |
| Cancellation polled at frame entry, taken backward jumps and after host operations; one-branch fuel check | 255 ms | 750 ms | |
| Plain CSV record path, reusable line buffer, shared reader record names, quick integer parse | 247 ms | 737 ms | |
| One map lookup when updating an existing key | 238 ms | no change | |

The cancellation change is a behavior change. A cancelled program can now finish its current straight-line run of instructions before stopping. Host operations and loops still stop promptly, and the HTTP cancellation tests pass.

### Findings

- **The integer loop's gap came from one instruction.** A single `print` of `Text` after the loop kept the whole function out of the native tier. Python's bytecode count was never the issue there.
- **`std:csv` already exists.** The CSV gap is not a missing library. `csv_next` is a Rust host operation, as Python's `csv` is C. The cost was in how Guion reached it (a full frame and two value walks per call) and in the interpreter around it.
- **Option constructor wrappers.** Rewriting them in the artifact gained 4% on JSONL, but it removes the functions from native eligibility, because `MakeNone` takes its type from the enclosing function's result. The fused-call entries get part of that gain with native eligibility intact. They keep the value limit check, which probably accounts for the difference; that was not measured separately.
- **The native value bridge is still slower.** JSONL 1377 ms against 760 ms on the previous build, CSV 356 ms against 297 ms. It stays off by default.
- **A shipped bug, fixed.** A native function that printed and then failed lost the printed output; `print(7)` followed by a division by zero printed nothing. Native failures now commit pending output, as the VM does.

### Tried and rejected

| Experiment | Result | Decision |
| --- | ---: | --- |
| Check the `Load` combine path only where the table says it can apply | JSONL 748 to 765 ms, CSV 257 to 256 ms | reverted |
| Skip `value_matches` on `Return` and record field reads | about 1% | reverted: below the bar, removes a defensive check |

### Where the remaining cost is

Profile of the CSV ledger on this build: interpreter dispatch function 28%, `BTreeMap` lookups 12%, `Value` drop and clone accounting about 12%. CSV runs 102 instructions per row against Python's 66, so closing it needs fewer instructions as well as cheaper ones. Two approaches have published or measured support:

- Register-based bytecode. [Shi et al.](https://usenix.org/legacy/events/vee05/full_papers/p153-yunhe.pdf) removed about 47% of executed instructions for a JVM, and [the journal version](https://mural.maynoothuniversity.ie/10185/1/KC-Virtual-2008.pdf) reports a 1.48x average speedup with switch dispatch. Those are JVM results and do not transfer directly.
- Specialization and superinstructions, as in [PEP 659](https://peps.python.org/659), which CPython uses. The fused entries here are a small, static form of this.

`Map` is a `BTreeMap` with sorted iteration, against Python's hash table with cached string hashes. Changing it reaches across the runtime, and at 12% it cannot close CSV alone.

## Current position: profile-guided build `b7dd73e`

This section supersedes the gate decision and the "not closable" conclusions further down. Binary built by [`distribution/pgo/build.sh`](../../../distribution/pgo/build.sh), which trains only on its own separate programs and never on these benchmarks. CPython is the same PGO 3.14.7 build as before, so both sides are profile-guided. Same pinned Linux container, output checked against the reference on every workload (`correctness_status: passed`).

| Workload | Lane | `main` | This build | 95% interval or upper bound | Result |
| --- | --- | ---: | ---: | --- | --- |
| JSONL 20k | artifact, suite | 1.140 | 0.897 | 0.868-0.917 | Guion faster |
| JSONL 20k | uncached source, suite | | 0.903 | 0.887-0.909 | Guion faster |
| JSONL 20k | artifact, gate script, 31 trials | | 0.902 | upper 0.913 | Guion faster |
| JSONL 20k | source, gate script, 31 trials | | 0.928 | upper 0.935 | Guion faster |
| JSONL 200k | artifact, suite | 1.350 | 1.058 | 1.048-1.083 | Python faster |
| JSONL 200k | uncached source, suite | | 1.055 | 1.041-1.074 | Python faster |
| JSONL 200k | artifact, gate script, 9 trials | | 1.040 | upper 1.056 | Python faster |
| JSONL 200k | source, gate script, 9 trials | | 1.052 | upper 1.080 | Python faster |
| JSONL 200k, fused accessors and `format_utc_date` | artifact, gate script | 1.250 | 0.965 | upper 1.008 | median faster, not proven |
| JSONL 200k, fused accessors and `format_utc_date` | source, gate script | 1.325 | 0.959 | upper 0.981 | Guion faster |
| JSONL 20k vs Python with SQLite | artifact, suite | 0.911 | 0.729 | 0.721-0.735 | Guion faster |
| JSONL 200k vs Python with SQLite | artifact, suite | 1.007 | 0.773 | 0.761-0.802 | Guion faster |
| CSV ledger 200k | artifact, interleaved | 3.62 | 2.494 | 2.359-2.556 | Python faster |
| Integer loop, VM | artifact, interleaved | 4.52 | 1.818 | 1.794-1.835 | Python faster |

The `main` column for fused rows is the last recorded fused run, on an earlier build. The 9-trial gate script also reported a 20k source upper bound of 1.136, caused by one cold first sample (0.171 s against a median of 0.101 s); the raw samples are in the 9-trial file and the 31-trial rerun is in the table.

Raw results: [suite](results/suite-format50-container-final-pgo.json), [gate script, 9 trials](results/jsonl-final-pgo-paired-9.json), [gate script, 20k, 31 trials](results/jsonl-final-pgo-20k-paired-31.json), [interleaved against `main`](results/final-pgo-vs-main-interleaved-9.json). The interleaved runs measured JSONL 200k at 0.993 (bootstrap 95% 0.96-1.065); the suite and the gate script are the stricter and more repeatable readings, so they decide.

**Gate status.** Upper 95% below 1.0 is met at 20k rows in both lanes. It is not met at 200k rows on the frozen program, which is 4-8% short. The gate is open again, not closed.

### What changed, and what each part was worth

Plain build medians unless noted, JSONL 200k against Python.

| Change | JSONL 200k | CSV 200k | Integer loop |
| --- | ---: | ---: | ---: |
| `main` | 1000 ms | 441 ms | 3310 ms |
| `Value` from 88 to 24 bytes, frame vectors reused, inline integer and clone fast paths, shared record names | 877 ms | 368 ms | 1658 ms |
| Profile-guided build, same code | 749-784 ms | 290-299 ms | 1286-1318 ms |
| Python | 754-793 ms | 116-119 ms | 709-726 ms |

### Correction to earlier conclusions in this document

- "The remaining cost is the work itself" was wrong. The profile below already showed real data work (parse, read, lookup) at about 5-6% of the run. Most of the cost was value size and per-instruction overhead, and it moved once those were changed.
- The integer loop was 4.5x CPython, not "roughly twice". It is now 1.82x.
- The earlier "measured ceilings" were each taken one lever at a time with an 88-byte `Value` and without PGO. Together with a smaller `Value` they overlap and compound differently, so summing them did not bound the result.
- The conclusion that small VM changes could not close the gap was wrong for JSONL. No native tier was needed to go from 1.35 to about 1.05. It may still be right for CSV.

### What remains, with measurements

JSONL 200k and Python run the same number of bytecodes per row (about 265 and 270), and Guion's per-instruction cost is still higher (the integer loop's 1.82x). The near-tie therefore implies Guion spends less than Python outside the interpreter loop; that is an inference from these numbers, not a separate measurement. CSV runs 102 instructions per row against Python's 66. As an estimate only, 1.55x the instructions at the integer loop's 1.82x per-instruction cost predicts about 2.8x, near the measured 2.49x. Closing CSV needs fewer instructions per row as well as cheaper ones.

Tried in this round and rejected, all with identical output:

| Experiment | Result | Decision |
| --- | ---: | --- |
| Shared JSON object keys, a parser key cache, `Number::as_str`, reusing the `Arc` in `json_set` | 858 ms to 858 ms | reverted |
| Ceiling: skip re-walking `Map` and `List` contents in limit checks | 903 ms to 884 ms (2%), CSV 0% | not built: below the 5% bar and it weakens a safety check |
| Ceiling: also skip the JSON tree walk at frame boundaries | no further gain | not built |
| Allocator thread-local lookups | about 5% of samples, but already const-initialized, so this is free-list work | no change available |

The fused-accessor and `format_utc_date` form of the program is about 8% faster than the frozen form, which matches the distance left at 200k. It is available today by writing the program that way, and it is not applied automatically to programs written the frozen way.

## Current-branch refresh: the full performance suite (format 50, build `d1ad443`)

The repository's own suite, [`evaluation/completion/performance/runner.py`](../../completion/performance/runner.py), run unchanged against the current binary in the pinned Linux container (PGO CPython 3.14.7). Every workload's output matched its reference (`correctness_status: passed`). Ratios are Guion/Python, median with the bootstrap 95% interval; lower is better. Raw report: [`results/suite-format50-container-d1ad443.json`](results/suite-format50-container-d1ad443.json). These are Linux container numbers and are not comparable with the earlier macOS table above (for example Richards is 0.52 here and 0.375 there).

| Task class | Lane | Ratio (95% interval) | Result |
| --- | --- | ---: | --- |
| Richards scheduler | uncached source | 0.523 (0.512-0.540) | Guion faster |
| Richards scheduler | artifact | 0.527 (0.514-0.541) | Guion faster |
| N-body | uncached source | 0.171 (0.168-0.173) | Guion faster |
| N-body | artifact | 0.164 (0.163-0.165) | Guion faster |
| Mandelbrot | uncached source | 0.253 (0.247-0.268) | Guion faster |
| Mandelbrot | artifact | 0.245 (0.231-0.253) | Guion faster |
| Parallel CPU, 20k items | artifact, vs serial Python | 0.074 (0.067-0.076) | Guion faster |
| Parallel CPU, 200k items | artifact, vs serial Python | 0.159 (0.126-0.166) | Guion faster |
| Parallel CPU, 200k items | artifact, vs Python processes | 0.089 (0.071-0.093) | Guion faster |
| JSONL summary, 20k rows | artifact, vs Python | 1.140 (1.119-1.150) | Python faster |
| JSONL summary, 20k rows | artifact, vs Python with SQLite | 0.911 (0.903-0.918) | Guion faster |
| JSONL summary, 200k rows | artifact, vs Python | 1.350 (1.322-1.376) | Python faster |
| JSONL summary, 200k rows | artifact, vs Python with SQLite | 1.007 (0.992-1.021) | tie |
| CSV request ledger, 200k rows | source, earlier run below (not in this suite) | 3.62 (upper 95% 3.70) | Python faster |

The suite reports `performance_status: failed` only because its two JSONL data comparisons against the pure-Python reference are above 1.0. Every compute, parallel and SQLite-referenced comparison it requires passes.

### Decision on the JSONL speed gate

Superseded by [Current position](#current-position-profile-guided-build-b7dd73e). This build closed the gate as not met and said the remaining cost was the work itself. That reading was wrong, as the correction in that section records, and the gate is open again.

Measured at the time, one lever at a time: dispatch head 4.1%, allocation charging about 4%, all per-instruction bookkeeping together about 10% on JSONL (about 20% on compute loops), typed slots about 1%, moves by liveness 1-2%, native 47% slower than the VM.

## Final paired measurement (format 50 binary)

Nine randomized interleaved trials per program in the pinned container, run with the `--services` allow-list, with Guion output checked against the Python reference before timing. Ratios are Guion/Python; the second figure is the upper bound of the 95% interval. Script: [`benchmark_jsonl_paired_python.py`](benchmark_jsonl_paired_python.py); raw samples: [`results/jsonl-fused-vs-python-paired-9.json`](results/jsonl-fused-vs-python-paired-9.json).

| Rows | Program | Lane | Median | Ratio (median / upper 95%) |
| ---: | --- | --- | ---: | ---: |
| 20,000 | Python | | 0.110 s | |
| 20,000 | frozen | source | 0.148 s | 1.350 / 1.413 |
| 20,000 | frozen | artifact | 0.143 s | 1.294 / 1.356 |
| 20,000 | fused accessors and `format_utc_date` | source | 0.139 s | 1.235 / 1.273 |
| 20,000 | fused accessors and `format_utc_date` | artifact | 0.120 s | 1.096 / 1.145 |
| 200,000 | Python | | 0.761 s | |
| 200,000 | frozen | source | 1.150 s | 1.488 / 1.637 |
| 200,000 | frozen | artifact | 1.263 s | 1.658 / 1.714 |
| 200,000 | fused accessors and `format_utc_date` | source | 1.049 s | 1.363 / 1.404 |
| 200,000 | fused accessors and `format_utc_date` | artifact | 1.026 s | 1.358 / 1.383 |

The variant program is [`jsonl-summary-fused-accessors.gu`](../core_north_star_sprint/profile-inputs/jsonl-summary-fused-accessors.gu). Its output is identical to the frozen program's. The frozen program's artifact lane at 200k is slower than its source lane here; that is an unexplained run-to-run difference and is reported as measured.

The gate (upper 95% below 1.0 in both lanes at both sizes) is not met.

## Holdout and second workload (format 50 binary, same container and Python)

**Holdout: lockfile auditor** (`evaluation/completion/performance/benchmark_lockfile_diagnostics.py`, 15 fresh-process source runs per size, outputs checked against the expected report; raw: [`results/lockfile-diagnostics-format50-vs-python.json`](results/lockfile-diagnostics-format50-vs-python.json)). Guion/Python median ratio 0.325 (bootstrap 95% 0.314–0.343) at 512 packages and 0.412 (0.404–0.418) at 2,048. Guion leads. The `candidate_over_baseline` field in that file compares the current source against itself (`--baseline-commit HEAD`) and is a noise control (about 1.00), not a regression check.

**Second workload: CSV request ledger** ([`second_workload/`](second_workload/)): a per-service group-by over `service,status,latency_ms,bytes` rows with typed integer parsing, about 1% malformed rows, 40 services, and Python's C `csv` module as the reference. Stdout is identical to the reference on every run. Nine interleaved trials (raw: [`results/csv-ledger-vs-python-paired-9.json`](results/csv-ledger-vs-python-paired-9.json)):

| Rows | Python | Lane | Median | Ratio (median / upper 95%) |
| ---: | ---: | --- | ---: | ---: |
| 20,000 | 0.020 s | source | 0.057 s | 2.813 / 2.839 |
| 20,000 | | artifact | 0.054 s | 2.624 / 2.684 |
| 200,000 | 0.119 s | source | 0.434 s | 3.623 / 3.696 |
| 200,000 | | artifact | 0.436 s | 3.611 / 3.667 |

Cumulative cost at 200k rows (ms): read rows only 140; plus field access and two `text_parse_integer` calls 250; full ledger with four map updates 450. Reading alone already costs more than the whole Python run, and it is five VM operations per row around one host call (`csv_next_row`, `is_ok`, `unwrap`, `is_some`, `unwrap`), so the cost is again per-operation overhead, not the CSV state machine.

The calendar-conflicts scaling harness was also tried and rejected as a second workload: its Python reference needs the third-party `icalendar` package, which the pinned CPython does not have, so the Python side failed (`results/calendar-scaling-format50-vs-python.json` records that failed preflight).

Reading across classes: Guion leads on short-lived and compute-bound programs, and trails Python on programs whose time is spent in many small operations over a large stream of rows, where Python runs its loop body in C (`csv`, `json`) and Guion runs it in the interpreter. The CSV gap (3.6×) is larger than the JSONL gap (1.36×), so the per-operation cost is the common lever, and it is worth more than any JSONL-specific change.

## What was measured on the JSONL gate workload (200k rows, container)

Cumulative wall time for loop bodies of increasing depth:

| Stage | ms |
| --- | ---: |
| `fs_next_line` only | 80 |
| plus `json_parse` | 205 |
| `jsonl_next` as written in the stdlib | 318 |
| plus whitespace collapse | 370 |
| plus two `json_get_safe` and two `json_text_safe` | 450 |
| plus timestamp parse and both formatters | 515 |
| full program | about 1,090 |

About 575 ms is the grouping logic after the fields are decoded. The PC profile of the full program is flat: the interpreter loop is 31%, and nothing else exceeds 5%. Spread across it are limit checks (about 8%), `Value` clone and drop, allocation, and Arc refcount traffic.

The grouping logic was split further with a synthetic 200k-iteration loop (10 services, 60 dates, no file I/O), adding one block at a time:

| Block added | ms |
| --- | ---: |
| loop, index two lists | 74 |
| `var by_date = {}` and the `groups[service]` lookup | 10 |
| `map_has`, `by_date[date]` and branch | 70 |
| three `json_int_safe(unwrap(json_get_safe(...)))` reads | 120 |
| up to three `json_set` updates and compares | 125 |
| two `map_set` writes | 110 |

Every block is about six operations at roughly 100 ns each, so the cost is per-operation overhead (call frame, `Result` box, unwrap, accounting) spread evenly, not one slow operation.

Levers tried, all with byte-identical output:

| Lever | Gain | Decision |
| --- | ---: | --- |
| Pool allocator (committed) | about 15% | kept |
| Fat LTO, one codegen unit (committed) | about 4% | kept |
| Typed `record Group` with `Map[Text, Map[Text, Group]]` instead of `Json` group objects | none (1.18 s vs 1.15 s) | not adopted |
| `format_utc_date` instead of `text_chars` plus nine concatenations | about 2.5% | valid idiom; frozen program left unchanged |
| Hand-inlined stdlib wrappers | about 6.5% | not built: conflicts with the forwarder call-count and depth-limit guarantees |
| Fuel carry across frames | none | reverted |
| Cancellation polling every 64 instructions | n/a | broke 3 HTTP cancellation tests; reverted |
| Register-VM dispatch | about 11% ceiling | not built |

Correction to an earlier note: charge-on-load is already O(1). `cloned_value_allocation` charges 0 for lists, maps and JSON because their storage is shared, so there was no load-charge quota to change.

## Language finding

Bytecode `Some`, `None`, `Ok` and `Err` take their types from the enclosing function's return type. The verifier rejects them elsewhere ("Option constructor outside Option-returning function") with a function-level `R_ARTIFACT_INVALID` at line 0:0, although the compiler accepts the source. Stdlib code works around this with one-line shim functions; `jsonl_next` makes six shim calls per row. The measured wrapper cost of `jsonl_next` over a direct `fs_next_line` plus `json_parse` loop is about 113 ms (about 10%), so the ceiling for removing the shims is under 7% on this workload.

Typed constructor ops (`MakeNoneOf(T)` and so on) would remove the shims and the opaque error. They need an artifact-format bump and matching changes in the self-hosted compiler, so they are recorded here as a candidate and not built. A cheaper first step is for the compiler to report this case at the source line.

## Position

Incremental levers on this workload are each 10% or less. The best measured candidates are a fused field accessor (get, type-check and unwrap in one operation, about 10%) and removing the `jsonl_next` shims (about 7%); lazy JSON parsing is now estimated at about 5%, because parsing is only 11% of the run. Closing the remaining gap (ratio 1.45 to below 1.0 needs about 31% less wall time) needs a different value representation for dynamic data, such as lazy JSON field access and fewer allocations per row, not another VM tweak. That is a multi-session engine project whose payoff is unmeasured until prototyped.

The gate is not met. The honest claim today is the per-class table above: Guion leads on compute, parallel, short-source, and everyday-application classes, and trails on large-corpus dynamic-data shaping.

## Option/Result prototype: negative result and the per-instruction finding

The planned prototype (cheaper `Option`/`Result` handling) was checked against the profile before any layout change. Results, all in the pinned container with identical output:

| Experiment | Measurement | Decision |
| --- | --- | --- |
| Quote-free fast path in the CSV field parser | read-only loop 140 to about 138 ms; full ledger 439 to 440 ms | reverted: the parser is not the cost |
| Inlined scalar fast path ahead of the recursive `value_limit_error` | integer loop 2.77 to 2.69 s (3%), text loop 5.0 to 4.9 s (3%), CSV ledger unchanged | reverted: under the 5% bar and does nothing for the target |
| `Box` per `Ok`/`Some` | not built: about 3 small pool allocations in roughly 25 instructions and 625 ns per row, so at most 3% | closed |

What the measurements do show: the CSV read-only loop is 25 VM instructions per row (500,080 for 20,000 rows) and the full ledger is 102 per row (2,042,237 for 20,000 rows), which is about 20–25 ns per instruction. A pure integer loop in the VM costs about 10 ns per instruction (2.77 s for 20 million iterations of about 14 instructions), which the paired measurement later put at about 4.5x CPython's time for the same loop, not the roughly 2x first written here. The PC profile of that integer loop puts 62% in the VM dispatch function itself, with the limit check (11%), `Value` clone (8%) and drop (5%) and the allocation accounting (4%) behind it. Compute benchmarks beat Python because their hot loops run in the native tier; data programs do not, because `main` is rejected from native compilation at the first mixed `Value` operation and the mixed-value bridge made things slower (2.67 s against 1.46 s).

So the remaining distance on data-shaping programs is the interpreter tier's per-instruction cost, not any one operation or container. Another small VM change was not expected to close it; a smaller `Value` and PGO later moved JSONL from 1.35 to about 1.05 without a native tier. The two ways to change that are a native tier that owns `Text`/`List`/`Map`/`Json` values (so data programs run native like compute programs), or a much leaner interpreter core. Both are multi-session projects.

## Native-tier ceiling on the CSV ledger

Before building a native tier that owns `Text`/`List`/`Map` values, its payoff was bounded with the PC sampler on the 200k-row CSV ledger (three runs, 1,315 samples, format 50 release binary):

| Share of samples | Where |
| ---: | --- |
| 34% | VM dispatch loop (`execute_vm_frame_with_forwarders`) |
| about 9% | frame setup, `value_matches`, `host_op` dispatch |
| about 15% | `Value` clone and drop, limit checks, allocation accounting |
| about 8% | map lookup and replace (`BTreeMap::get`, `replace_map_value`) |
| about 7% | CSV read and integer parse |
| about 5.5% | atomic refcount and compare-and-swap traffic |
| about 4% | allocation |

A native tier removes dispatch and frame setup, at most about 43% of the run. Ownership, limits and allocation accounting must stay in native code, so the other shares remain. The ceiling is therefore about 450 ms to 260 ms, a Guion/Python ratio near 2.2 against the measured 3.6. It cannot reach 1.0 on this workload, so it was not built. The same shares on the JSONL gate program put its ceiling near 1.0 only if the native tier lost no accounting cost at all, which is not a result to plan on.

## Conclusion at this stage (superseded)

Current figures are in [Current position](#current-position-profile-guided-build-b7dd73e). At this stage, the gate (upper 95% below 1.0 in the source and artifact lanes at 20k and 200k rows) was not met: JSONL was 1.36 at 200k and the CSV ledger 3.6. Every lever the records listed had been measured. The remaining ones are each under 10%, and the native-tier ceiling above is about 40%. The claim this evidence supports is per task class: Guion is faster than CPython on short-lived programs, compute-bound programs, parallel CPU work and the lockfile-auditor holdout, and is slower on programs that run a long interpreter loop over a large stream of rows.

## Measured bound on the owned-value native tier (VM profile, JSONL 200k, line-table build)

PC-sampled VM run (2,174 in-binary samples). Real data work is small: serde JSON parse 0.9%, file line read 1.0%, text collapse 0.9%, BTreeMap get 1.3%, `json_try_get` 0.9% (about 5–6% together). `execute_vm_frame_with_forwarders` is 28.1% and includes inlined handlers, so pure dispatch is below that. Value plumbing is about 22%: `drop_glue<Value>` 4.1%, `value_limit_error_with_json` 3.7%, `cloned_value_allocation` 3.2%, alloc 2.9%, `Value::clone` 2.6%, atomics 3.4%, `charge_allocation` 1.2%, `shared_json_metrics` 1.1%.

Reading: an owned-pointer native tier removes dispatch (at most about 28%) but keeps clone, drop, limit and quota work unless values also get an unboxed representation. If native callbacks matched VM handler cost, JSONL would land near 0.72 x 1.15 s = 0.83 s against Python's about 0.85 s: at best break-even with the gate, not a clear win, and only after every op family is native. The fully native run today is 1.60 s against the VM's 1.15 s, so callbacks currently cost more than VM handlers. CSV stays above 2x on the same arithmetic. This bound is measured from the profile, not from the earlier 43% guess.

## Native callback-path steps (bridge build, JSONL 200k, output hash a1a9b95d unchanged)

| Step | Native median | VM |
| --- | ---: | ---: |
| Before | 1.60 s | 1.10 s |
| Native-to-native calls hand remaining fuel to the callee through the output buffer instead of reserve/release on the shared budget | 1.57 s | 1.10 s |
| Literal Text/Bytes constants charge a compile-time byte count instead of an arena lookup | 1.56 s | 1.10 s |

Both steps pass the native test suites with and without `native-value-bridge`; the IR-size guard thresholds in `execution.rs` were raised to cover the extra fuel load/store per function. Run-level counts for the same program: 3.57M native calls, 20.2M successful allocation charges (about 100 per row), 4.38M record charges, 3.48 GB gross charged bytes. The profile stays flat: the native gap is spread across per-operation callbacks, arena stores and quota charges, not one hotspot.

## Load-time forwarder inlining (relaxes the forwarder call-count and depth guarantees)

`Runtime::load` now rewrites each `Call` to a tiny forwarding function (`Load 0..n; <host operation>; Return`) into the host operation itself, for host operations only (`MakeSome`/`MakeNone` forwarders stay as calls because the native backend does not lower them inside functions returning other types). Instruction indices, spans and stack effect are unchanged; no format bump, no compiler or seed change, so the source and artifact lanes both get it. The result-limit check that the forwarder's return applied is kept after each such host operation in the VM. User-approved relaxations: wrapper calls no longer appear in `function_call_counts`, no longer consume call depth, and no longer add a trace frame.

JSONL 200k, output hash a1a9b95d unchanged:

| Build | VM median | Native (bridge) median |
| --- | ---: | ---: |
| Before (fuel handoff only) | 1.10–1.15 s | 1.56 s |
| Host-op forwarders inlined | 1.00 s | 1.47 s |

A first attempt that also inlined `MakeSome`/`MakeNone` appeared to bring native to 1.08 s, but the profile showed `main` and `jsonl_next` had stopped compiling natively (6 native calls), so that figure was a VM run and is not used.

## Paired result after forwarder inlining and the JSON key pre-check (container, PGO CPython 3.14.7, 9 interleaved output-checked trials)

Raw output: [`results/jsonl-forwarder-inlined-paired-9.txt`](results/jsonl-forwarder-inlined-paired-9.txt) (binary before the JSON key pre-check). Guion/Python median / upper 95%:

| Rows | Program | Lane | Ratio |
| ---: | --- | --- | ---: |
| 20k | frozen | source | 1.238 / 1.312 |
| 20k | frozen | artifact | 1.082 / 1.149 |
| 20k | fused + `format_utc_date` | source | 1.144 / 1.238 |
| 20k | fused + `format_utc_date` | artifact | **0.995 / 1.047** |
| 200k | frozen | source | 1.385 / 1.427 |
| 200k | frozen | artifact | 1.354 / 1.392 |
| 200k | fused + `format_utc_date` | source | 1.325 / 1.355 |
| 200k | fused + `format_utc_date` | artifact | 1.250 / 1.286 |

The gate is still not met. Compared with the previous paired run, the frozen artifact lane at 200k moved from 1.658 to 1.354 and the fused artifact lane at 20k from 1.096 to 0.995. Per-row cost is about 4.5 us for Guion against about 3.5 us for Python at 200k rows. Executed instructions are about 265 per row (Load 72, Store 30, Const 23, JumpIfFalse 21, Unwrap 18, Not 13, Index 12); the remaining levers measured so far are each 1–5%: lazy JSON, typed constructors, superinstruction fusion of the top opcode pairs (Store-Load, Load-Const, Load-Unwrap, Not-JumpIfFalse), folding JSON metrics into the parse.

## Instruction-pair fusion (negative result)

Fused `Not`->`JumpIfFalse`, plain `Store`->`Load` and `Load`->`Const` inside the VM loop with exact fuel and quota accounting, plus a scalar fast path for `Const` limit checks (the existing `Load` fast paths already fuse `Load` with `IsOk`/`IsSome`/`Unwrap`/`GetField`/list `Index`, so those pairs in the transition profile were already single dispatches). Output hashes unchanged. JSONL 200k: 1.018 s to 0.998 s; second benchmark pair 0.913 s to 0.933 s; CSV 434 ms to 419 ms; integer loop 2467 ms to 2489 ms. All within run-to-run noise; reverted.

## Upper bound on the quota and limit plumbing (throwaway build, not committed)

To bound what a value-representation redesign could recover, a probe build turned `charge_allocation`, `Budget::allocate` and `value_limit_error` into no-ops (all allocation accounting and value-limit checks removed; output hash a1a9b95d unchanged). JSONL 200k: 1.02 s to about 0.92 s (median of the three runs after the first), roughly 10%. That is the ceiling for removing every quota charge and limit check, which a real design must keep in some form (compile-time-known sizes, batched charges), so the realistic recovery is well under 10%. The gap to Python at 200k is about 22%; quota and limit plumbing cannot close it, and the rest of the value cost (clone and drop with atomic refcounts, allocation, dispatch) is spread across the same flat profile.
