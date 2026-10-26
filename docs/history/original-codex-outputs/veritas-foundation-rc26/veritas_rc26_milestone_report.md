# Veritas RC26 milestone report

**Date:** October 22, 2026  
**Status:** Integrated foundation build in progress; this is not a general Python-replacement or recommendation verdict.  
**Build root:** `work/veritas-foundation/`

## What changed

RC26 advances the portable artifact contract from format 15 to format 16. Versioned build metadata schema 1 records the Veritas compiler version, build host, numeric runtime ceilings, a SHA-256 identity for the exact source graph parsed by the compiler, and an optional combined identity for the Python lock files.

The compiler hashes the source buffers it parsed, paired with sorted project-relative module paths and length-delimited bytes. Builds from unchanged inputs and policy produced byte-identical artifacts on this host. Editing an imported source module changed the source digest. Editing `uv.lock` changed the dependency-lock digest while leaving the source digest unchanged. A partial lock-file pair fails the build.

`veritas inspect` reports this metadata from `.vrb` files. The runtime reads format 14, 15, and 16 artifacts. Format 14 has no build metadata; format 15 metadata is decoded as legacy schema 0. For embedded numeric limits, the runtime takes the stricter value between the artifact and host policy.

## Evidence

- The offline workspace suite passed **76 Rust tests**, including the managed CPython integration using the pinned uv toolchain.
- `cargo fmt --all -- --check`, `cargo clippy --offline --workspace --all-targets -- -D warnings`, and `python3 -m unittest discover -s evaluation/tests -v` passed; the Python evaluation harness has 2 tests.
- The known sales-report case matched its Python standard-library reference: both printed `12` and `1524`. The machine-readable report is `development-sales-report.json`. This is a known development case, not independent evidence or a performance comparison.
- The extracted RC26 macOS ARM64 package passed `doctor`, matching, records, and modules examples. A program built with a 1,024-byte allocation limit was copied without its project/source, inspected to confirm format 16, metadata schema 1, source digest, absent dependency locks, and the limit; execution failed with `R_ALLOCATION_LIMIT` as designed.
- The source archive was extracted to a fresh directory and passed all 76 workspace tests, including managed CPython integration.
- The `.vrb` move test and source/lock identity tests are in the CLI suite; the model test covers format 16 metadata plus format 15 and 14 compatibility.

## Artifacts

| Deliverable | SHA-256 |
|---|---|
| `veritas-0.1.0-darwin-arm64.tar.gz` | `88a5998dedcc55fd84305263dbc6c0b49f9c9ccef3666dd89c9c2fdff9244bef` |
| `veritas-foundation-source-rc26.tar.gz` | `67bde513b622f0bba67f96a8c27a2c39c357215a1e9317419696be67b26e55d7` |

Each archive has an adjacent SHA-256 sidecar. The source archive contains editable Rust sources, examples, docs, tests, and evaluation assets; it excludes compiled targets, Git metadata, and generated virtual environments.

## Limits and next work

The runnable package targets macOS ARM64 only. It is unsigned and does not include a relocatable CPython or wheel environment. Build hashes identify input bytes but do not authenticate a publisher. Runtime allocation limits estimate language-managed allocation; they are not exact process-memory limits or an OS sandbox. Metadata does not yet record Veritas package dependency identities, full compiler compatibility ranges, native ABI/library requirements, or selectable native compilation targets.

The current native-language boundary remains the Python sidecar described in `language/python-abi.md`; a native C/FFI interface still needs an explicit type, ownership, capability, and platform contract. HTTP and other host operations remain incomplete. Most importantly, Veritas has no independent multi-class recommendation evidence yet. The next work remains to close the approved lifecycle and host-operation gaps, then run the pre-registered comparison against Python with equal tools and task access.
