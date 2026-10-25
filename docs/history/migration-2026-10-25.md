# Veritas to Guion migration record

Recorded 2026-10-25. This note preserves the state and evidence trail for moving the completed Veritas work into the private `guion` repository. Historical reports retain their original product identity and values.

## Frozen source identity

- Source checkout: `/Users/rmichaelthomas/Documents/Codex/2026-10-22/referenced-chatgpt-conversation-this-is-an/work/veritas-foundation`
- Source branch and commit: `veritas/north-star-sprint`, `3ece1ce19a5d2aa8443491ab58c39873a45af1a9`
- Tracked files at freeze: 1,283; Git pack: 28,154,266 bytes; no configured remote.
- Four tracked working-tree edits were saved in `evaluation/protocol/case-runner.md`, `evaluation/results/independent-v2/README.md`, `evaluation/results/native-backend-probe-cranelift-0.135.0.json`, and `evaluation/review/independent-v1/artifact-hashes.json`. Their full patch and SHA-256 are recorded in the accompanying retention manifest.
- The destination `~/guion` was empty when inspected. The GitHub repository `rmichaelthomas/guion` was private and empty when inspected. This migration does not make it public or publish a release.

The complete file-level inventory, preserved hashes, deletion candidates, campaign disposition, and scan record are in [`migration-retention-manifest.json`](migration-retention-manifest.json). It is the detailed record; this page is the human-readable summary.

## Completion evidence retained

The ordinary repository keeps the 48 Veritas checkpoint documents, original product plan, sprint log, completion report, source, protocols and fixtures, frozen independent-v1/v2/v3 result documents, all retained compact completion JSON reports (including unfavorable results), builder reports and briefs, and selected final application source. The v2 and v3 record READMEs explain the removed bulk and remaining replay limits. A separate development experiment note records the exploratory installed-performance timings without presenting them as a general Python comparison.

The final Veritas completion report identifies product commit `cb5587b1f4d48a72b955ad2e6e666cdef28f5373`, source snapshot digest `sha256:4d515d48ba1912119df9cb800cc73d6c1a8ddf02ca25a66de4589b51c9258066`, artifact format 39, compiler seed, runtime and standard-library identities. It records supported targets as macOS ARM64 and Debian 12-compatible GNU/Linux ARM64. Preserve that report as evidence about the Veritas build; it is not proof that the renamed Guion build has passed the same checks.

## Material preserved outside the ordinary Git tree

Two pre-rename Veritas 0.1.0 archives were copied to the migration output folder and checked against their SHA-256 sidecar and archive listing. They are companion historical downloads, not Guion releases:

| Target | SHA-256 |
|---|---|
| Debian ARM64 | `75c47c105ec240f7635658b768f35888e3b98af8796658f2795d6f3ec2571f1f` |
| macOS ARM64 | `286da47c8cdbbb8eaf29bb0710ad60e130e58dfd0df852136c48e471a3e5db37` |

They were built from Veritas source commit `cb5587b1f4d48a72b955ad2e6e666cdef28f5373`, not the frozen checkout HEAD above. The release report and working-tree digest are the identity authority. The output folder is `outputs/veritas-legacy-0.1.0/` in the migration workspace; do not commit the archives into this repository.

## Cleanup inventory and replay consequences

The source checkout contained approximately 15.9 GB of ignored Rust `target/` output, about 950 MB of v2 workspace data plus a 1.17 GB compressed raw run-evidence archive, about 1.03 GB of v3 delivery data, nine application `dist/` trees totaling about 414 MB, about 149 MB of temporary `work/`, a 14 MB crash core, and a 23 MB expanded copy of the tracked libical source archive. Exact observed sizes, hashes, and retained replacements are in the JSON manifest. Deleting these generated or duplicate bytes does not delete the source, compact reports, or Git history.

Replay losses are explicit:

- Independent v2: removing the raw evidence archive and pinned workspace means the eight partial format-30 implementation sessions can no longer be replayed byte for byte. No official scored choices or runner results were recorded. The protocol, setup attempts, schedule, compact records and archived-evidence hash remain; a hash is not a backup.
- Independent v3: removing 13 per-run `.delivery/` folders removes the bulky delivered workspaces (about 926 MB). Fourteen compact run reports, their delivery manifests and hashes, campaign source, prompts, decisions and final assessments remain. Exact replay of those delivery directories is no longer possible.
- Completion applications: generated `dist/` copies and repeated runtime executables are reproducible from retained source/reports but their historical binary bytes are not recoverable from hashes alone.
- Expanded libical tree: it is generated from the tracked source archive. The 594-byte test fixture and 43-byte license text are retained separately so tests and release notices do not depend on an expanded checkout-time copy.

