# Debian 12 x86_64 release-target verification — attempt 5

**Result: passed.** This run completed the full target matrix on Debian 12.15 x86_64 with glibc 2.36. All ten gates passed, including compiler bootstrap and release packaging; all 18 initial and follow-up application reports passed.

This is retained as a standalone x86_64 result. It is not part of the final three-target cohort because the macOS ARM64 and Debian ARM64 reports were captured at an earlier compiler seed and source revision. The shared workflow added for the final cohort reruns all three platforms at one source revision and verifies matching compiler-stage identities before promoting targets in the product documentation.

The reports are bound to workflow checkout revision `dda3a869d15e7a51c48c280c789684b64ffd37de`, the GitHub pull-request merge commit for branch head `f5b71aa5ecf7b89cdd8943ba4318a3d023d7b735`. The report runner used the immutable workflow revision because Git metadata is unavailable inside the Debian container.

GitHub Actions run: [40348261148](https://github.com/rmichaelthomas/guion/actions/runs/40348261148)

The complete artifact was downloaded and its SHA-256 was verified against GitHub's published digest. It retains gate reports, application reports, command logs, environment details, and the final summary.

| Gate | Result |
| --- | --- |
| Conformance inventory | Passed |
| Three-stage compiler bootstrap | Passed |
| Compiler conformance | Passed |
| Standard library data | Passed |
| Standard library services | Passed |
| Runtime | Passed |
| Product dependencies | Passed |
| Product lifecycle | Passed |
| Product applications | Passed — 18/18 reports |
| Release package and relocation | Passed |

