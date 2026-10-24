# Veritas checkpoint v1.9: source and dependency provenance

## Goal

Make artifacts identify the exact source graph and locked Python inputs that produced them, version that metadata independently, and preserve the earlier artifact formats.

## Delivered

- Artifact format 16 introduces build metadata schema 1. It records compiler version, build host, the source graph SHA-256, an optional combined dependency-lock SHA-256, and the effective numeric runtime limits.
- The compiler hashes the exact UTF-8 source buffers it parsed, keyed by sorted project-relative module paths and length-delimited path/content bytes. The CLI does not reread source files after compilation to derive this identity.
- When both `veritas.lock` and `.veritas/python-bridge/uv.lock` exist, the build records a combined SHA-256 over their exact bytes. An incomplete pair fails the build; no lock files is reported as absent.
- `veritas inspect` shows the metadata schema, source digest, dependency lock digest/absence, host, compiler, and runtime ceilings.
- Runtime continues to accept format-14 and format-15 artifacts. Format-15 metadata decodes as legacy schema 0 without source/dependency digests; format 14 has no build metadata.
- Artifact encoders reject unsupported format versions and unknown metadata schema versions rather than emitting data the current reader cannot understand.

## Verification evidence

- The source-digest acceptance test failed before implementation because `inspect` had no source digest. It now verifies the digest is a SHA-256 hex string, repeated builds from identical source and policy produce identical artifact bytes on the same host, and changing an imported module changes the digest.
- A lock-identity acceptance test confirms changing `uv.lock` changes the recorded combined lock digest while the source graph digest remains unchanged. An incomplete lock pair fails the build.
- Model coverage round-trips format 16 metadata and legacy formats 15 and 14, and checks unsupported format/schema rejection.
- The artifact-policy test still proves that copying only a built artifact preserves its allocation ceiling, which the runtime enforces.
- The offline workspace suite passed all **76 Rust tests**, including managed CPython integration; formatting, warning-free Clippy, and both Python evaluation-harness tests passed. The known sales-report case still matches its Python standard-library reference (`12\n1524\n`) and remains explicitly classified as development evidence, not an independent evaluation.
- Fresh RC26 package `doctor`, matching, records, and modules runs passed. A format-16 artifact with a 1,024-byte allocation ceiling was copied to a new directory after deleting its source project; inspection showed its schema, source digest, absent dependency-lock state, and limit, and execution returned `R_ALLOCATION_LIMIT`.
- The final source archive is verified separately; its extracted workspace test result and archive checksum are recorded in the accompanying RC26 milestone report.

## Boundaries and open work

These hashes identify inputs; they do not authenticate a publisher or prevent someone from replacing both an artifact and its metadata. The metadata does not yet record Veritas package dependency identities, complete compiler compatibility ranges, native ABI/library requirements, or target selection. Lock digests do not bundle the Python runtime or wheels. Allocation accounting remains estimated rather than an OS memory boundary.

The next approved-plan work is the remaining host operations, package/dependency lifecycle and representative independent workload evaluation. Native FFI requires an explicit design contract for types, ownership, capability reporting, and platform support before implementation; the current contract treats external Python calls as the supported bridge.

## Resume

Continue the approved integrated build in `work/veritas-foundation/` from this checkpoint, `docs/implementation-plan.md`, and `.superpowers/sdd/implementation-plan/progress.md`. Preserve all original language repositories, NowLookHere, and starter uploads. Do not claim general Python replacement or recommendation advantage without independent multi-class evidence.
