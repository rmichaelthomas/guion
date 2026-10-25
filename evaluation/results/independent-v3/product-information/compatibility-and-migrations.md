# Compatibility and migration policy

This is the compatibility policy for the Veritas 0.1.0 evaluation build. Veritas is pre-1.0: source syntax, public APIs, bytecode details and package metadata may change before 1.0. This build does not promise long-term upgrade compatibility.

## Supported combinations

| Boundary | Supported in this candidate | What happens outside it |
| --- | --- | --- |
| Source | Compile with the matching Veritas 0.1.0 CLI and its bundled documentation. | A newer compiler may require source edits; no source-level compatibility range is promised before 1.0. |
| Bytecode artifact | The reader accepts artifact formats 14 through 33; the current compiler writes format 33. | A future format is rejected by older readers. Rebuild from source with a matching compiler when possible. |
| Package metadata | The verifier accepts package schemas 1 and 2; new packages use schema 2. | Unknown schema versions fail closed. |
| Application bundle | The format-33 campaign candidate is Debian 12-compatible GNU/Linux ARM64 with glibc. Same-target relocation is supported. | The repository has an earlier format-30 macOS ARM64 package; refresh and revalidate it before claiming format-33 parity. Other Linux distributions, libc implementations, operating systems and CPU architectures are unverified. |
| Python bridge | The exact Python patch version declared in `veritas.toml`; the release pins uv 0.13.4. | A different Python patch version or host target does not satisfy the lock. Use the explicit update workflow after reviewing dependency changes. |

Artifact format compatibility does not imply that the original compiler version is checked at runtime. Build metadata records the compiler version and other provenance for inspection. Formats 14 and 15 have less metadata than current artifacts; format 14 has no embedded runtime policy. The loader validates each feature against the minimum format that introduced it.

Package hashes and lock fingerprints detect accidental changes. Release archives and package metadata are unsigned, so they do not authenticate their publisher. Python adapter declarations are typed calls, not general Python source compatibility.

## Updating a Python environment

`veritas sync PROJECT` checks the existing lock and installs that exact dependency graph. `veritas sync --update PROJECT` stages a new graph and managed environment, validates the interpreter, then activates the new environment and writes matching lock metadata. If resolution, installation or interpreter validation fails, the previous bridge environment and lock files remain available. Review `veritas.lock` and `.veritas/python-bridge/uv.lock` after a successful update before rebuilding an application package.

## Moving to another Veritas candidate

1. Keep the project source, manifest, both Python lock files, and application data together. Make a backup before changing candidate versions.
2. Use the new candidate's `doctor` and `check` commands, then run the project's test functions.
3. If the existing locks pass, run without `--update`; change dependency resolution only with the explicit update command and review both lock files.
4. Rebuild artifacts and app packages with the new CLI. Verify the package and test it after moving it to a fresh path on the same supported OS and architecture.
5. Keep the previous candidate and data backup until the new build passes the project's acceptance checks.

Veritas does not provide source rewrites or artifact-format upgrades beyond reading the supported range. There is no automatic application-data migration in this release; application data schemas and their upgrades remain the application's responsibility. If a future runtime rejects an artifact format, retain its source and rebuild with a compatible compiler; if the source no longer compiles, follow migration notes for that specific release rather than editing the artifact bytes.