The current source branch is retained unchanged as a small historical checkout until the new local and private remote state pass final parity. No blanket process kill was used.

## Additional generated outputs in the cleanup inventory

The exact inventory includes all nine generated application `dist/` trees (seven files each), every top-level temporary `work/` group, the v2 workspace, the 13 v3 delivery directories, the crash core and the expanded libical tree. The source `work/veritas-v3-pair2-amended-candidate` source-only tree was compared with the retained independent-v3 source amendment; they match when generated distribution and Python environment files are excluded. Selected Linux config-migration and Python lock-smoke source plus their rerun scripts were copied into `evaluation/development/experiments/2026-10-25/`. Other generated package-refresh candidates and copied virtual environments are removed as superseded outputs.

The v3 runtime amendment archive (`veritas-format33-file-root-linux-aarch64.tar.gz`, 49,477,829 bytes, SHA-256 `f6cba12f4ccfcf7c38b3fcd6167b7ac6d137a89f3003eb564fe4dfb733d56635`) is not copied into the ordinary repo. Its source and package metadata remain; exact package bytes are not recoverable from the hash. Eleven tracked builder evidence runtime executables (141,139,488 bytes across two SHA-256 identities) are removed from the current tree after their exact paths, hashes and referring reports were recorded. Their Git history remains.

## Process inspection

Two idle project containers, `veritas-release-linux-20261025` and `veritas-task6-linux`, were confirmed by their mounts and command lines to belong to this checkout and were stopped. Both were running only `sleep infinity`; their exit code 137 followed the stop grace period and is not evidence of an out-of-memory failure. A shared Colima virtual machine remains running. Inspection found it held only a directory handle into the old release staging tree, with no open build-output file handles. It is shared infrastructure, not a Guion build process, and was left running.

## Security and privacy scan

`gitleaks 8.30.1` reported no findings in the prospective working tree (22.61 MB scanned) or Git history (9 commits, 8.53 MB scanned). A manual scan found historical absolute paths to the owner's local workspace in 111 files and 9 generated/example email-shaped strings in 5 files; these are retained as provenance in the private repository. No account credential was found. The push is scoped to the verified private `rmichaelthomas/guion` repository.

## Migration mapping

The original Veritas branch and tag remain historical. The Guion migration creates `main` from the frozen ancestry, preserves the final pre-rename state with an archival Veritas tag, and records later Guion rename and documentation commits in Git history. The exact resulting commit IDs, measured post-cleanup sizes, and final remote parity are appended below after those gates complete.

- Pre-rename source commit: `3ece1ce19a5d2aa8443491ab58c39873a45af1a9`
- Archival tag: pending
- Guion `main` migration commit: pending
- Pre-release candidate commit: pending
- Local/remote parity: pending

## Cleanup result

The ignored Rust `target/` tree (15,947,272,192 allocated bytes) is gone. The v2 workspace and raw archive are gone; all 13 v3 delivery workspaces and the v3 amendment package archive are gone; all nine generated completion `dist/` trees, the 14 MB core dump, temporary smoke inputs, Python bytecode caches, temporary `work/` tree, and expanded libical tree are also gone. Selected final sources, compact outcomes, hashes, logs, and small required license/test files remain as listed in the retention manifest.

`df -Pk` measured available space increasing from 23,960,000 KiB before the first removal to 35,880,000 KiB after cleanup: a net increase of 11,920,000 KiB (12,206,080,000 bytes; about 11.37 GiB). The `target/` removal alone increased the free-space counter by 9,040,000 KiB. Some smaller directory removals showed no separate `df` delta even though their paths are absent; see the per-group log in the JSON manifest.

The first v2 recursive deletion stopped on read-only package-license directories. The workspace was already copied and hash-verified. Only directories under that exact approved workspace were made owner-writable, after which the remaining 1.9 MB and raw archive were removed. This partial attempt and its measured free-space increase are included in the manifest.
