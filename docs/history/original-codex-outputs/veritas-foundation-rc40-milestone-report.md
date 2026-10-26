# Veritas RC40 milestone report

**Result:** the current-host foundation now builds and relocates source-free Veritas apps with managed Python. It has passed an end-to-end package test with pure-Python dependencies and with compiled `orjson` and NumPy wheels on macOS ARM64.

## What changed

`veritas package` now produces a runnable app directory containing the matching Veritas runtime, source-free artifact, policy, declared project resources, managed CPython, pinned `uv`, the locked Python bridge, and an offline dependency cache. Its `PACKAGE.json` records artifact/runtime/resource hashes, the lock identity, and the managed Python tree hash and size. The package refuses overwrite and resource paths that collide with package files.

The Python-assisted feed-inbox app declares its local parser module and sample feed as resources. The compiled-wheel interop app exercises `orjson`, NumPy, and `requests`. Both run after relocation without relying on the original source tree or caller-provided Python settings. The native-wheel app ran twice after relocation and produced the expected output both times.

## Verification

- `cargo fmt --all --check`: passed.
- `cargo clippy --workspace --all-targets --offline -- -D warnings`: passed.
- `cargo test --workspace --offline` with pinned `VERITAS_UV`: 105 passed, 0 failed.
- `python3 -m unittest discover -s evaluation/tests -v`: 20 passed, 0 failed.
- RC40 archive checksum verified; extracted CLI `doctor` passed; shipped task-ledger package created and listed state successfully.
- RC40 itself packaged, relocated, and ran the Python interop example twice. `otool -L` showed the tested orjson and NumPy extensions using macOS system libraries/frameworks listed in [packaging-rc40.json](/Users/rmichaelthomas/Documents/Codex/2026-10-22/referenced-chatgpt-conversation-this-is-an/work/veritas-foundation/evaluation/results/packaging-rc40.json).

## Release candidate

- Archive: [Veritas 0.1.0 for macOS ARM64](</Users/rmichaelthomas/Documents/Codex/2026-10-22/referenced-chatgpt-conversation-this-is-an/outputs/veritas-foundation-rc40/veritas-0.1.0-darwin-arm64.tar.gz>)
- SHA-256: `8f2160467ec7c395d3a1be5f04b3e5522314a080ff5e1a65fa89a5442835d8df`
- Detailed checkpoint: [v1.20 current-host packaging](</Users/rmichaelthomas/Documents/Codex/2026-10-22/referenced-chatgpt-conversation-this-is-an/outputs/veritas_checkpoint_v1_20_current_host_packaging.md>)
- Machine-readable evidence: [packaging-rc40.json](/Users/rmichaelthomas/Documents/Codex/2026-10-22/referenced-chatgpt-conversation-this-is-an/work/veritas-foundation/evaluation/results/packaging-rc40.json)

## Verdict and remaining work

This is a meaningful deployment proof for a narrow host and dependency sample. It does not yet support recommending Veritas over Python for a general builder task class. Only macOS ARM64 has been tested; arbitrary native wheel library closure, Python dependency license collection, post-launch integrity verification, archive installation, and other host targets remain open. The comparative evaluation also has no independent builders or assistant sessions yet. Use Python when broad package compatibility or cross-platform delivery is the main requirement; Veritas is now a candidate for testing where a single bundled current-host app and typed Veritas/Python boundary are useful.

The next build work is to finish package dependency and license reporting, validate runtime integrity after relocation, and expand clean-host coverage. The recommendation gate stays open until the preregistered independent evaluation has evidence across multiple task classes.
