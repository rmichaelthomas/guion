# Self-hosted compiler and seed updates

The normal `veritas` executable embeds `language/self_host/compiler.vrb`. It invokes the Veritas compiler's `compiler_compile_project` entry for source checking, formatting validation, testing, running source, building, documentation, inspection, and packaging. It has no normal Cargo dependency on `veritas-compiler` and does not recover from compiler errors by invoking the Rust source compiler.

The Rust implementation is retained as explicitly named developer tooling: the `veritas-stage0` binary in `crates/bootstrap`. It creates the first compiler artifact from `language/self_host/compiler.vr` and its imported source graph. The checked artifact seed is then verified through the native reader and reproduced by successive Veritas compiler stages.

## Reproducible seed update

From the repository root, run:

```sh
cargo run --locked --offline -p veritas-bootstrap --bin veritas-stage0 -- seed .
cargo xtask bootstrap --stages 3
cargo xtask conformance --compilers stage2,stage3
```

The first command rebuilds `language/self_host/compiler.vrb` and `compiler-seed.json` using only the explicit stage-zero package. The manifest records the source-file list and source graph digest, seed digest, artifact-format schema digest, artifact version, host ABI version, and this regeneration command. The CLI build script verifies all of those identities before compiling the normal binary. At startup, the CLI checks the embedded bytes against the manifest, format, host ABI, and embedded schema contract. An identity mismatch is a hard error; no development-path fallback exists.

The bootstrap command compiles the compiler source with stage zero to produce C1, asks C1 to compile that source to C2, and asks C2 to produce C3. It verifies each artifact with the native reader and requires exact C2/C3 bytes for the same relative source graph. It saves the verified C2 and C3 artifacts under the disposable `target/self-hosted-bootstrap/` directory for stage-specific conformance. The conformance command checks the full current Task 2 corpus through both stages.

`veritas-stage0 assemble-ir` remains available for byte-level differential checks. Product source compilation does not use that path; `language/self_host/artifact.vr` owns artifact field ordering and emission. The artifact encoder and runtime protocol are versioned in `language/artifact-format.md` and `veritas_model::Artifact::CURRENT_VERSION`.
