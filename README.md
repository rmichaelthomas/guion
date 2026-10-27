# Guion

Guion is a general-purpose programming language and toolchain built to make useful software easier to write, understand, and run. It has clear, typed syntax, a compiler written in Guion, and a Rust runtime. Normal source runs use the in-process Rust compiler. The default build works without Python.

The long-term ambition is to make Guion a practical choice over Python for more everyday programming, with a quick response and a broad range of useful work. Its programs use structured syntax; “plain English” describes the learning and tooling goal, not free-form natural-language code. That is a goal, not a result we claim today. Guion is an early, private pre-release project; there is no public release download yet.

## Try it from this checkout

You need Rust and Cargo to build Guion from source. From the repository root:

```sh
cargo run --locked -p guion-cli -- new /tmp/hello-guion
cargo run --locked -p guion-cli -- check /tmp/hello-guion/src/main.gu
cargo run --locked -p guion-cli -- test /tmp/hello-guion/src/main.gu
cargo run --locked -p guion-cli -- run /tmp/hello-guion
```

The starter program prints `Hello, Guion!`. The same CLI can format and lint source, build portable `.gub` artifacts, package and verify applications, inspect effects, and generate API documentation.

## What works today

- A self-hosted compiler checks and builds `.gu` programs into versioned `.gub` artifacts.
- The Rust runtime supports typed functions and records, collections, JSON, files, SQLite, TOML, HTTP, bounded concurrency, and explicit host permissions.
- The portable shared VM also exposes a versioned WebAssembly ABI for typed browser calls. ABI 1.0 preserves the legacy exports, grants no host capabilities, and bounds requests, values, output, and execution; see the [browser ABI guide](docs/browser-wasm-abi/README.md).
- The core CLI can create, check, format, lint, test, run, build, package, install, and inspect programs.
- Optional Python interoperability exists for projects that need it. It is off in the default core build.

See the [getting-started guide](docs/getting-started.md), [language and command reference](docs/reference.md), and [language contract](language/spec.md).

## Status and comparison with Python

The Guion validation matrix previously passed on macOS ARM64 and Debian 12 GNU/Linux on ARM64 and x86_64, all with glibc. In the latest randomized, output-checked 30-trial Richards run on macOS ARM64, fresh Guion source took **28.55 ms**, down from 30.04 ms; CPython 3.14.7 took 73.62 ms. Guion's source cache was bypassed, and the OS file cache was uncontrolled. The native compiler now lowers literal division and remainder by two to a signed shift and a bit mask, preserving floor behavior for negative integers. Candidate/prior median ratio was 0.940 (paired 95% interval 0.930–0.967); candidate/Python was 0.389 (0.382–0.395). In a separate randomized 25-execution lane, Guion improved from 7.164 to 6.061 ms per execution (ratio 0.847, interval 0.843–0.854). Generated native code fell from 409 to 398 IR blocks and from 4,234 to 4,086 instructions. These Richards-specific results do not establish a general Python performance claim or a cold-disk result. See the [plain-English Richards follow-up](evaluation/completion/results/runtime-efficiency/2026-10-27-richards-power-of-two.md), [fresh-source comparison](evaluation/completion/results/runtime-efficiency/2026-10-27-richards-power-of-two-fresh-source-30-trial-comparison.json), and [25-execution comparison](evaluation/completion/results/runtime-efficiency/2026-10-27-richards-power-of-two-25-run-30-trial-comparison.json).

A separate synthetic workload tested 25,000 iterations of integer floor division and remainder by 1, 4, 8, and 16. Fresh-source Guion measured 8.980 ms before and 8.206 ms after; the 25-execution lane improved from 1.102 to 0.515 ms per execution. CPython 3.14.7 measured 32.943 ms fresh and 5.937 ms per repeated execution. These results are specific to this arithmetic loop, with the same cache limits described above. See the [arithmetic report](evaluation/completion/results/runtime-efficiency/2026-10-27-power-of-two-arithmetic.md) and [runnable benchmark fixtures](evaluation/completion/performance/fixtures/power-of-two-arithmetic/).

A follow-up loop targeted truncating integer division (`/`) by 1, 2, 4, 8, and 16. Fresh-source Guion measured 8.507 ms before and 7.932 ms after; the 25-execution lane improved from 0.679 to 0.482 ms per execution. Paired candidate/previous ratios were 0.921 (95% interval 0.908–0.958) fresh and 0.713 (0.695–0.727) repeated. CPython 3.14.7 measured 36.835 ms fresh and 9.746 ms per repeated execution. This is another focused arithmetic workload, not a general performance claim. See the [truncating-division report](evaluation/completion/results/runtime-efficiency/2026-10-27-power-of-two-integer-division.md) and [benchmark fixtures](evaluation/completion/performance/fixtures/power-of-two-integer-division/).

