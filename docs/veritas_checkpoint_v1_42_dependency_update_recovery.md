# Veritas checkpoint v1.42 — dependency update recovery

Status: implemented; update failure and success paths pass focused integration tests.

`veritas sync --update` now builds the proposed Python lock and environment in a separate staging directory. It checks that resolution succeeds, dependency installation succeeds, and the staged interpreter matches the declared exact Python version before switching the bridge environment and writing the matching Veritas lock metadata. A resolver, installer, or interpreter validation failure removes the stage and leaves the current bridge environment and lock files intact.

The final switch renames the existing bridge directory to a same-filesystem backup, activates the staged directory, and atomically replaces `veritas.lock`. If lock metadata cannot be recorded, the code removes the candidate environment and restores the prior bridge directory; rollback failures include the recovery path in the diagnostic. Existing non-update `sync` and locked run paths retain their lock-check behavior.

Two CLI integration tests use a controlled uv executable. One simulates a lock change followed by an installation failure and checks that both lock files and a sentinel in the prior environment remain unchanged. The other creates a staged managed interpreter, confirms the update report contains lock fingerprints, and successfully runs the project after the update.

## Limits

The transaction protects against ordinary command and filesystem errors. A process or machine crash between the directory rename and lock-file replacement can leave the lock pair inconsistent; locked execution detects the mismatch and refuses to run. The current cross-host target matrix is not complete. Package and lock hashes detect accidental changes but remain unsigned.

## Remaining work

- Add an explicit Veritas compatibility range and migration notes for generated projects and portable artifacts.
- Validate dependency update behavior on additional target operating systems.
- Complete the independent assistant recommendation evaluation.
