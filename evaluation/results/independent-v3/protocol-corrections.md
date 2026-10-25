# Campaign protocol corrections

This file records changes made after the campaign freeze. Original freeze and run reports remain immutable and are not silently reclassified.

## Correction 1: executable temporary workspace and project entry resolution

- Date: 2026-10-24.
- Trigger: the first scored Veritas runs showed that relocated launchers copied beneath `/tmp` failed with `Permission denied`. The container explicitly mounted `/tmp` with `noexec`, while the case runner created its fresh candidate workspace there and launched the copied executable from it. Both Veritas candidates also declared valid `veritas.toml` entry paths under `src/`, while the runner ignored the manifest and hardcoded `${candidate}/main.vr`.
- Change: the runner now keeps `/tmp` `noexec` at 128 MiB, mounts a separate explicitly executable `/candidate-tmp` at 768 MiB, and sets `TMPDIR=/candidate-tmp`; the combined tmpfs allowance is 896 MiB under the existing 2 GiB container memory limit. It mounts the reviewed host candidate configuration into both source and delivery runs. The Veritas source command runs the candidate project directory, allowing the CLI to resolve the declared manifest entry.
- Previous reviewed runner SHA-256: `6b82a3bc74bb037044c3f09fe686ff7c639eeb748b4aa57ccd37416b70a5b3ea`.
- Corrected runner SHA-256: `3923325e6d9283abf2544f569157e62ad3393280fb5cd8f5c06b858486eaea8a`.
- Previous reviewed Veritas candidate-config SHA-256: `8fafb67c2018d35b3fe67938be1336bdeeb66b599618c75f0c483466d1c15b80`.
- Corrected Veritas candidate-config SHA-256: `302a86d18d32f732c39e2d5299554daf6fc2e7d4e8b325e3f1d1797e9e627205`.
- Scope: the frozen task manifest, candidate product bundle, randomized schedule, and language-choice evidence are unchanged. The original reviewer approval applies to the prior hashes only. All subsequent run reports must name this correction and use new report paths; original reports are retained as diagnostics. Comparisons across this point will disclose the protocol correction.
- Verification: shell syntax and JSON parsing pass. The pair-2 Veritas follow-up rerun under this runner passes both source cases; delivery is no longer blocked by `noexec`, but fails because the frozen Veritas binary cannot grant the caller's working directory as a file root. See Correction 2.

## Correction 2: package CLI data roots

- Date: 2026-10-24.
- Trigger: the corrected pair-2 source cases passed, but its relocated package could not read the fixture-provided `policy.json` from the caller's working directory. The frozen package runtime grants file access only beneath the installed bundle; the CLI had no way to keep bundle resources accessible while also granting caller data.
- Change: Veritas `run` now accepts repeatable `--file-root DIRECTORY` options. Each is an additional bounded grant; reads search the project/artifact root first, then the additional roots, while writes go to the last granted root. The package launcher adds the invocation directory by default and accepts `VERITAS_FILE_ROOT` to select another additional root. No path traversal or absolute guest paths are allowed.
- Evidence: `packaged_cli_can_read_inputs_from_its_invocation_directory` was observed failing before the implementation and now passes while also checking an external output write. `package_builds_a_relocatable_app_with_declared_resources` passes, preserving package-resource reads. The six package-focused CLI tests pass. Runtime suites pass 9 unit, 94 execution, 9 HTTP, and 20 Python-bridge tests.
- Campaign consequence: the frozen v3 candidate image still contains the old runtime. Pair-2's corrected-runner report remains valid evidence about that frozen binary, not about the updated source tree. Do not score subsequent Veritas builds against the updated product until a Linux ARM64 bundle containing this change is built and its new package/image hashes are recorded. The existing independent review did not review this product change.
