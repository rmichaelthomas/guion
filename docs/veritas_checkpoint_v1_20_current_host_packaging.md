# Veritas checkpoint v1.20: current-host application packages

**Status:** active build checkpoint. This advances the approved north-star work; it does not establish that an assistant should recommend Veritas over Python.

## Objective

Turn `veritas package` into a usable delivery path for ordinary Veritas projects, including projects that call managed Python. Preserve the source-free artifact model, explicit resource declarations, pinned dependency inputs, useful package reporting, and same-host relocation.

## Implemented

- Added `veritas package PROJECT [--output DIRECTORY]`. It compiles the project, includes the matching Veritas executable and project policy, copies only declared resources, stages before installation, refuses overwrite and package-owned path collisions, and writes `PACKAGE.txt` plus machine-readable `PACKAGE.json`.
- Python-assisted packages include the pinned managed CPython installation, pinned `uv`, the locked bridge project, and the offline resolver cache. The package carries no Veritas source files unless they are explicitly declared as resources.
- Python modules needed by a project can be listed in `[package].resources` with data files. The `feed_inbox` app declares both its parser module and sample feed this way.
- Normalized the managed Python install's absolute staging alias into a relative link. Package metadata reports the CPython tree SHA-256 and byte size, `uv` SHA-256, Python lock SHA-256, target, Veritas artifact/runtime hashes, and resource hashes.
- Added an end-to-end acceptance case that packages `feed_inbox`, moves the package into a path containing spaces and Unicode, runs without the source tree or external Python/uv settings, imports its declared local Python module and locked `feedparser`, writes persistent state, and starts a second offline invocation.
- Exercised the RC39 release archive itself with the existing `python_interop` application. Its compiled `orjson` and NumPy wheels and pure-Python `requests` dependency passed after relocation into a path with spaces, with external Python/uv settings removed; the app ran twice with the expected output.

## Evidence

- `cargo fmt --all --check`: passed.
- `cargo clippy --workspace --all-targets --offline -- -D warnings`: passed.
- `cargo test --workspace --offline` with the pinned `VERITAS_UV`: 105 passed, 0 failed (including the native-wheel package test, compiler, model, runtime, HTTP, and Python bridge suites).
- `python3 -m unittest discover -s evaluation/tests -v` with the rebuilt CLI and pinned uv: 20 passed, 0 failed.
- The packaging integration tests verify runtime/resource metadata, a relative managed-Python alias, source-free relocation, no reliance on a caller-provided Python or uv path, and repeated offline launch. They cover `feedparser` and two native extension wheels (`orjson` and NumPy).
- Full app interoperability is covered by the existing feed-inbox evaluation, including Python parity and concurrent HTTP feed refresh against the local controlled server.

## Limits and open risks

- Packaging has only been executed and verified on macOS ARM64. Linux, Windows, cross-host delivery and archive installation are unverified.
- The package acceptance covers two native extension wheels on this host, not arbitrary compiled wheels. Their complete shared-library closure, licenses/notices, and system-library assumptions need explicit audit before claiming general Python package portability.
- `PACKAGE.json` records build-time hashes. The first relocated run may rebuild the Python virtual environment from its bundled offline cache; verification of post-launch integrity is not implemented.
- Resources are explicit. A project must list its own Python modules, templates, data files and other runtime inputs; unlisted project files are not copied.
- The Veritas runtime and bridge still impose host-process and Python interoperability limits described in the language reference. Current packaging does not imply OS-level sandboxing or broad Python compatibility.
- This is not evidence of recommendation advantage, broad ecosystem adoption, or performance superiority. The controlled acceptance programs remain author-created workloads.

## Next north-star work

Finish distribution-quality package validation: native-wheel and license reporting, integrity checking, installation/archive behavior, and clean-host tests across supported targets. In parallel, move beyond author-written workloads to an independent task set and assistant-choice evaluation; recommendation must follow observed success, repairability, setup friction, and developer preference against Python and suitable compiled-language baselines.
