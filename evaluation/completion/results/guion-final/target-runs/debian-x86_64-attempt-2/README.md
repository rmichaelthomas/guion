# Debian 12 x86_64 verification — attempt 2

Source revision: `8b57cabeb4325271cb1eb3f8dfdd6480db17b390`

GitHub Actions run: [40338795973](https://github.com/rmichaelthomas/guion/actions/runs/40338795973)

Overall result: **failed; x86_64 is not validated**.

The complete Debian target evidence from this attempt is retained beside this file. The downloaded artifact's SHA-256 was verified against GitHub's published digest and is recorded in `workflow-run.json`.

The Ubuntu source-check job passed formatting and strict Clippy, then failed the workspace test suite in the nine-application acceptance check: the generated health-digest cases still requested four workers on a two-worker host. The Debian target matrix reported the same issue: 9 of 10 gates passed, while `product-applications` failed; 17 of 18 application reports passed, with only `health-digest-generated.json` failing.

That generated workload was corrected in the following revision by setting its worker request and in-flight bound to two. The correction passed the complete local nine-application acceptance test. This historical attempt remains failed and does not validate the target.

| Gate | Result |
| --- | --- |
| Conformance inventory | Passed |
| Bootstrap | Passed |
| Compiler conformance | Passed |
| Standard library data | Passed |
| Standard library services | Passed |
| Runtime | Passed |
| Product dependency audit | Passed |
| Product lifecycle | Passed |
| Product applications | Failed — generated health-digest requested 4 workers on a host limit of 2 |
| Release | Passed |
