# Self-hosted compiler and seed updates

**Status (2026-11-14): frozen.** The self-hosted compiler in `language/self_host/` is a historical proof artifact: Guion compiled itself, and three stages reproduce byte for byte. It is not updated for new language features and the product never compiles with it; `guion` uses the Rust `guion-compiler` crate. Features added after the freeze (closures, enums, generic functions and everything after them) are Rust-compiler only, and the self-hosted compiler rejects them. New tests for those features belong in Rust-only tests, not in the shared `compiler_conformance` set. One mechanical duty remains: the CLI build script verifies `compiler.gub` against `compiler-seed.json`, which records the digest of `language/artifact-format.md`, so a change to that file needs the seed regenerated with the command below.

The normal `guion` executable compiles source with the Rust `guion-compiler` crate, which has been the faster path since commit 7a909f7. The self-hosted compiler in `language/self_host/compiler.gu` must produce the same programs: its checked artifact `compiler.gub` is verified by the CLI build script against `compiler-seed.json`, and the bootstrap and conformance runs below compare it stage against stage. A language or code-generation change therefore goes into both compilers, followed by a seed update.

The Rust implementation is retained as explicitly named developer tooling: the `guion-stage0` binary in `crates/bootstrap`. It creates the first compiler artifact from `language/self_host/compiler.gu` and its imported source graph. The checked artifact seed is then verified through the native reader and reproduced by successive Guion compiler stages.

## Reproducible seed update

From the repository root, run:

```sh
cargo run --locked --offline -p guion-bootstrap --bin guion-stage0 -- seed .
cargo xtask bootstrap --stages 3
cargo xtask conformance --compilers stage2,stage3
```

The first command rebuilds `language/self_host/compiler.gub` and `compiler-seed.json` using only the explicit stage-zero package. The manifest records the source-file list and source graph digest, seed digest, artifact-format schema digest, artifact version, host ABI version, and this regeneration command. The CLI build script verifies all of those identities before compiling the normal binary. At startup, the CLI checks the embedded bytes against the manifest, format, host ABI, and embedded schema contract. An identity mismatch is a hard error; no development-path fallback exists.

The bootstrap command compiles the compiler source with stage zero to produce C1, asks C1 to compile that source to C2, and asks C2 to produce C3. It verifies each artifact with the native reader and requires exact C2/C3 bytes for the same relative source graph. It saves the verified C2 and C3 artifacts under the disposable `target/self-hosted-bootstrap/` directory for stage-specific conformance. The conformance command checks the full current Task 2 corpus through both stages.

`guion-stage0 assemble-ir` remains available for byte-level differential checks. Product source compilation does not use that path; `language/self_host/artifact.gu` owns artifact field ordering and emission. The artifact encoder and runtime protocol are versioned in `language/artifact-format.md` and `guion_model::Artifact::CURRENT_VERSION`.
