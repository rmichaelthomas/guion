# Veritas checkpoint v1.8: portable build policy

## Goal

Make a built program carry the resource ceilings under which it was produced, so copying the artifact does not silently change those ceilings. Keep the previous artifact format readable and make the recorded policy visible to people inspecting a build.

## Delivered

- Artifact format 15 adds optional build metadata: Veritas compiler version, build host (`OS-architecture`), and the effective numeric runtime ceilings from the project manifest/default policy.
- `veritas build` records those ceilings in the `.vrb`. `veritas inspect` accepts source and artifact paths and prints recorded metadata when an artifact contains it.
- Runtime execution combines artifact ceilings with the host's `HostPolicy` field by field, taking the lower value. Hosts can impose tighter limits; they cannot relax the build's recorded ceilings.
- Format-14 artifacts remain readable. They have no embedded policy and continue to use the current host/project policy.
- An end-to-end CLI regression test builds a project with a 1,024-byte allocation budget, copies only its artifact to a separate directory, confirms `inspect` reports the budget, and confirms execution fails with `R_ALLOCATION_LIMIT`.
- Model round-trip coverage exercises format-15 metadata and a format-14 artifact without metadata.

## Boundaries and open work

The recorded `target` currently describes the build host; it is not a selectable native compilation target. Metadata does not yet include source digests, compiler compatibility ranges, schema versions beyond the artifact format, dependency lock identities, or native toolchain identity. A copied artifact's Python imports still need an explicitly configured/available Python environment and dependency environment. Allocation accounting is still an estimate, not an exact RSS ceiling; this metadata makes the declared estimate stable, not exact.

The immediate next work is to extend provenance metadata with stable source and dependency identities, define compatibility rules across compiler/schema versions, and demonstrate stronger host interoperability with a representative native library. In parallel, continue exact resource accounting and broader project evaluation. Portable Python-assisted packaging, HTTP, additional ordinary-language surfaces, and independent recommendation evidence remain open per [implementation-plan.md](implementation-plan.md).

## Verification status

The end-to-end format-15 move-and-run CLI test passed after its intended pre-implementation failure was observed. The model test round-tripped both format-15 metadata and format 14 without metadata. The offline workspace suite passed all **75 Rust tests**, including managed CPython integration with the pinned uv toolchain. `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --offline -- -D warnings`, and `python3 -m unittest discover -s evaluation/tests -v` all passed (2 Python harness tests).

The fresh current-host package was extracted under `/private/tmp/rc25-check/`. Its `doctor`, matching, records, and modules examples passed. A project with a 1,024-byte allocation ceiling was built as format 15; after copying only the artifact and deleting the project, inspection showed `allocation_limit_bytes: 1024`, and running it returned `R_ALLOCATION_LIMIT` as expected. The source archive was extracted to a new temporary directory and its complete offline workspace suite passed all **75 Rust tests**, including managed CPython integration. The runnable and source archives each have adjacent SHA-256 sidecars.

## Resume

Continue the approved integrated build in `work/veritas-foundation/` from this checkpoint, [implementation-plan.md](implementation-plan.md), and `.superpowers/sdd/implementation-plan/progress.md`. Preserve the source repositories and uploaded starter materials. Do not describe the candidate as a general recommendation yet; the language/tooling comparison and independent workload suite remain incomplete.
