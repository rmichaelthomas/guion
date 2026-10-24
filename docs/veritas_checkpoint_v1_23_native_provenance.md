# Veritas checkpoint v1.23 — native and runtime provenance

## Change

Python-assisted app packages now report both Python distributions and native runtime images:

- `PYTHON-DEPENDENCIES.json` includes distributions from the locked project environment and the managed interpreter. This captures bundled pip metadata and its `.dist-info/licenses/` files as well as project wheels. Runtime components include the managed CPython license, uv's MIT/Apache notices, and Tcl/Tk 9.0 `license.terms`.
- `NATIVE-DEPENDENCIES.json` scans the Veritas runtime, pinned uv, managed CPython, installed native Python extensions, and declared package resources. It recognizes Mach-O magic bytes even when a file lacks a conventional extension; records each image's package-relative path, SHA-256, byte size, target architectures and direct load edges; and classifies bundled versus operating-system libraries. It parses `otool -D` to separate an image's own install ID from load edges, and reads `LC_RPATH` values to resolve `@rpath` dependencies.
- Packaging fails when a native load edge is unresolved, ambiguous, outside the package and operating-system library roots, or lacks the current target architecture. `PACKAGE.json` stores the native report digest. `verify-package` checks the report and each listed native image and license document before launch.
- The package resource validator reserves both report names and the shared license directory.

The Tcl and Tk notices are kept as separate source files because their copyright lists differ. Their upstream URLs and SHA-256 values are recorded in `distribution/licenses/UPSTREAM-NOTICES.md`. The license files are preserved verbatim; their inclusion is not a legal interpretation.

## Evidence

- A first test run failed because `pip` was absent from the dependency report. The integration assertions were then implemented and pass.
- Native parser tests cover Mach-O install IDs, multiple `LC_RPATH` entries, and magic-based image discovery.
- The native-wheel package integration passes with NumPy, orjson and requests. Its report includes pip, CPython, uv and Tcl/Tk; it identifies the bundled Tcl/Tk dylibs and has no unresolved direct references. The app relocates, verifies and launches twice. Tampering with an orjson license, an orjson Mach-O image, or the Python runtime is rejected.
- Pure-Veritas current-host packaging passes with the same native scan enabled.
- `cargo fmt --all --check`: passed.
- `cargo clippy --workspace --all-targets --offline -- -D warnings`: passed.
- `cargo test --workspace --offline`: 107 passed, 0 failed.
- `python3 -m unittest discover -s evaluation/tests -v`: 20 passed, 0 failed against the updated debug CLI.
- The shipped RC43 binary built a native-wheel app bundle. Its report listed 8 Python distributions (including pip), all 8 with complete metadata and 0 missing-license warnings; it copied 47 distribution license files plus 5 CPython/uv/Tcl/Tk runtime notice files. It recorded 34 native images, 2 bundled Tcl/Tk libraries, 23 system-library paths and 0 unresolved load references. The report and all image/notice hashes verified after relocation to a path with spaces and Unicode; the app launched twice offline.
- The shipped RC43 archive checksum verified. Its included task-ledger package verified and accepted a state-changing command.
- Evidence applies to macOS ARM64 and the pinned Python 3.14.8/uv 0.13.4 toolchain.

## Audit boundaries

The scanner walks static Mach-O load commands; it cannot prove libraries loaded only through runtime APIs such as `dlopen`. `.uv-cache` is excluded from the native-image graph because the application loads the installed environment, not the offline wheel cache. The cache and full virtual environment are not independently hashed as trees. System-library paths are recorded but OS files are not redistributed in the app. The report does not yet prove binary ownership against package `RECORD` metadata, audit every wheel-cache copy, or include the Veritas runtime's complete Rust dependency notice set in standalone app bundles. Package reports remain unsigned and do not authenticate the publisher. Cross-platform packaging and legal compatibility assessment remain open.

## Next

Attach each bundled native image to its owning distribution using installed `RECORD` metadata and carry every required license source alongside it. Embed the Rust runtime's third-party notices in independently distributable app bundles. Extend coverage to dynamic loader APIs and the offline wheel cache, then repeat the relocation and integrity matrix on clean hosts and additional targets. Continue with independent builder evaluations; this package milestone does not establish that an assistant should recommend Veritas over Python.
