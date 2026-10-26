# Local package artifacts

`artifacts/local/` is inside the canonical Guion repository directory and is intentionally excluded from Git. It holds the two Guion pre-release validation archives, the two final historical Veritas archives, and the small historical prototype checkout. The archives are local package evidence, not a public release. Their identities are recorded below and in the checked-in release reports.

| Package | Bytes | SHA-256 |
|---|---:|---|
| `local/guion-prerelease-0.1.0-2026-10-25/guion-macos-aarch64.tar.gz` | 5,429,536 | `bf24981b70f9c96833b7c5a1e8452b78acb6a4ec0ca11fb465d4df541141ec9c` |
| `local/guion-prerelease-0.1.0-2026-10-25/guion-debian-aarch64.tar.gz` | 5,880,206 | `67de0ee99b613891d1223965680c6445df849cb24ab3c86ba87b34ff92def7ad` |
| `local/veritas-legacy-0.1.0/veritas-macos-aarch64.tar.gz` | 5,414,452 | `286da47c8cdbbb8eaf29bb0710ad60e130e58dfd0df852136c48e471a3e5db37` |
| `local/veritas-legacy-0.1.0/veritas-debian-aarch64.tar.gz` | 5,869,757 | `75c47c105ec240f7635658b768f35888e3b98af8796658f2795d6f3ec2571f1f` |

A clone of the private GitHub repo contains the source, docs, hashes, and compact validation reports, but not these four archive bytes or the local prototype checkout. When a release is approved, publish the Guion host archives through the release channel rather than adding generated packages to Git history.
