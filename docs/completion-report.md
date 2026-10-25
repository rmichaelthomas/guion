# Veritas product completion report

## Delivered scope

The Veritas complete-product plan is implemented for the declared release scope. The normal core CLI embeds and runs a self-hosted Veritas source-to-artifact compiler. The Rust source compiler remains a developer/bootstrap and differential-testing tool, not a fallback in the installed core CLI. The core workflow covers project creation, checking, formatting, linting, tests, execution, building, packaging, dependency synchronization, inspection, API documentation, diagnosis, installation, and updates. Python interoperability remains opt-in.

The shipped Veritas standard library includes bounded file and CSV/JSONL access, reusable text and collection operations, JSON access, date/time and SemVer handling, local date access behind an explicit clock grant, TOML and SQLite services, Unicode operations, HMAC support, HTTP, and package resolution. Nine complete applications have initial and follow-up acceptance runs on both release targets. The three fresh matched builder evaluations cover small automation CLIs, concurrent network tools, and maintainable local applications. Their class recommendations are `either`, `veritas`, and `either`, respectively. Each assessment records its observed user benefit, setup and delivery evidence, and material regressions; this three-pair evaluation is task-specific evidence, not a general adoption estimate. It does not revise the frozen 18-choice result.

## Final source and artifact identities

The product source commit is `cb5587b1f4d48a72b955ad2e6e666cdef28f5373` on branch `veritas/north-star-sprint`, based on `b8755f08e8c453051572b9e57a34fd1da0a73199`. The final audited product snapshot is `sha256:4d515d48ba1912119df9cb800cc73d6c1a8ddf02ca25a66de4589b51c9258066`.

| Identity | SHA-256 |
|---|---|
| Artifact format schema | `e50354f9431c7e244d093a0a2ad94fa897b91b8eb1793338d9dfce94fe38ce10` |
| Embedded compiler seed | `86aa851606f499999af2ad5a30daf708b62b1ecc571904b62896dd2613ed8c20` |
| Compiler seed manifest | `6fa10cb874c55367f61039604c791af8b3622b149673e90219aeafc64615f6de` |
| Runtime source tree | `2f43071c84548aa37dac0ced5f0b993cb7abf3b3352c5dbd2b038e3de10598a3` |
| Standard-library source tree | `19b745d51f8124fb98e7aec01ae9a2980e14f7fe8a8f8592395d63b807317600` |

The installed artifact format is version 39. Machine-readable release and performance reports under `evaluation/completion/results/` retain target artifact hashes, full benchmark samples, confidence intervals, correctness checks, and package measurements.

## Targets and verification

Supported release targets are macOS ARM64 and Debian 12-compatible GNU/Linux ARM64 using glibc. Other Linux distributions and libc implementations, architectures, Windows, browser, and phone targets are unverified or outside this release scope.

The final acceptance sequence is:

1. `cargo xtask conformance --inventory-only`
2. `cargo xtask conformance --compilers stage2,stage3 --target both`
3. `cargo xtask bootstrap --stages 3`
4. `cargo xtask stdlib --group data`
5. `cargo xtask stdlib --group services`
6. `cargo xtask runtime --target macos-aarch64` and `cargo xtask runtime --target debian-aarch64`
7. `cargo xtask product --group dependencies`
8. `cargo xtask product --group lifecycle`
9. `cargo xtask product --group applications --stage initial,follow-up --target both`
10. `cargo xtask release --target both`
11. `cargo xtask performance --installed`
12. `cargo xtask builders`
13. `cargo xtask closeout`

The current final performance report compares the installed Veritas product to pinned CPython 3.14.7 and records a win on each required advertised compute/data workload. The complete ratios, 95% intervals, timing samples, ties, regressions, memory, and package sizes are in `evaluation/completion/results/performance-host.json`. A free-threaded CPython executable was not available, so the comparison to free-threaded Python concurrency remains unverified. These results do not make a general claim about every Python program.

## Installation, update, and removal

Use the distribution archive for the matching supported host, extract it, and run `sh install.sh` from the extracted distribution directory. The default prefix is `~/.local`; pass `sh install.sh /chosen/prefix` to use another prefix. The installer creates `bin/veritas` and keeps the versioned product under `share/veritas/`. Add the chosen prefix's `bin` directory to `PATH` using the method for the user's shell; the installer does not edit shell startup files.

To update, extract the newer distribution and run its installer with the same prefix. It installs a new versioned directory, retargets the managed `bin/veritas` launcher, and retains older version directories for recovery. The installer refuses to overwrite an existing directory with the same release name.

To remove a dedicated installation, remove its `bin/veritas` launcher and its `share/veritas/<release-directory>` tree. If the prefix contains other tools or Veritas versions, remove only the matching Veritas paths. No shell startup files are changed.

Release archives are unsigned and were not published or notarized. Verify archive identity against the SHA-256 recorded in its package metadata before distributing it through another channel.

## Maintenance ownership

R. Michael Thomas owns the Veritas product and this completion state. Changes to the language contract, compiler seed, runtime host ABI, standard library, native component manifest, or supported targets require updating their versioned compatibility documentation and rerunning the affected conformance, bootstrap, target, application, release, and performance gates. The recorded `cargo xtask closeout` sequence is the full completion check.
