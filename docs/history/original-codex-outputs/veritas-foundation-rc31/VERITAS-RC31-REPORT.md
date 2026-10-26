# Veritas RC31 build report

**Result:** the Veritas foundation is feasible as a serious language project, and this build now connects several pieces into a usable vertical application. The assistant-recommendation goal remains unproven. The evidence supports continued building, not recommending Veritas over Python generally.

## What the build demonstrates

The feed-digest app accepts URLs at runtime, fetches feeds concurrently under an explicit host policy, parses them with the locked Python `feedparser` package through a typed adapter, resolves duplicate links deterministically, preserves Unicode, reports an individual feed failure, and writes a Markdown file. Python supplies a real library ecosystem while Veritas owns the CLI, concurrency, network boundary and user-visible flow.

The released command path is `veritas sync PROJECT` followed by `veritas run PROJECT URL...`; users do not need to remember whether their machine calls the interpreter `python` or `python3` for this project. A target host still acquires the managed Python runtime and dependency wheels during sync.

The macOS ARM64 archive was extracted to a new temporary directory, passed `veritas doctor`, synchronized its pinned Python environment offline from the already populated cache, and passed all six acceptance cases using the packaged executable. This is extracted-path evidence on one host with a warm cache. It does not establish cold-install success or cross-platform portability.

## Verification

- Rust workspace: **88 passed, 0 failed** across CLI, dependency management, compiler, model/artifact, runtime, HTTP and Python bridge suites.
- Feed-digest acceptance: **6 passed, 0 failed** against the development CLI and again against the extracted RC31 executable. Tests cover Python output parity, duplicates, Unicode, malformed input, a 404 with partial success, changed feed content, URL-order independence, denied hosts and actual overlap of two requests in both implementations.
- `cargo fmt --all -- --check`: passed.
- `cargo clippy --offline --workspace --all-targets -- -D warnings`: passed.
- Package linkage: macOS ARM64 Mach-O; only `/usr/lib/libiconv.2.dylib` and `/usr/lib/libSystem.B.dylib` are dynamically linked. The archive contains uv 0.13.4 and target-specific license/notice files for 121 resolved Rust dependencies.

The checks used a loopback-only mock server. No public feeds were contacted, no signing credentials were used, and no throughput comparison was run. The Python comparison is a correctness and concurrency comparison, not evidence that one implementation is faster.

## Concrete gaps found and fixed

The first real integration pass caught three bugs that compile-only checks missed: a draft used an unavailable map helper and an invalid integer/text concatenation; duplicate-entry choice changed with feed URL order; and the isolated Python sidecar could not import project-local adapters. The final source uses supported map iteration and typed output, stable duplicate rules, and an explicit project-root import path. A regression test reproduced the Python import failure before the bridge change.

## Limits that still matter

- The recommendation protocol is a draft; no independent briefs, builders or evaluation sessions have been run. Sales aggregation and feed digest are known development tasks.
- The network API supports bounded UTF-8 GET only. It lacks general methods, headers, request bodies, binary responses and streaming. DNS rebinding and OS-level egress confinement need controls outside the language runtime.
- Python remains host code, not sandboxed guest code. Native extension relocation and a bundled CPython/wheel environment remain unsolved.
- The archive targets macOS ARM64 only. It is unsigned and not notarized.
- The implementation uses Rust for the compiler, VM and host runtime. Rust is a defensible implementation choice for this systems layer, but Rust itself is not what makes Veritas useful; measured developer experience and task outcomes must justify the added language.

## Next milestone

Build one materially different user application that exercises modules, package-backed computation and a follow-up requirement. Then freeze the docs/tooling and use the preregistered comparison in `docs/evaluation-protocol.md` with independent briefs and equivalent Python tooling. Keep the verdict “insufficient evidence” until the outcomes justify a task-specific recommendation.

## Files

- Package: [veritas-0.1.0-darwin-arm64.tar.gz](veritas-0.1.0-darwin-arm64.tar.gz)
- SHA-256: `0ca58d7012cf79e582a6da8d8b3cc4b5e9cd07e1b57e27dded35b1da54d9e258`
- Executable package directory: [veritas-0.1.0-darwin-arm64](veritas-0.1.0-darwin-arm64/)
- Machine-readable acceptance record: [feed-digest-acceptance.json](feed-digest-acceptance.json)
- Source checkpoint: [veritas_checkpoint_v1_13_feed_digest.md](veritas_checkpoint_v1_13_feed_digest.md)
