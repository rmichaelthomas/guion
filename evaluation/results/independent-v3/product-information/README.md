# Veritas

Veritas is an experimental general-purpose language and toolchain. The current build is a real Rust-hosted compiler and bytecode runtime, not a Python wrapper. Its design target is a project lifecycle that may fit some tasks well; the current build is experimental, and its capabilities and limits are described below.

## Try the current build

From this directory:

```sh
cargo run -p veritas-cli -- run language/examples/sum.vr
cargo run -p veritas-cli -- check language/examples/sum.vr
cargo run -p veritas-cli -- test language/examples/sum.vr
cargo run -p veritas-cli -- doc --json language/examples/records.vr
cargo run -p veritas-cli -- build --json language/examples/sum.vr
cargo run -p veritas-cli -- build language/examples/sum.vr
cargo run -p veritas-cli -- run language/examples/sum.vrb
```

The example prints `15`; `cargo run -p veritas-cli -- run language/examples/matching.vr` exercises exhaustive `Option`/`Result` matching; `cargo run -p veritas-cli -- run language/examples/records.vr` exercises records; `cargo run -p veritas-cli -- run language/examples/modules_main.vr` compiles an imported module graph. Veritas needs Rust/Cargo only to build the tool itself; a compiled `.vrb` file runs in the Veritas runtime without the source file.

`language/examples/binary_copy.vr` copies an `input.bin` file to `copy.bin` while preserving arbitrary bytes. Run it from the example directory with file access enabled; `len(Bytes)` reports bytes, not Unicode characters.

`language/examples/python_keywords.vr` demonstrates a declared Python call with a named argument and a Python default. The usual path is to declare the exact Python version and dependencies in `veritas.toml`, then run `veritas sync PROJECT`; Veritas starts that managed interpreter. If configuring an external interpreter explicitly, use its actual launcher name, commonly `python3`, and do not assume a `python` command exists.

`language/examples/http_get.vr` demonstrates a bounded HTTP text GET. Network access defaults to denied; grant exact hostnames in `[network]` in the project manifest before running it. `http_request` supports bounded JSON API calls with methods, headers, text bodies, response status, and same-host redirects. `http_request_with_timeout` supports a per-request deadline capped by the manifest's network timeout. `eprint(value)` writes captured program diagnostics to stderr separately from `print(value)` output.

`veritas package PROJECT_DIRECTORY --output DIRECTORY` creates a runnable, current-host bundle. List required data files and Python modules under `[package].resources`; the bundle contains only those project files. For Python-assisted projects, it also includes pinned managed CPython, pinned `uv`, locked dependencies and an offline cache. `PYTHON-DEPENDENCIES.json` inventories managed Python distributions and bundled interpreter/tool licenses. `NATIVE-DEPENDENCIES.json` records macOS Mach-O dependencies; `linux-dependencies.json` records Linux ELF dependencies, runtime resolutions and required glibc symbol versions. Both target audits record hashes and architectures, attribute installed native Python files through wheel `RECORD` entries, and fail on unresolved dependencies or unattributed extensions. Prepared Veritas distributions add a target-specific Rust dependency notice inventory and license tree. The launcher verifies recorded package hashes before each run. Package schema 2 hashes immutable Python offline-cache payloads while excluding uv's mutable bookkeeping files; schema 1 packages remain verifiable. `PACKAGE.json` records runtime, artifact, resource, Python lock, offline-cache, native-report and available Rust-notice identities. Run the program with `./run`; check a package directly with `veritas verify-package DIRECTORY`. Hashes detect accidental corruption but do not authenticate the publisher. Static load audits cannot see libraries opened only by dynamic runtime calls. License metadata is preserved as supplied; Veritas does not interpret license compatibility or make a legal compliance determination.

Veritas supports project packaging, stateful multi-module applications, bounded HTTP, manifest-granted environment access, and managed Python interop. The current package flow and its documented limits are described in the included [architecture](architecture.md) and [setup guide](setup-guide.md).

