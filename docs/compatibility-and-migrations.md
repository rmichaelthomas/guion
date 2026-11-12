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

Runtime limits changed in the same development series without a format change: the `guion` command line now has no default instruction limit and treats `allocation_limit_bytes` as a bound on memory in use. Projects that relied on the old 2,000,000-instruction default to stop runaway programs should set `[runtime] instruction_limit` explicitly. Artifacts built earlier recorded a 2,000,000-instruction limit in their metadata and keep it when run, because a host cannot loosen an artifact's limit; rebuild them to drop it. The current compiler writes artifact format 62 and the runtime reads artifact formats 14 through 62. Format 62 adds the `Array[e]` type (type tag 18), the array instruction family (op tag 193) and in-place local array updates (op tag 194); an artifact that uses arrays cannot be written at an earlier format, and artifacts without arrays are otherwise unchanged. Format 61 adds the 64-bit random instructions that let `std:random` run on the machine-code path (with unchanged sequences) and mutual-TLS client and server handshakes in `std:net`. Format 60 records the packages a build links (name, exact version and content hash) in its build metadata; artifacts built without packages are unchanged. Format 59 adds SHA-1 and byte XOR for the WebSocket support in `std:http_server` and `std:websocket`; format 58 adds TLS client and server handshakes in `std:net`; format 57 adds TCP and UDP socket operations behind `std:net`; format 56 adds subprocess and signal operations behind `std:process`; format 55 adds the regular-expression operations behind `std:regex`; format 54 adds the filesystem operations behind `std:fs` and `std:path`; format 53 adds byte inspection, base64, hex, UTF-8 and percent-encoding functions behind `std:encoding` and `std:url`; format 52 adds Float64 math, numeric conversions, and the SplitMix64 mixing step behind `std:math` and `std:random`; format 48 added direct UTC date formatting from an `Instant`; formats 47, 46, and 45 added JSON decoding from HTTP bytes, Unicode text slicing, and checked UTF-8 decoding for byte HTTP responses. An artifact cannot contain an operation introduced after its format version, and existing compatible artifacts remain readable. Current packages use schema 2; the verifier accepts schemas 1 and 2. Artifact format compatibility does not promise that the source compiler version is checked at runtime. Hashes detect accidental changes but do not authenticate the publisher.

Package and lock formats are unchanged: `guion.lock` stays at format 2, and a library's `[capabilities]` declaration is recorded as an extra table in its lock entry, which earlier readers ignore. Existing package names and exact-version locks keep loading; a name that breaks the new rules (uppercase, over 64 characters, reserved) produces a warning for an existing lock and is refused for a new `add`, `new --lib` or `publish`. Version ranges apply only to registry packages; manifests that pin an exact `path` or `url` dependency are read as before. No migration command is needed for this series.

The Guion-identity pre-release matrix passed on macOS ARM64 and Debian 12 GNU/Linux on ARM64 and x86_64 with glibc, including same-target relocation. Other operating systems, CPU architectures, Linux distributions beyond Debian 12, and libc implementations beyond glibc remain unverified. Consult [status](status.md) and the [pre-release readiness report](pre-release-readiness.md) before relying on a target.

## Python interoperability

The default core build does not discover, launch, or bundle Python or `uv`. The optional bridge requires a build with `--features python-interop` and uses the exact interpreter and dependencies declared by the project. New Guion configuration uses `guion.toml`; `GUION_PYTHON` is the explicit interpreter override when no managed interpreter is configured. Old Veritas project configuration is read for transition compatibility.

For an existing project, first keep a backup of its source, manifest, lock files, and application data. Run `guion doctor`, then `guion check` and the project's tests. Preserve its existing locks unless you intentionally change dependencies. A lock migration or dependency update must be explicit and reviewed before packaging. Application data migration remains the application's responsibility.

## Package and installation behavior

`guion package PROJECT --output DIRECTORY` builds a current-host application bundle. `guion verify-package DIRECTORY` checks its recorded hashes. `guion install DIRECTORY [--prefix PATH]` installs a verified bundle under the chosen prefix, defaulting to `~/.local`; it does not edit shell startup files. The current release candidate has not been publicly published, so these commands are development interfaces, not a public installation channel.
