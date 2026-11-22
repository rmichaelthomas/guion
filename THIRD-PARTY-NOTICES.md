# Third-party notices

Guion's own source is offered under the MIT License in [`LICENSE`](LICENSE). Third-party components keep their own license terms; the Guion license does not replace them.

## Rust dependencies

The exact Rust dependency graph is recorded in [`Cargo.lock`](Cargo.lock). Release packaging copies license and notice files from the locked dependency sources and writes a target-specific `RUST-DEPENDENCIES.md` using [`distribution/collect-rust-licenses.py`](distribution/collect-rust-licenses.py). The collector fails if it cannot find a license notice for a resolved dependency. The bundled Apache-2.0-with-LLVM-exception text is in [`distribution/licenses/`](distribution/licenses/).

### Compression and archives

`std:compress` and `std:tar` use these pure-Rust crates, pinned exactly in the workspace `Cargo.toml` and linked into the native runtime only (not the browser VM). None of them needs a C compiler or a system zlib. The collector above copies each crate's own license files into release archives.

- `flate2` 1.1.10 (MIT OR Apache-2.0), with its `miniz_oxide` 0.9.1 backend (MIT OR Zlib OR Apache-2.0), which in turn uses `adler2` 2.0.1 (0BSD OR MIT OR Apache-2.0), `simd-adler32` 0.3.10 (MIT) and `crc32fast` 1.5.2 (MIT OR Apache-2.0).
- `tar` 0.4.46 (MIT OR Apache-2.0), with `filetime` 0.2.29 (MIT OR Apache-2.0).

### Cryptography

`guion-crypto` (the `Crypto` instruction behind `std:crypto`'s SHA-512 and SHA3-512) uses these pure-Rust RustCrypto crates, pinned exactly in the workspace `Cargo.toml`. They need no C compiler and build for the browser VM as well as the native runtime.

- `sha2` 0.10.9 (MIT OR Apache-2.0), also used elsewhere in the workspace, with `digest` 0.10.7 (MIT OR Apache-2.0).
- `sha3` 0.10.8 (MIT OR Apache-2.0), with `keccak` 0.1.6 (Apache-2.0 OR MIT).

## Other bundled or evaluated components

- The pinned `uv` tool's Apache-2.0 and MIT license texts are retained in [`distribution/licenses/`](distribution/licenses/).
- The bundled libical component is LGPL-2.1-or-later; its notice is retained at [`native/vendor/libical-COPYING.LESSER.txt`](native/vendor/libical-COPYING.LESSER.txt) and its exact source checksum is listed in `distribution/native-components.lock.json`.
- The optional Python-assisted package notices for Tcl and Tk are retained separately there; their upstream source references and SHA-256 values are in [`distribution/licenses/UPSTREAM-NOTICES.md`](distribution/licenses/UPSTREAM-NOTICES.md).
- The `pyperformance` license file applies to the upstream benchmark material under `evaluation/development/famous_benchmarks/upstream/`; that material is evaluation input, not part of the Guion CLI release by default.
- Native component identities, versions, and source checksums are recorded in [`distribution/native-components.lock.json`](distribution/native-components.lock.json). Target release archives include their generated dependency and notice records.

For a distributed archive, use the notices shipped inside that exact target archive. A source-tree inventory or a lockfile alone is not a substitute for the license files in the archive.