## Current status

For a machine-readable environment check, run `veritas doctor --json`. It reports whether native Veritas execution and optional Python calls are available separately.

Implemented: arbitrary-precision integers with explicit division rules; finite `Float64`; functions, modules, records, typed empty lists/maps, immutable list updates, Float64 square root, JSON values and typed JSON column extraction; `json_project_columns` streams through JSON while retaining only requested fields and still checks ignored data for duplicate keys and resource limits; stable first-seen text categorization for projected columns; `Option`/`Result`; bounded `parallel_map`; text/byte file operations with atomic replacement; default-deny bounded HTTP GET and general text requests with exact-host grants and request-specific deadlines bounded by host policy; exact-grant environment reads; typed Python calls through a supervised sidecar; managed CPython and locked dependencies via pinned uv; separate stdout/stderr program output with structured effects; source-mapped diagnostics, comment-preserving formatting, inspection and structured CLI reports; current bytecode format 33, including Unicode-scalar `text_chars` and copy-on-write local list and JSON-array append; local package creation/verification and a current-host installer. Cranelift JIT supports selected numeric, list, record-call, and stdout-print paths; checked integer overflow and unsupported operations fall back to the VM. The format-33 campaign candidate is packaged for Debian 12-compatible GNU/Linux ARM64. A macOS ARM64 0.1.0 package exists at the earlier format-30 level and must be refreshed before claiming format-33 packaging parity across both targets.

An experimental self-hosted compiler frontend is included for a supported subset: a Rust-hosted compiler builds the Veritas compiler, then the Veritas-built compiler recompiles its own source to matching compiler IR and compiles selected real programs. The bytecode assembler and VM remain Rust components; this is a bootstrap proof, not a claim that the entire compiler/runtime is written in Veritas.

Remaining gaps: maps and other broader collection shapes in native code, streaming/binary HTTP, cancellation during a blocking network call, syntax-aware formatting, zero-copy Python buffers, additional targets, broader ecosystem coverage, wider VM/native allocation parity, and an exact RSS cap. Selected compiled wheels support same-target relocation and offline launch on macOS ARM64 and Debian 12-compatible Linux ARM64; arbitrary wheels and hosts remain outside the documented support boundary. Network host grants do not prevent DNS rebinding or provide operating-system egress confinement. This bundle describes product capabilities and limits; it contains no candidate or comparative outcome data.

See [architecture.md](architecture.md), [compatibility-and-migrations.md](compatibility-and-migrations.md), [host-api.md](host-api.md), [language-spec.md](language-spec.md), [python-abi.md](python-abi.md), [reference.md](reference.md), and [setup-guide.md](setup-guide.md) in this bundle.

## Build a current-host bundle

On macOS ARM64, provide the archive named by `distribution/toolchains/manifest.toml` as `VERITAS_UV_ARCHIVE` or a standalone uv executable matching its binary checksum as `VERITAS_UV_BINARY`. On Debian-compatible GNU/Linux ARM64, use `distribution/toolchains/manifest-linux-aarch64.toml` and its matching pinned binary. Then run `sh distribution/package-current-host.sh /path/to/output` on the target host. The script verifies uv before packaging. After extraction, run `sh install.sh` to install under `~/.local`, or provide a prefix. The installer keeps release files together, creates a `bin/veritas` command, preserves previous versions, and does not edit shell startup files. Archives are unsigned and require their named OS, architecture and runtime ABI. Managed CPython and wheel dependencies are acquired by `veritas sync` on the target host. See `PACKAGE.txt` and `doctor.txt` inside the archive for included capabilities and limits.

For a Python-assisted application, run `veritas sync` during development, then `veritas package` to create an app bundle containing managed CPython, the locked bridge environment, and offline cache for the current target. Same-target relocation and offline launch are supported for selected wheels on macOS ARM64 and Debian 12-compatible Linux ARM64; arbitrary native wheels and other targets remain outside the documented support boundary.

