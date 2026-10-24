# Veritas checkpoint v1.24 — standalone Rust dependency notices

## Change

When `veritas package` runs from a prepared Veritas distribution, the app bundle now carries that distribution's `licenses/RUST-DEPENDENCIES.md` inventory and complete `licenses/rust/` tree. `PACKAGE.json` records the inventory digest and the notice-tree digest and size. `verify-package` checks both before launch. Packaging fails when only part of the expected notice set is present or when the source inventory is not a regular file. Development builds without a sibling distribution license set remain packageable and record `rust_notices: null`.

The distribution's task-ledger app is built and verified during release assembly, exercising notice discovery through the actual shipped runtime. This carries notices for the Rust runtime; it does not establish legal compatibility or publisher authenticity.

## Evidence

- `cargo fmt --all --check` and `cargo clippy --workspace --all-targets --offline -- -D warnings` passed.
- `cargo test --workspace --offline`: 109 passed, 0 failed with the pinned uv executable configured.
- `python3 -m unittest discover -s evaluation/tests -v`: 20 passed, 0 failed with the pinned uv, managed CPython installation and offline cache configured.
- Release assembly inventoried 121 locked Rust dependencies and copied 207 notice files (974,963 bytes) into the source-free task-ledger app. `verify-package` passed; changing a copied license caused verification to reject the app.
- The final archive checksum passed. After relocation to a path containing spaces and Unicode, the nested app verified and its state-changing `add` command succeeded.

## Limits

The Rust notice list is generated from the release lockfile and the copied upstream license files. The package hashes detect accidental changes but are unsigned. Native image ownership, dynamic loader calls, offline-cache hashing, cross-platform packaging, and independent builder recommendation remain open.
