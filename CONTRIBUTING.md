# Contributing

Guion is a pre-release project. Contributions should keep the implemented language contract clear and preserve the evidence behind compatibility and performance claims.

## Development setup

Install Rust and Cargo, clone the repository, and use the locked dependency versions. The default core build does not require Python. Python interoperability is an explicit feature for work that needs the bridge.

## Before submitting a change

From the repository root, run the checks relevant to the change:

```sh
cargo xtask artifact-format-audit
cargo fmt --all -- --check
cargo check --locked --offline --workspace
cargo test --locked --offline --workspace
```

A change to the native code arena (`crates/runtime/src/native_arena.rs`) or to how generated code holds pointers into it is also checked with Miri, locally and under both aliasing models (there is no CI job for it). It needs the nightly toolchain with the `miri` component:

```sh
CARGO_TARGET_DIR=target/miri MIRIFLAGS="-Zmiri-disable-isolation" \
  cargo +nightly miri test -p guion-runtime --lib native_inline_arena_tests
CARGO_TARGET_DIR=target/miri MIRIFLAGS="-Zmiri-disable-isolation -Zmiri-tree-borrows" \
  cargo +nightly miri test -p guion-runtime --lib native_inline_arena_tests
```

Miri stops at the first undefined behaviour in a test binary, so name a single test after `native_inline_arena_tests::` to see each one.

Changes to the language, artifact reader, runtime, standard library, or packaging path may need the matching `cargo xtask` conformance, runtime, product, release, and closeout checks. The approved pre-release plan lists the full matrix. Do not describe a target as passing if it was skipped.

Artifact-format assertions in Rust tests must compare against `guion_model::Artifact::CURRENT_VERSION`, not a numeric format literal. The conformance, standard-library, runtime, product, release, performance, and builders gates run this source audit before their longer checks; `cargo xtask artifact-format-audit` runs the quick check by itself.

## Release hygiene checks

These keep a MINOR release additive (see [VERSIONING.md](VERSIONING.md)).

- **Standard-library surface.** `language/surface-0.4.0.json` is the output of `guion doc --all --json` for 0.4.0, kept as the snapshot later releases are compared against (`language/surface-0.3.0.json` stays as the previous one, and 0.4.0 is checked against it). To check a build: `guion doc --all --json > /tmp/now.json`, then `cargo xtask surface-diff --from language/surface-0.4.0.json --to /tmp/now.json`. The command lists additions and exits 0; it exits 1 and names each module, function, record or record field that was removed or re-signed. A function is re-signed when its parameter names, parameter types, result type or effects differ. The `deprecated` note, `source` and summaries are not compared.

- **Docs name the release.** `shipped_getting_started_names_this_release` (`crates/cli/tests/release_hygiene.rs`) requires every version `docs/getting-started.md` names to be the `X.Y.Z` core of `[workspace.package].version` in `Cargo.toml`, so `0.4.0-rc.1` is documented as `0.4.0`. While the changelog's top section is marked `(unreleased)`, the page may also name that section's version, so it can be updated before the version bump. Anything else fails.

- **Outside program.** After `cargo build --locked --release -p guion-cli`, `cargo xtask outside-check` runs `evaluation/outside/jsonl-summary/run.py` and fails if the Guion and Python outputs differ or if the hello world, 5,000-row or 200,000-row Guion median is more than 5% slower than `baseline-0.2.0.json` (the noise allowance; see that directory's README). It does not build, because a few-millisecond hello world measures slow for a while after a build; re-run once before treating a single over-allowance result as a regression.

## Change expectations

- Keep the source of truth in the implementation and `language/spec.md`; do not duplicate API signatures by hand when a generated report can provide them.
- Add or update focused tests for behavior changes and preserve source spans and stable diagnostic codes.
- Keep capability grants explicit. Network, clock, file access, and optional Python use should not silently become broader.
- Preserve historical Veritas reports byte-for-byte. Add new Guion observations as a separate cohort.
- Update user docs, compatibility notes, changelog, and machine-readable reports when a public behavior or identity changes.
- Do not add or edit a project-level `AGENTS.md` without the repository owner's specific approval.

## Reporting a change

Describe what changed, the user-visible effect, commands run, and any target or evidence that remains unverified. Keep the report factual and scoped to the observed result.