The next arithmetic pass added literal negative power-of-two divisors for floor division, truncating division, and remainder while preserving the `-1` overflow fallback. On a 25,000-value mixed-sign loop, fresh-source Guion improved from 9.204 to 8.383 ms; the 25-execution lane improved from 1.183 to 0.635 ms per calculation. Candidate/prior ratios were 0.904 (95% interval 0.856–0.934) fresh and 0.534 (0.527–0.543) repeated. CPython 3.14.7 measured 38.797 ms fresh and 11.854 ms per repeated calculation. These are focused workload results. See the [negative-divisor report](evaluation/completion/results/runtime-efficiency/2026-10-27-negative-power-of-two-division.md) and [benchmark fixtures](evaluation/completion/performance/fixtures/power-of-two-negative-division/).

A follow-up specialized native list-update calls by their known element type. In the canonical Richards workload, the 25-execution lane improved from 6.045 to 5.830 ms per execution (candidate/prior 0.966, 95% interval 0.961–0.969). The fresh-source result was inconclusive at 28.784 versus 28.134 ms (ratio 0.990, interval 0.962–1.010). This retains a sustained-runtime win without claiming a first-run improvement. See the [typed-helper report](evaluation/completion/results/runtime-efficiency/2026-10-27-richards-typed-list-set.md).

The next Richards pass specialized list updates by both element type and copy-versus-local mode, removing the mode flag from native calls. Fresh-source Guion improved from 28.004 to 27.617 ms (candidate/prior 0.988, paired 95% interval 0.976–0.997). The 25-execution lane was effectively unchanged at 5.991 versus 5.990 ms per execution (ratio 0.997, interval 0.984–1.013). Generated native code fell by 38 instructions. This is a small fresh-run gain with no demonstrated repeated-runtime gain. See the [copy-mode report](evaluation/completion/results/runtime-efficiency/2026-10-27-richards-list-set-copy-mode.md) and [raw comparison](evaluation/completion/results/runtime-efficiency/2026-10-27-richards-list-set-copy-mode-comparison.json).

Immutable, reference-counted text storage targets everyday text-heavy programs. In randomized output-checked AllHereHTML source runs, the candidate was 5.9% faster at the original 42 KB template, 2.7% faster at 2× input, and 2.8% faster at 4× input; every generated page matched its expected hash. The Richards control was statistically unchanged: candidate/prior was 1.008 (95% interval 0.999–1.013) fresh and 1.004 (0.996–1.006) over 25 executions per process. This is a measured gain for AllHereHTML, not a general performance claim. Source caching was bypassed and OS file caching was uncontrolled. See the [shared-text report](evaluation/completion/results/runtime-efficiency/2026-10-27-shared-text-storage.md) and its [paired AllHereHTML measurements](evaluation/completion/results/runtime-efficiency/2026-10-27-shared-text-allherehtml-comparison.json).

A separate post-commit AllHereHTML run reproduced wins at all three sizes: paired candidate/baseline ratios were 0.953, 0.947, and 0.966, with 95% intervals below 1. The follow-up agent-choice study stayed at Guion 3/3, unchanged from the previous study; all three applications passed their acceptance runs. That study reused the earlier frozen task timings, so it confirms the preference result but does not show that this text-storage change caused a preference increase. See the [post-commit comparison](evaluation/completion/results/runtime-efficiency/2026-10-27-shared-text-allherehtml-postcommit-comparison.json) and the [choice-study summary](evaluation/completion/results/real-task-sprint/agent-choice/summary.json).

The five short-source comparisons also passed. A host profile captured generated-code execution and memory-management activity; it did not isolate every cost inside generated code.

Guion and a Python reference each passed 127 frozen cases across nine applications. Guion took less time on the selected measured case for each application; the shared Python reference uses broad imports, so the numbers describe those particular implementations. The Guion AllHereHTML builder is also retained as a regression, and its text operations now handle larger inputs within the default allocation budget. See the [real-task and runtime report](evaluation/completion/results/real-task-sprint/summary.md) for timings, memory, method, and limits. These measurements do not establish that Guion generally outperforms or replaces Python.

Guion remains a private pre-release project; no public release download has been published. See the [pre-release readiness report](docs/pre-release-readiness.md) for the validated scope and remaining work.

## Project guide

- [Getting started](docs/getting-started.md)
- [Agent programming path](docs/agents/start-here.md) and [runnable recipes](docs/agents/recipes.md)
- [Architecture](docs/architecture.md)
- [Browser WebAssembly ABI](docs/browser-wasm-abi/README.md)
- [Language and command reference](docs/reference.md)
- [Compatibility and migration](docs/compatibility-and-migrations.md)
- [Current status](docs/status.md)
- [Historical record index](docs/history/README.md)
- [Contributing](CONTRIBUTING.md)
- [Security](SECURITY.md)
- [License](LICENSE) and [third-party notices](THIRD-PARTY-NOTICES.md)
