# Veritas

Implementation scope and acceptance criteria: [complete product plan](docs/superpowers/plans/2026-10-24-veritas-complete-product.md). The final builder assessment and release handoff are recorded in [docs/completion-report.md](docs/completion-report.md) once closeout is complete.

Veritas is an experimental general-purpose language and toolchain. The default CLI embeds a Veritas self-hosted compiler and Rust bytecode runtime; the core build does not require Python. Its design target is a project lifecycle that an assistant can recommend over Python for suitable work.

## Try the current build

From this directory:

```sh
cargo run -p veritas-cli -- run language/examples/sum.vr
cargo run -p veritas-cli -- check language/examples/sum.vr
cargo run -p veritas-cli -- test language/examples/sum.vr
cargo run -p veritas-cli -- doc --json language/examples/records.vr
cargo run -p veritas-cli -- build --json language/examples/sum.vr
cargo run -p veritas-cli -- package --json evaluation/development/task_ledger --output /tmp/task-ledger-json
cargo run -p veritas-cli -- verify-package --json /tmp/task-ledger-json
cargo run -p veritas-cli -- build language/examples/sum.vr
cargo run -p veritas-cli -- run language/examples/sum.vrb
cargo run -p veritas-cli -- package evaluation/development/task_ledger --output /tmp/task-ledger-app
/tmp/task-ledger-app/run add "First packaged task"
```

The example prints `15`; `cargo run -p veritas-cli -- run language/examples/matching.vr` exercises exhaustive `Option`/`Result` matching; `cargo run -p veritas-cli -- run language/examples/records.vr` exercises records; `cargo run -p veritas-cli -- run language/examples/modules_main.vr` compiles an imported module graph. Veritas needs Rust/Cargo only to build the tool itself; a compiled `.vrb` file runs in the Veritas runtime without the source file.

`language/examples/binary_copy.vr` copies an `input.bin` file to `copy.bin` while preserving arbitrary bytes. Run it from the example directory with file access enabled; `len(Bytes)` reports bytes, not Unicode characters.

`language/examples/python_keywords.vr` demonstrates the optional compatibility bridge. It requires a build with `--features python-interop`, declared dependencies, and explicit `veritas sync`; the default core build does not discover or launch Python or uv.

Applications that need the current local date can import `std:time` and call `today_local()`. Clock access is denied by default; grant it with `[clock] enabled = true` in `veritas.toml` or `veritas run --allow-clock`. The date follows the host process time zone. See [`language/host-api.md`](language/host-api.md) for the capability and deterministic embedding API.

`language/examples/http_get.vr` demonstrates a bounded HTTP text GET. Network access defaults to denied; grant exact hostnames in `[network]` in the project manifest before running it. `http_request` supports bounded JSON API calls with methods, headers, text bodies, response status, and same-host redirects. `http_request_with_timeout` supports a per-request deadline capped by the manifest's network timeout. `eprint(value)` writes captured program diagnostics to stderr separately from `print(value)` output.

`veritas package PROJECT_DIRECTORY --output DIRECTORY` creates a runnable current-host bundle with the compiled artifact, matching Veritas runtime, project policy, launcher, and declared resources. Core packages never contain Python or uv. Python-assisted projects require the opt-in `python-interop` build and retain their pinned-runtime packaging behavior. `NATIVE-DEPENDENCIES.json` and `linux-dependencies.json` record target-specific runtime dependencies. `veritas verify-package DIRECTORY` checks package hashes; hashes detect accidental corruption but do not authenticate the publisher.

`evaluation/development/feed_digest` is a larger known workload that combines concurrent HTTP GET, a typed Python call into the managed `feedparser` package, deterministic link deduplication and file output. Its checked-in manifest grants only `127.0.0.1` for the loopback acceptance suite. Copy the project before adapting it to public feeds, then list the exact feed hostnames in `[network].hosts` and run `veritas sync` in that project.

`evaluation/development/inventory_reconcile`, `api_sync`, and `package_analysis` cover data validation/state updates, restartable paginated APIs, and compiled-wheel analysis at 100,000-row volume. Each has an independent Python reference and parity/relocation acceptance. `api_sync` reads its Bearer token through a manifest-granted `env("VERITAS_API_TOKEN")` capability, not from its command line.

`evaluation/development/task_ledger` is a pure-Veritas multi-module CLI app that creates `tasks.json` on first use. It demonstrates JSON construction, atomic replacement, subcommand argument slicing, persistence across runs, and process exit statuses. Atomic replacement prevents partial files; simultaneous writers are not coordinated. Artifact format 39 adds explicitly granted clock reads for applications that need wall-clock time or the current local date.

## Current status

For a machine-readable environment check, run `veritas doctor --json`. It reports whether native Veritas execution and optional Python calls are available separately.

The default build is the Python-free core. Its workflow includes `new`, `check`, `fmt`, `lint`, `test`, `run`, `build`, `package`, `verify-package`, `add`, `sync --locked --offline`, `update`, `inspect`, `doc`, `doctor`, and `install`. Core commands do not discover or launch Python or uv. The Python bridge is an opt-in compatibility feature built with `--features python-interop`.

