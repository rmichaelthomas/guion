# Debian 12 x86_64 verification — attempt 1

Source revision: `2d0c82a8d350653a9419580756c1edff9a95b4bd`

GitHub Actions run: [40328954686](https://github.com/rmichaelthomas/guion/actions/runs/40328954686)

Overall result: **failed; x86_64 is not validated**.

The complete workflow evidence from this attempt is retained beside this file, including gate reports, logs, and all 18 application reports. `workflow-run.json` records the GitHub artifact ID and verifies its SHA-256 digest.

| Gate | Result |
| --- | --- |
| Conformance inventory | Passed |
| Bootstrap | Failed — dependency audit rejected the in-process `guion-compiler` crate |
| Compiler conformance | Failed — one test requested 4 workers on a host limit of 2 |
| Standard library data | Passed |
| Standard library services | Passed |
| Runtime | Failed — three tests requested 4 workers on a host limit of 2 |
| Product dependency audit | Failed — same false rejection of the in-process compiler crate |
| Product lifecycle | Passed |
| Product applications | Failed — calendar-conflicts and health-digest follow-up cases requested 4 workers on a host limit of 2; two generated health-digest cases also used the 4-worker default |
| Release | Passed |

The parallelism failures reflect hard-coded test and example worker counts that exceeded the two-core host policy. The dependency audit failures reflect an incorrect check: `guion-compiler` is the compiler embedded in the CLI and is expected in its Rust dependency graph. This attempt did not uncover a Rust target build failure. The failing gates must pass on a later run before x86_64 can be listed as validated.
