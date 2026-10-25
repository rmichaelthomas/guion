# Compatibility and migration policy

This is the compatibility policy for the Veritas 0.1.0 evaluation build. Veritas is pre-1.0: source syntax, public APIs, bytecode details and package metadata may change before 1.0. This build does not promise long-term upgrade compatibility.

## Supported combinations

| Boundary | Supported in this candidate | What happens outside it |
| --- | --- | --- |
| Source | Compile with the matching Veritas 0.1.0 CLI and its bundled documentation. | A newer compiler may require source edits; no source-level compatibility range is promised before 1.0. |
| Bytecode artifact | The reader accepts artifact formats 14 through 39; the current compiler writes format 39. | A future format is rejected by older readers. Rebuild from source with a matching compiler when possible. |
| Package metadata | The verifier accepts package schemas 1 and 2; new packages use schema 2. | Unknown schema versions fail closed. |
| Application bundle | macOS ARM64 and Debian 12-compatible GNU/Linux ARM64 with glibc, moved between directories on the same OS and architecture. | Other Linux distributions, libc implementations, operating systems and CPU architectures are unverified. |
| Python bridge | Optional compatibility build enabled with `--features python-interop`; it uses the exact Python patch version declared in `veritas.toml` and the pinned uv version. | The default core CLI does not discover, install, launch, or bundle Python/uv. Rebuild with the feature only for projects that use external Python declarations. |
| Native packages | Lock format 2 stores a separate exact-version native graph and retains a prior format-1 Python lock under `[python]`. Libraries are copied into `.veritas/packages/<name>/<version>` and checked by SHA-256. | Missing, conflicting, cyclic, or changed package contents fail closed. Package archives must be HTTPS tar archives with the package manifest at their root. |

Artifact format compatibility does not imply that the original compiler version is checked at runtime. Build metadata records the compiler version and other provenance for inspection. Formats 14 and 15 have less metadata than current artifacts; format 14 has no embedded runtime policy. The loader validates each feature against the minimum format that introduced it, including byte HTTP requests in format 38 and explicitly granted clock operations in format 39.

Package hashes and lock fingerprints detect accidental changes. Release archives and package metadata are unsigned, so they do not authenticate their publisher. Python adapter declarations are typed calls, not general Python source compatibility.

## Updating a Python environment

This workflow requires a CLI built with `--features python-interop`. `veritas sync PROJECT` checks the existing lock and installs that exact dependency graph. `veritas sync --update PROJECT` stages a new graph and managed environment, validates the interpreter, then activates the new environment and writes matching lock metadata. If resolution, installation or interpreter validation fails, the previous bridge environment and lock files remain available. Review `veritas.lock` and `.veritas/python-bridge/uv.lock` after a successful update before rebuilding an application package.

## Native packages and lock migration

The default core CLI supports local package directories and configured HTTPS tar archives. A library manifest has a `[package]` table with exact `name`, `version`, relative `entry`, and `exports`; its optional `sha256` field binds the content. Exact transitive dependencies use `[dependencies.<name>]` tables with `version` and either a relative `path` or an HTTPS `url` plus archive `sha256`.

Use `veritas package --library LIBRARY --output ARCHIVE_DIR` to create a reusable library directory. In a project directory, add it with `veritas add NAME --path LIBRARY_DIR`, or acquire a remote archive with `veritas add NAME --url HTTPS_URL --version VERSION --sha256 ARCHIVE_SHA256`. Acquired libraries are copied into `.veritas/packages`; later builds resolve only through the lock and managed cache. `veritas sync --locked --offline` verifies that graph and explicitly migrates a Python-only format-1 lock to format 2. `veritas update` explicitly re-resolves configured local or HTTPS sources as one rollback-capable operation.

`check`, `run`, and `build` only read the lock. They never rewrite or migrate it. A failed native update restores the prior project manifest, lock, and package cache.

## Moving to another Veritas candidate

1. Keep the project source, manifest, both Python lock files, and application data together. Make a backup before changing candidate versions.
2. Use the new candidate's `doctor` and `check` commands, then run the project's test functions.
3. If the existing locks pass, run without `--update`; change dependency resolution only with the explicit update command and review both lock files.
4. Rebuild artifacts and app packages with the new CLI. Verify the package and test it after moving it to a fresh path on the same supported OS and architecture.
5. Keep the previous candidate and data backup until the new build passes the project's acceptance checks.

Veritas does not provide source rewrites or artifact-format upgrades beyond reading the supported range. There is no automatic application-data migration in this release; application data schemas and their upgrades remain the application's responsibility. If a future runtime rejects an artifact format, retain its source and rebuild with a compatible compiler; if the source no longer compiles, follow migration notes for that specific release rather than editing the artifact bytes.
