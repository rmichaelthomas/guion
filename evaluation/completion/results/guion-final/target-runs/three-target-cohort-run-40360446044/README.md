# Three-target release validation — run 40360446044

GitHub Actions [run 40360446044](https://github.com/rmichaelthomas/guion/actions/runs/40360446044) completed successfully against source revision `0fecfbf1694474b5e85bebb8bc4f87db29275f53`. The working-tree digest was empty. All three release targets passed the ten release gates and all 18 application reports for each target passed, for 54 of 54 reports overall.

| Target | Host | Result | Application reports |
|---|---|---|---:|
| `macos-aarch64` | macOS 15.7.9, arm64 | PASS | 18/18 |
| `debian-aarch64` | Debian 12.15, glibc 2.36 | PASS | 18/18 |
| `debian-x86_64` | Debian 12.15, glibc 2.36 | PASS | 18/18 |

Each target directory retains its environment record, gate table, logs, machine-readable reports, application results, and target-specific reports. The x86_64 directory also contains the final aggregate reports with all three targets completed and no skipped targets. Aggregate JSON reports are copied to the parent `guion-final/` directory; run-stamped target JSON files are copied to its `target-runs/` directory so earlier evidence remains intact.

The release archives themselves are not retained in this evidence bundle. Their sizes, SHA-256 hashes, package audits, and installation results are recorded in the aggregate and per-target `release.json` files.
