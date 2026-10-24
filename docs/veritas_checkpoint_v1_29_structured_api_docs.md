# Veritas checkpoint v1.29 — structured API documentation

## Change

`veritas doc --json FILE.vr` emits a versioned `veritas_api_report` suitable for an editor, assistant, or other tool. It reports declared Veritas functions with ordered parameter types and result types, record names and typed fields, and declared Python calls with their source module, callable, named parameters, result type, and opaque-effect marker. Existing `veritas doc FILE.vr` text output remains available.

Function parameter names are not preserved by the current compiled artifact, so the report identifies Veritas parameters by source order and type. Source compile failures still use the existing human-readable diagnostics.

## Evidence

- The integration test was first run against the old CLI and failed because `--json` was treated as the filename.
- After implementation, the report parsed as JSON and identified a typed `Point` record, a function using it, and a named-parameter `builtins:pow` declaration.
- The schema kind/version and empty stderr were checked.
- Human documentation output still prints its existing signature format.
- The installed RC49 CLI produced the structured report from shipped example sources, verified the bundled schema-2 Python package, and installed under a path containing spaces and Unicode. The bundled Python app ran twice offline with an empty inherited environment.
- Full verification: Rust workspace 114 passed, Python acceptance 20 passed, formatting and strict Clippy passed.

## Limits and next work

This is searchable structured documentation for one source module and its imported compilation graph as represented in the artifact. It does not provide source parameter names, prose docstrings, global package search, or build/setup error JSON. Continue toward shipped self-repair documentation and independent recommendation evidence.
