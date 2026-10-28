# Current-format managed native-wheel package verification — 2026-10-27

## Result

The format-44 Guion CLI built a self-contained Python-assisted application on macOS ARM64. The existing integration test installed managed CPython, packaged NumPy and orjson native wheels plus the pure-Python requests package, checked the native-library and license inventories, moved the source-free package into a path containing spaces, and launched it twice after removing external Python and uv settings. Both launches returned the expected output.

This closes the same-host managed-install and relocation gate for this mixed-dependency fixture. It does not establish current-format Linux support, broad compatibility across Python packages, or managed packaging of the separate 100,000-row grouped-analysis workload.

## Verification

- `version_reports_artifact_and_pinned_timezone_data_versions`: 1 passed. The stale assertion expected format 42 while the CLI correctly reported 44; it now compares against the model and timezone-data constants.
- `package_runs_compiled_python_wheels_after_relocation_without_python_setup`: 1 passed in 88.42 seconds; 60 unrelated CLI tests were filtered out.
- The package test reads `app.gub` and asserts its artifact format equals `Artifact::CURRENT_VERSION` (44).
- The package contains managed CPython 3.14.8 and uv 0.13.4. The Python runtime is 68,121,972 bytes; the offline wheel cache is 23,433,579 bytes.
- Dependency inventory contains NumPy 2.5.4, orjson 3.13.0, requests 2.32.5, and the managed interpreter components with license records. Native images are attributed to their Python distributions; unresolved and unattributed native dependencies are empty.
- Package verification rejects tampered licenses, native images, owner records, the Python runtime, and the offline cache.
- After relocation, both launches ran with `GUION_UV`, `GUION_PYTHON`, `UV_PYTHON_INSTALL_DIR`, `UV_CACHE_DIR`, and `UV_OFFLINE` removed and produced:

  ```text
  15
  9007199254741000
  3
  python-requests/2.32.5
  ```

- `cargo fmt --all --check` passed.

The exact commands are:

```sh
cargo test --locked --offline --jobs 2 -p guion-cli --features python-interop --test cli version_reports_artifact_and_pinned_timezone_data_versions -- --exact
cargo test --locked --offline --jobs 2 -p guion-cli --features python-interop --test cli package_runs_compiled_python_wheels_after_relocation_without_python_setup -- --exact --nocapture
cargo fmt --all --check
```

The pinned uv 0.13.4 Apple Silicon archive was checked against SHA-256 `e0b1d4743ce64c55d9f0f4a444afa6b81125680070ee04e853fcd0f6c1ebf8f1` from the [official uv release](https://github.com/astral-sh/uv/releases/tag/0.13.4). The package test uses the retained migration-compatibility fixture at `evaluation/development/python_interop`; it builds and verifies the current artifact format.

## Scope and next gaps

The current 100,000-row NumPy/orjson timing still uses an explicit installed interpreter and remains slower than both Python comparisons. That workload has not yet been run from a relocated managed package. Current-format validation on Debian 12 ARM64 and x86_64, broader important-package coverage, bridge overhead, and language learnability remain open.

## Scratch hygiene

All downloaded tools, managed Python files, wheels, and temporary test files stayed under ignored `target/perf-session/managed-python-package`. The test removed its temporary application directory. After the run, transient uv lock files and the duplicate 14 MB archive were deleted; the 124 MB pinned toolchain/cache is retained to avoid downloading the same dependencies again. Available disk space after the complete run was 57 GB. No VM was started.
