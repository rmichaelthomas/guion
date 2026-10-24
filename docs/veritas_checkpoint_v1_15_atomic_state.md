# Veritas checkpoint v1.15 — first-run and atomic state replacement

## What changed

Artifact format 21 adds `write_text_atomic(path, content) -> Result[Unit, FileError]`. It creates a unique temporary sibling, writes and syncs the complete content, preserves an existing target's permissions, then renames the temporary file over the target. A reported failure cleans up the temporary file. Readers see the old complete file or the new complete file, rather than a partly written replacement.

This is atomic visibility on the packaged macOS target. It does not lock a read-modify-write sequence: concurrent writers can still overwrite each other's full updates. It does not sync the containing directory, so it makes no promise that the rename survives sudden power loss. A process crash before cleanup can leave a temporary sibling behind.

Missing files now return `R_FILE_NOT_FOUND`, and permission failures return `R_FILE_PERMISSION`. Other filesystem failures retain their operation codes. These classifications let source programs handle first-run state without treating permission or storage problems as an empty database.

## Proof program

The pure-Veritas `task_ledger` now creates its data file on the first mutation. It reads missing state as an empty array, reports other read failures, and saves every update through atomic replacement. Its project ignores its generated `tasks.json` file.

The acceptance suite checks first-run behavior with no seeded state file, Python parity for command output/status/state bytes, and a format-21 artifact after its source tree has been removed. The atomic-write runtime test replaces a target twice, verifies the final complete contents, verifies `R_FILE_NOT_FOUND`, and checks that no temporary sibling remains after success.

After this checkpoint was packaged as RC33, a temporary pinned CPython 3.14.8 plus wheel cache was provisioned. The combined feed-digest and task-ledger acceptance suites then passed all 10 cases against the exact RC33 executable from a fresh package extraction. This is same-host warm-cache evidence; the release archive still does not include CPython or wheels. The result is recorded in `evaluation/results/feed-digest-acceptance-rc33.json`.

## Limits and next work

Atomic replacement prevents torn files but does not solve simultaneous update conflicts. A lock or transaction API would need clear process and platform semantics before the app could safely support concurrent writers. Direct `write_text` and `write_bytes` remain non-atomic. The recommendation goal remains unproven; this checkpoint expands the implementation evidence for a sequential local data app only.
