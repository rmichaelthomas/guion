# Third-party notices

Guion's own source is offered under the MIT License in [`LICENSE`](LICENSE). Third-party components keep their own license terms; the Guion license does not replace them.

## Rust dependencies

The exact Rust dependency graph is recorded in [`Cargo.lock`](Cargo.lock). Release packaging copies license and notice files from the locked dependency sources and writes a target-specific `RUST-DEPENDENCIES.md` using [`distribution/collect-rust-licenses.py`](distribution/collect-rust-licenses.py). The collector fails if it cannot find a license notice for a resolved dependency. The bundled Apache-2.0-with-LLVM-exception text is in [`distribution/licenses/`](distribution/licenses/).

## Other bundled or evaluated components

- The pinned `uv` tool's Apache-2.0 and MIT license texts are retained in [`distribution/licenses/`](distribution/licenses/).
- The bundled libical component is LGPL-2.1-or-later; its notice is retained at [`native/vendor/libical-COPYING.LESSER.txt`](native/vendor/libical-COPYING.LESSER.txt) and its exact source checksum is listed in `distribution/native-components.lock.json`.
- The optional Python-assisted package notices for Tcl and Tk are retained separately there; their upstream source references and SHA-256 values are in [`distribution/licenses/UPSTREAM-NOTICES.md`](distribution/licenses/UPSTREAM-NOTICES.md).
- The `pyperformance` license file applies to the upstream benchmark material under `evaluation/development/famous_benchmarks/upstream/`; that material is evaluation input, not part of the Guion CLI release by default.
- Native component identities, versions, and source checksums are recorded in [`distribution/native-components.lock.json`](distribution/native-components.lock.json). Target release archives include their generated dependency and notice records.

For a distributed archive, use the notices shipped inside that exact target archive. A source-tree inventory or a lockfile alone is not a substitute for the license files in the archive.
