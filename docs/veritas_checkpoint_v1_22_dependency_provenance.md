# Veritas checkpoint v1.22 — Python dependency provenance

## Change

Python-assisted application bundles now include `PYTHON-DEPENDENCIES.json`. It lists the distributions in the project's locked bridge environment, their declared names and versions, `License-Expression`/legacy `License` metadata, license status, missing declared license files, and the SHA-256 and byte size of each copied license document. Documents are copied from each distribution's own `.dist-info` directory into `licenses/python/<distribution-version>/`.

`PACKAGE.json` records the report digest. `veritas verify-package` checks that report and every listed license file before the launcher starts the app. Resource declarations cannot overwrite the report or the reserved `licenses/` directory. Missing metadata remains visible as `incomplete`; it does not block packaging, because absence should be reported rather than guessed.

The copier rejects unsafe metadata paths, special files, and license paths whose resolved source escapes its own installed distribution. The feature preserves distribution-provided metadata; it does not determine license compatibility or legal obligations. The report covers the locked project environment only: it omits pip and its vendored dependencies bundled inside CPython, CPython/Tcl/Tk notices, and the uv notice inside each application bundle.

## Evidence

- `cargo fmt --all --check`: passed.
- `cargo clippy --workspace --all-targets --offline -- -D warnings`: passed.
- `cargo test --workspace --offline`: 105 tests passed, 0 failed. The run required network and localhost permissions for the already-approved CPython/wheel and HTTP acceptance cases.
- `python3 -m unittest discover -s evaluation/tests -v`: 20 tests passed, 0 failed.
- `package_runs_compiled_python_wheels_after_relocation_without_python_setup`: passed for the `numpy`, `orjson`, and `requests` distributions. The relocated bundle launched twice without external Python configuration; modifying a copied `orjson` license caused `verify-package` to reject it, as did modifying the bundled Python runtime.
- The RC42 archive checksum verified. Its shipped release binary created a Python app package; that package passed `verify-package` after relocation into a path with spaces and Unicode and ran twice offline. The report contained seven distributions, all marked complete with no warnings: NumPy (17 files), orjson (3), and one license document each for certifi, charset-normalizer, idna, requests, and urllib3.
- The RC42 archive's included source-free task-ledger bundle passed `verify-package` and successfully added a task.
- A one-off Mach-O trace examined 32 images in the shipped Python runtime and bridge environment. After excluding each image's `LC_ID_DYLIB` and resolving `LC_RPATH`, it found 2 bundled Tcl/Tk libraries, 23 distinct macOS system libraries, and 0 unresolved direct references. This was host-specific reconnaissance, not a shipped audit or a completeness proof for dynamically loaded code paths.
- The test uses macOS ARM64 and Python 3.14.8 with the pinned uv version. It does not establish behavior on another operating system or architecture.

## Current limits and follow-up

The report hash lives in unsigned package metadata, so it detects accidental changes but is not publisher authentication. The report inventories the locked project environment only. It omits bundled pip and its vendored libraries, the CPython/Tcl/Tk runtime notices, uv notices in app bundles, and license provenance for bundled native libraries. The one-off Mach-O trace is not an integrated check. The `.uv-cache` and full virtual environment are still not independently digested. Cross-host packages, archive extraction hardening, independent builders, and assistant recommendation evidence remain open under the implementation plan.

Next, add a repeatable macOS Mach-O audit that scans the app's Veritas runtime, uv, CPython, and project wheels; parses install IDs and runpaths; records direct load edges, bundled binary digests, system dependencies, and unresolved references; and fails closed on ambiguous or unresolved dependencies. Inventory CPython/pip and uv notices in the app itself, then associate bundled native libraries with available license sources. Keep the inventory separate from legal conclusions. Repeat packaging tests on clean target machines and add an independent review of resulting notices.