`veritas fmt FILE.vr` tokenizes code outside comments and strings, normalizes spacing, and preserves comment and string contents. `veritas lint FILE.vr` reports unused imports and locals and unreachable statements with stable warning codes. `veritas doc --json FILE.vr` includes source parameter names, checked types, effects, standard-library availability, native package identities, and optional profile requirements. `veritas install PACKAGE --prefix DIRECTORY` verifies a current-host package, keeps its versioned contents under `share/veritas/apps`, and creates its app command under `bin`.

Implemented: arbitrary-precision integers, finite `Float64`, functions and modules, records and typed collections, `Option`/`Result`, bounded parallel execution, streaming data/file APIs, transactional SQLite/TOML/calendar services, Unicode and HMAC support, bounded cancellable HTTP, explicitly granted clock access, a self-hosted source-to-artifact compiler, a Python-free core profile, exact native package locks, and an installed builder lifecycle with formatting, lint, docs, packaging, and app installation. The Python bridge remains an opt-in compatibility feature.

Final status, supported targets, verification commands, remaining limits, and installation/removal guidance are in [docs/completion-report.md](docs/completion-report.md).

See [docs/veritas_checkpoint_v1_34_structured_sync_reports.md](docs/veritas_checkpoint_v1_34_structured_sync_reports.md), [docs/veritas_checkpoint_v1_33_structured_package_reports.md](docs/veritas_checkpoint_v1_33_structured_package_reports.md), [docs/veritas_checkpoint_v1_32_structured_build_reports.md](docs/veritas_checkpoint_v1_32_structured_build_reports.md), [docs/veritas_checkpoint_v1_31_relocatable_evaluation_kit.md](docs/veritas_checkpoint_v1_31_relocatable_evaluation_kit.md), [docs/veritas_checkpoint_v1_30_shipped_docs_repair.md](docs/veritas_checkpoint_v1_30_shipped_docs_repair.md), [docs/veritas_checkpoint_v1_29_structured_api_docs.md](docs/veritas_checkpoint_v1_29_structured_api_docs.md), [docs/veritas_checkpoint_v1_28_offline_cache_integrity.md](docs/veritas_checkpoint_v1_28_offline_cache_integrity.md), [docs/veritas_checkpoint_v1_27_json_test_reports.md](docs/veritas_checkpoint_v1_27_json_test_reports.md), [docs/veritas_checkpoint_v1_26_local_installer.md](docs/veritas_checkpoint_v1_26_local_installer.md), [docs/veritas_checkpoint_v1_25_native_python_owners.md](docs/veritas_checkpoint_v1_25_native_python_owners.md), [docs/veritas_checkpoint_v1_24_standalone_notices.md](docs/veritas_checkpoint_v1_24_standalone_notices.md), [language/spec.md](language/spec.md), [language/python-abi.md](language/python-abi.md), [docs/reference.md](docs/reference.md), [docs/architecture.md](docs/architecture.md), [docs/veritas_checkpoint_v1_23_native_provenance.md](docs/veritas_checkpoint_v1_23_native_provenance.md), [docs/veritas_checkpoint_v1_22_dependency_provenance.md](docs/veritas_checkpoint_v1_22_dependency_provenance.md), [docs/veritas_checkpoint_v1_21_package_integrity.md](docs/veritas_checkpoint_v1_21_package_integrity.md), [docs/veritas_checkpoint_v1_16_python_json_abi.md](docs/veritas_checkpoint_v1_16_python_json_abi.md), [docs/veritas_checkpoint_v1_15_atomic_state.md](docs/veritas_checkpoint_v1_15_atomic_state.md), [docs/veritas_checkpoint_v1_14_json_state.md](docs/veritas_checkpoint_v1_14_json_state.md), [docs/veritas_checkpoint_v1_13_feed_digest.md](docs/veritas_checkpoint_v1_13_feed_digest.md), [docs/veritas_checkpoint_v1_11_python_keyword_calls.md](docs/veritas_checkpoint_v1_11_python_keyword_calls.md), [docs/veritas_checkpoint_v1_7_resource_accounting.md](docs/veritas_checkpoint_v1_7_resource_accounting.md), [docs/reuse-inventory.md](docs/reuse-inventory.md), and [docs/prior-language-build-lessons.md](docs/prior-language-build-lessons.md).

## Build a current-host bundle

On macOS ARM64, provide the archive named by `distribution/toolchains/manifest.toml` as `VERITAS_UV_ARCHIVE` or a standalone uv executable matching its binary checksum as `VERITAS_UV_BINARY`. On Debian-compatible GNU/Linux ARM64, use `distribution/toolchains/manifest-linux-aarch64.toml` and its matching pinned binary. Then run `sh distribution/package-current-host.sh /path/to/output` on the target host. The script verifies uv before packaging. After extraction, run `sh install.sh` to install under `~/.local`, or provide a prefix. The installer keeps release files together, creates a `bin/veritas` command, preserves previous versions, and does not edit shell startup files. Archives are unsigned and require their named OS, architecture and runtime ABI. Managed CPython and wheel dependencies are acquired by `veritas sync` on the target host. See `PACKAGE.txt` and `doctor.txt` inside the archive for included capabilities and limits.

For a Python-assisted application, run `veritas sync` during development, then `veritas package` to create an app bundle containing managed CPython, the locked bridge environment, and offline cache for the current target. Same-target relocation and offline launches are verified on macOS ARM64 and Debian 12-compatible Linux ARM64 for selected wheels; arbitrary native wheels and other targets remain unverified.

