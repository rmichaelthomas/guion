# Debian 12 x86_64 verification — attempt 3 (partial)

Source revision: `0d49c6eac862c7d29086e1cdc87b84224743b7f8`

GitHub Actions run: [40340954241](https://github.com/rmichaelthomas/guion/actions/runs/40340954241)

Result: **canceled before the Debian matrix completed; x86_64 is not validated by this attempt**.

The Ubuntu source-check job passed formatting, strict Clippy, the application acceptance test, and most workspace tests. It failed `parallel_map_reuses_one_native_backend_across_workers`: the test requested four workers under the two-worker host policy, so the CLI returned exit code 1 instead of 0.

The Debian target matrix completed the first three gates before it was canceled as superseded by the next revision. Conformance inventory, bootstrap, and compiler conformance passed. Later gates did not run in this attempt. The complete partial artifact is retained beside this file; its SHA-256 was verified against GitHub's published digest and is recorded in `workflow-run.json`.

The CLI test now requests two workers while processing four tasks. The focused regression test passed locally. This partial attempt does not validate x86_64.

| Gate | Result |
| --- | --- |
| Conformance inventory | Passed |
| Bootstrap | Passed |
| Compiler conformance | Passed |
| Remaining target gates | Not run — superseded and canceled |
| Ubuntu workspace tests | Failed — four-worker CLI test exceeded host limit of two |
