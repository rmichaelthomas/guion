# Veritas to Guion rename inventory

Reviewed as part of migration Task 4 on 2026-10-25.

## Active surfaces renamed

- Cargo workspace packages and Rust imports: `guion-model`, `guion-compiler`, `guion-runtime`, `guion-cli`, and `guion-bootstrap`.
- Executables and commands: `guion` and `guion-stage0`.
- Language inputs and outputs: `.gu` source files and `.gub` compiled artifacts.
- Project files and generated state: `guion.toml`, `guion.lock`, and `.guion/`.
- Environment variables, CLI messages, machine report kinds, installer paths, package launchers, distribution scripts, self-hosted compiler sources, active examples, and current completion runners.

The scoped implementation inventory covered the repository root manifests and ignore rules; all source and tests under `crates/`; all release and completion logic under `xtask/`; installer, package builder, and installer checks under `distribution/`; active language documentation, examples, standard library, and self-hosted compiler under `language/`; and the two live completion runners in `evaluation/completion/`.

## Preserved Veritas identities

The frozen evaluation inputs, prior result JSON, checkpoints, original plan and sprint log remain Veritas-era records. They were not renamed or rewritten. The package builder converts copied examples into Guion names inside the release staging directory and leaves the retained source evidence untouched.

During the transition, Guion reads legacy `veritas.toml`, `veritas.lock`, `.veritas/python-bridge`, and `.vr` project inputs. New projects, packages, locks, state directories, and source files use Guion names. Existing `.vrb` bytecode remains readable through the unchanged `VRTA` artifact magic.

## Serialized compiler identity

The self-hosted seed was rebuilt from `language/self_host/compiler.gu` with:

```sh
cargo run --locked --offline -p guion-bootstrap --bin guion-stage0 -- seed .
```

Artifact format remains version 39, the artifact magic remains `VRTA`, and host ABI remains version 1. The rename did not change serialized instructions or host-call layout, so no format or ABI version bump was appropriate. The regenerated seed SHA-256 is recorded in `language/self_host/compiler-seed.json`.

The seed's `artifact_schema_sha256` changed because the current bootstrap hashes the complete artifact-format document, including its newly branded title and descriptions. That digest is a document identity check; it does not indicate a bytecode layout change.

## Task 4 verification

- `cargo check --locked --offline --workspace` passed.
- Guion project creation regression passed.
- Legacy Veritas project read regression passed.
- Installed create-to-run lifecycle passed.
- `cargo test --locked --offline -p guion-cli --test bootstrap compiler_stage_equality -- --exact --nocapture` passed (298.03 seconds); the regenerated self-hosted seed retains byte-for-byte three-stage equality.
