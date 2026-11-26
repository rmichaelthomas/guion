# Self-hosted compiler (historical artifact)

**Status: stored, frozen, unused.** The self-hosted compiler in `language/self_host/` (`compiler.gu` and its imported sources, the checked artifact `compiler.gub`, and `compiler-seed.json`) is a historical proof that Guion once compiled itself: a compiler written in Guion, built by a Rust stage-zero tool, reproduced itself byte for byte over three stages. It is kept in the repository as a record of that result.

Nothing uses it. The `guion` executable compiles source with the Rust `guion-compiler` crate and runs it on the Rust runtime. The build, the default test suite, CI, the release checks and the release archives do not read, rebuild, compare, hash or ship these files, and a version bump, an artifact-format change or a release never requires touching them. The files are not updated for new language features, and the compiler rejects or mis-encodes features added after the freeze (closures, enums, generic functions, `std:crypto` operations and others). Do not add tests for it or keep it in step with the Rust compiler.

## Replaying the proof by hand

This is a manual tool for anyone who wants to repeat the historical result. It is not part of any gate.

The Rust stage-zero implementation is the `guion-stage0` binary in `crates/bootstrap`. From the repository root:

```sh
cargo run --locked --offline -p guion-bootstrap --bin guion-stage0 -- seed .
```

This compiles `language/self_host/compiler.gu` and its imported sources with the Rust stage-zero compiler and rewrites `language/self_host/compiler.gub` and `compiler-seed.json`. Running it changes the stored files, so use a scratch checkout and discard the result unless you mean to replace the artifact. The manifest records the source-file list and source-graph digest, the artifact digest, the artifact-format schema digest, the artifact version, the host ABI version and this command.

`guion-stage0 assemble-ir <input.gui.json> <output.gub>` assembles a compiler IR file into an artifact with the Rust writer. The artifact encoder and runtime protocol are versioned in `language/artifact-format.md` and `guion_model::Artifact::CURRENT_VERSION`; the stored seed is at an older format and is not expected to match the current one.
