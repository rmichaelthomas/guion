# Veritas checkpoint v1.43 — compatibility and migration policy

Status: documented, linked from the command reference, shipped in the release, and checked by the installed documentation acceptance test.

The compatibility guide states the supported artifact reader range (formats 14–22), current writer format (22), accepted package schemas (1 and 2), the macOS arm64-only app target, exact Python patch requirements, and pinned uv version 0.13.4. It distinguishes recorded compiler provenance from runtime compatibility enforcement and calls out the lack of pre-1.0 source/API stability.

It also gives builders an upgrade procedure: back up project and app data, check and test with the new candidate, preserve locked dependencies unless explicitly updating, review both lock files after updates, rebuild packages, verify them, and keep the previous release until acceptance passes. Application data migration remains application-owned. Unknown artifact or package formats fail closed.

The installed docs acceptance test checks the compatibility guide is present and names the actual artifact range, host, Python patch, and application-data migration boundary.

## Boundaries

This is a clear policy for the current release candidate, not a promise of a future support duration. Package hashes are unsigned, only macOS arm64 is tested here, and no automatic source or application-data migration is provided.

## Remaining work

- Run the preregistered independent recommendation evaluation across the three task classes.
- Add another supported host only after its compiler/runtime, native Python packages, installer, and relocation path pass the same acceptance criteria.
- Revisit source/API stability before a 1.0 release.
