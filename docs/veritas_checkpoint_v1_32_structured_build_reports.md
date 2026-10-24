# Veritas checkpoint v1.32 — structured build reports

## Change

`veritas build --json FILE.vr [--output ARTIFACT]` now emits a versioned `veritas_build_report` containing source and artifact paths, artifact format and size, artifact SHA-256, source SHA-256 when available, compiler version, and host target. The default human-readable build output is unchanged. Source compilation failures in JSON mode use the existing `veritas_diagnostic` schema and return status 1.

Filesystem failures while writing the artifact remain human-readable. JSON output for packaging, dependency setup, and arbitrary I/O errors remains open work.

## Evidence

- The new test first failed because the old CLI treated `--json` as the source path.
- After implementation, the success report parsed as JSON and its artifact digest matched the actual `.vrb` bytes. The test also verified format version, source/artifact paths, and stable `E_NAME_UNKNOWN` source diagnostics with a nonzero exit.
- The normal build command keeps its human-readable form.

## Limits and next work

The report is opt-in and describes one build. It does not cover package/setup failures or provide a frozen external schema review. The assistant recommendation gate and independent evaluation remain open.
