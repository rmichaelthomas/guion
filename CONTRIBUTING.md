# Contributing

Guion is a private pre-release project. Contributions should keep the implemented language contract clear and preserve the evidence behind compatibility and performance claims.

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

Changes to the language, compiler seed, artifact reader, runtime, standard library, or packaging path may need the matching `cargo xtask` conformance, bootstrap, runtime, product, release, and closeout checks. The approved pre-release plan lists the full matrix. Do not describe a target as passing if it was skipped.

Artifact-format assertions in Rust tests must compare against `guion_model::Artifact::CURRENT_VERSION`, not a numeric format literal. The conformance, bootstrap, standard-library, runtime, product, release, performance, and builders gates run this source audit before their longer checks; `cargo xtask artifact-format-audit` runs the quick check by itself.

## Change expectations

- Keep the source of truth in the implementation and `language/spec.md`; do not duplicate API signatures by hand when a generated report can provide them.
- Add or update focused tests for behavior changes and preserve source spans and stable diagnostic codes.
- Keep capability grants explicit. Network, clock, file access, and optional Python use should not silently become broader.
- Preserve historical Veritas reports byte-for-byte. Add new Guion observations as a separate cohort.
- Update user docs, compatibility notes, changelog, and machine-readable reports when a public behavior or identity changes.
- Do not add or edit a project-level `AGENTS.md` without the repository owner's specific approval.

## Reporting a change

Describe what changed, the user-visible effect, commands run, and any target or evidence that remains unverified. Keep the report factual and scoped to the observed result.
