# Debian 12 x86_64 verification — attempt 4

Source revision: `03e78deb7d0e9d4f8b559005c3d4be75d326ab38`

GitHub Actions run: [40343686290](https://github.com/rmichaelthomas/guion/actions/runs/40343686290)

Result: **the matrix job failed during final evidence assembly; x86_64 is not yet validated by this attempt.**

All ten individual target gates passed, including the release gate, and all 18 application reports passed. The wrapper then tried to obtain the source revision with `git rev-parse HEAD`; Git metadata was unavailable in the Debian job container, so it exited before writing its final environment and summary files. The source revision is independently recorded by GitHub Actions in `workflow-run.json`.

The artifact was downloaded from the run and its SHA-256 was verified against GitHub's published digest. The gate reports, application reports, and logs are retained here. The next attempt supplies the workflow revision to the report runner and evidence assembler, allowing the complete matrix to finish and bind reports to the exact source revision.

| Gate | Result | Exit |
| --- | --- | ---: |
| Conformance inventory | Passed | 0 |
| Bootstrap | Passed | 0 |
| Compiler conformance | Passed | 0 |
| Standard library data | Passed | 0 |
| Standard library services | Passed | 0 |
| Runtime | Passed | 0 |
| Product dependencies | Passed | 0 |
| Product lifecycle | Passed | 0 |
| Product applications | Passed — 18/18 reports | 0 |
| Release | Passed | 0 |
| Final evidence assembly | Failed — Git metadata unavailable in container | 1 |
