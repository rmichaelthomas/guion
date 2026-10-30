# Compatibility and migration

Guion is pre-1.0. Source syntax, public APIs, bytecode, and package metadata may change before 1.0; there is no long-term compatibility promise yet.

## Current Guion names

| Purpose | Current name |
| --- | --- |
| Source | `.gu` |
| Compiled artifact | `.gub` |
| Project manifest | `guion.toml` |
| Project lock | `guion.lock` |
| Local project state | `.guion/` |
| Core command | `guion` |

New projects, generated files, packages, locks, and local state use these names.

## Reading projects from Veritas

The rename is a pre-1.0 transition. The current CLI retains read compatibility for selected old project inputs:

- `veritas.toml` and `veritas.lock` can be read when a project has not yet adopted the Guion filenames.
- Existing `.vr` source files are accepted as legacy project inputs.
- Prior `.veritas/python-bridge` state can be located for the optional Python bridge.
- Existing compiled artifacts that use the `VRTA` artifact magic remain readable, including prior files named `.vrb`.

This compatibility is a migration aid, not a guarantee that old names will remain supported through 1.0. New projects and new outputs use Guion names. A Guion project may continue to contain a legacy-named manifest while it is being migrated; avoid creating both old and new manifest or lock files in the same project, because that makes project identity ambiguous.

## Supported artifact and package boundaries

The current compiler writes artifact format 48 and the runtime reads artifact formats 14 through 48. Format 48 adds direct UTC date formatting from an `Instant`; formats 47, 46, and 45 added JSON decoding from HTTP bytes, Unicode text slicing, and checked UTF-8 decoding for byte HTTP responses. An artifact cannot contain an operation introduced after its format version, and existing compatible artifacts remain readable. Current packages use schema 2; the verifier accepts schemas 1 and 2. Artifact format compatibility does not promise that the source compiler version is checked at runtime. Hashes detect accidental changes but do not authenticate the publisher.

The Guion-identity pre-release matrix passed on macOS ARM64 and Debian 12 GNU/Linux on ARM64 and x86_64 with glibc, including same-target relocation. Other operating systems, CPU architectures, Linux distributions beyond Debian 12, and libc implementations beyond glibc remain unverified. Consult [status](status.md) and the [pre-release readiness report](pre-release-readiness.md) before relying on a target.

## Python interoperability

The default core build does not discover, launch, or bundle Python or `uv`. The optional bridge requires a build with `--features python-interop` and uses the exact interpreter and dependencies declared by the project. New Guion configuration uses `guion.toml`; `GUION_PYTHON` is the explicit interpreter override when no managed interpreter is configured. Old Veritas project configuration is read for transition compatibility.

For an existing project, first keep a backup of its source, manifest, lock files, and application data. Run `guion doctor`, then `guion check` and the project's tests. Preserve its existing locks unless you intentionally change dependencies. A lock migration or dependency update must be explicit and reviewed before packaging. Application data migration remains the application's responsibility.

## Package and installation behavior

`guion package PROJECT --output DIRECTORY` builds a current-host application bundle. `guion verify-package DIRECTORY` checks its recorded hashes. `guion install DIRECTORY [--prefix PATH]` installs a verified bundle under the chosen prefix, defaulting to `~/.local`; it does not edit shell startup files. The current release candidate has not been publicly published, so these commands are development interfaces, not a public installation channel.
