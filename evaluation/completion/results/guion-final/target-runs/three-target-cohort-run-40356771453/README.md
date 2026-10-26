# Three-target matrix run 40356771453

Source revision: `51aeb93a93724b4aa81171250837cca04efcb383`.

The macOS ARM64 target matrix passed all ten gates and all 18 application reports. The Debian ARM64 job failed during GitHub Actions setup before any gate ran: the workflow referenced the nonexistent `actions/download-artifact@v7.0.2` tag. The x86_64 job was skipped because it depends on the Debian ARM64 job. The independent Ubuntu source-check job was canceled after this setup failure, so this run does not establish a complete three-target result.

The exact Debian ARM64 setup log is retained beside this note. The next run corrects the action pin to the official `v8.0.2` release. No release-target documentation was promoted from this incomplete run.

GitHub Actions run: [40356771453](https://github.com/rmichaelthomas/guion/actions/runs/40356771453)
