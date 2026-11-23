# Third-party notices

Guion's own source is offered under the MIT License in [`LICENSE`](LICENSE). Third-party components keep their own license terms; the Guion license does not replace them.

## Rust dependencies

The exact Rust dependency graph is recorded in [`Cargo.lock`](Cargo.lock). Release packaging copies license and notice files from the locked dependency sources and writes a target-specific `RUST-DEPENDENCIES.md` using [`distribution/collect-rust-licenses.py`](distribution/collect-rust-licenses.py). The collector fails if it cannot find a license notice for a resolved dependency. The bundled Apache-2.0-with-LLVM-exception text is in [`distribution/licenses/`](distribution/licenses/).

### Compression and archives

`std:compress` and `std:tar` use these pure-Rust crates, pinned exactly in the workspace `Cargo.toml` and linked into the native runtime only (not the browser VM). None of them needs a C compiler or a system zlib. The collector above copies each crate's own license files into release archives.

- `flate2` 1.1.10 (MIT OR Apache-2.0), with its `miniz_oxide` 0.9.1 backend (MIT OR Zlib OR Apache-2.0), which in turn uses `adler2` 2.0.1 (0BSD OR MIT OR Apache-2.0), `simd-adler32` 0.3.10 (MIT) and `crc32fast` 1.5.2 (MIT OR Apache-2.0).
- `tar` 0.4.46 (MIT OR Apache-2.0), with `filetime` 0.2.29 (MIT OR Apache-2.0).
- `ruzstd` 0.9.1 (MIT), a zstd decoder with no dependencies and no C code, built with default features off except `hash`, which verifies the zstd content checksum and pulls in `twox-hash` 2.1.4 (MIT, no dependencies, XXH64 only). The native runtime uses it to read a `Content-Encoding: zstd` HTTP response body; the browser VM does not link it.

### Cryptography

`guion-crypto` (the `Crypto` instruction behind `std:crypto`'s SHA-512, SHA3-512, RSA and ECDSA P-256 verification and X.509 key reading) uses these pure-Rust RustCrypto crates, pinned exactly in the workspace `Cargo.toml`. They need no C compiler and build for the browser VM as well as the native runtime.

- `sha2` 0.10.9 (MIT OR Apache-2.0), also used elsewhere in the workspace, with `digest` 0.10.7 (MIT OR Apache-2.0).
- `sha3` 0.10.8 (MIT OR Apache-2.0), with `keccak` 0.1.6 (Apache-2.0 OR MIT).
- `rsa` 0.9.10 (MIT OR Apache-2.0), with `num-bigint-dig`, `pkcs1` and `pkcs8` (Apache-2.0 OR MIT). Used only to verify signatures against public keys; the Marvin timing advisory RUSTSEC-2023-0071 concerns private-key decryption and signing, which Guion never does.
- `p256` 0.13.2 and `ecdsa` 0.16.9 (Apache-2.0 OR MIT), with `elliptic-curve`, `crypto-bigint`, `sec1` and `signature`. ECDSA P-256 verification.
- `p384` 0.13.1 (Apache-2.0 OR MIT), the NIST P-384 curve on the same `elliptic-curve` and `ecdsa` versions. ECDSA P-384 verification of certificate signatures (`crypto_x509_verify_issued_by`).
- `x509-cert` 0.2.5 (Apache-2.0 OR MIT), with `der`, `spki` and `const-oid`. Reads the public key of a DER certificate.
- `ed25519-dalek` 2.2.0 (BSD-3-Clause), with `curve25519-dalek` 4.1.3 (BSD-3-Clause), `curve25519-dalek-derive` 0.1.1 (MIT/Apache-2.0), `ed25519` 2.2.3 (Apache-2.0 OR MIT) and `fiat-crypto` 0.2.9 (MIT OR Apache-2.0 OR BSD-1-Clause), plus the build-time `rustc_version`; built with only the `zeroize` feature (not `std`, which would pull `rand_core/getrandom` into the browser build), so there is no random-number source. Ed25519 keys, signing and strict verification (`crypto_ed25519_public_key`, `crypto_ed25519_sign`, `crypto_ed25519_verify`).
- `hkdf` 0.12.4 (MIT OR Apache-2.0), with `hmac`. HKDF-SHA-256 for the age header MAC key, payload key and the timelock randomness derivation.
- `chacha20poly1305` 0.10.1 (Apache-2.0 OR MIT), with `chacha20`, `poly1305`, `cipher`, `aead`, `universal-hash`, `inout` and `opaque-debug`; built without the `getrandom` feature. ChaCha20-Poly1305 for the age payload in `crypto_tlock_encrypt` and `crypto_tlock_decrypt`.
- `ark-bls12-381`, `ark-ec`, `ark-ff` and `ark-serialize` 0.5.0 (MIT OR Apache-2.0), with `ark-std`, `ark-poly` and their helper crates (`educe`, `itertools`, `num-bigint`, `arrayvec`, `hashbrown`, `ahash`, `paste`). BLS12-381 pairing, hash to curve and point decoding for `crypto_bls12381_verify_g1`; built without the `std` feature so there is no random-number source.

## Other bundled or evaluated components

- The pinned `uv` tool's Apache-2.0 and MIT license texts are retained in [`distribution/licenses/`](distribution/licenses/).
- The bundled libical component is LGPL-2.1-or-later; its notice is retained at [`native/vendor/libical-COPYING.LESSER.txt`](native/vendor/libical-COPYING.LESSER.txt) and its exact source checksum is listed in `distribution/native-components.lock.json`.
- The optional Python-assisted package notices for Tcl and Tk are retained separately there; their upstream source references and SHA-256 values are in [`distribution/licenses/UPSTREAM-NOTICES.md`](distribution/licenses/UPSTREAM-NOTICES.md).
- The `pyperformance` license file applies to the upstream benchmark material under `evaluation/development/famous_benchmarks/upstream/`; that material is evaluation input, not part of the Guion CLI release by default.
- Native component identities, versions, and source checksums are recorded in [`distribution/native-components.lock.json`](distribution/native-components.lock.json). Target release archives include their generated dependency and notice records.

For a distributed archive, use the notices shipped inside that exact target archive. A source-tree inventory or a lockfile alone is not a substitute for the license files in the archive.
