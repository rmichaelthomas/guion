# Stability policy

This page says what you can rely on when you move between Guion releases, and what you cannot. It applies from Guion 0.2.0. Guion is pre-1.0, so the promises are narrower than a 1.0 policy would make.

## Source compatibility

Within one minor series (0.2.x), a patch release does not change what valid Guion source means: programs and projects that build and run on 0.2.0 keep building and running on later 0.2.x releases. Patch releases fix bugs and add things; they do not remove or reinterpret existing syntax, standard library functions, or command-line options.

A minor release (0.3, 0.4, and so on) may change source. When it does, the change is listed in [CHANGELOG.md](../CHANGELOG.md) and, where you need to act, explained in [compatibility and migrations](compatibility-and-migrations.md).

## Compiled artifacts

The runtime reads every artifact format from 14 up to the current format, which is 64 in this release. Newer runtimes keep reading older formats. An artifact cannot use an operation introduced after its own format version, and a runtime rejects one that does. Building with a newer compiler writes the newer format, so an older runtime cannot read it.

Hashes in artifacts and locks detect accidental change; they do not authenticate who made them.

## Deprecation

Anything that is removed is first deprecated for at least one full minor release. The deprecation is recorded in [CHANGELOG.md](../CHANGELOG.md), and a migration note goes in [compatibility and migrations](compatibility-and-migrations.md). For example, something deprecated in 0.3 can be removed no earlier than 0.5.

The one standing exception is the read compatibility for old Veritas project names. It is a migration aid and may be removed before 1.0 without a further deprecation period; see [compatibility and migrations](compatibility-and-migrations.md).

## Libraries and packages

Library packages are distributed through the package registry (`guion add NAME`, `guion publish`). The registry's rules are in the [registry protocol](registry-protocol.md):

- A published version is immutable. Its contents never change after publishing.
- Yanking a version hides it from new resolution. The package object stays downloadable, and a project whose `guion.lock` already pins that version still installs it.
- `guion.lock` pins the exact version and content hash of every package. A client installs a package only if the index, the registry's log, the lock and the downloaded bytes all agree.
- A library declares the capabilities it needs (for example network hosts) in its manifest. `guion check` compares the declaration with what the code does, and `guion update` will not widen a declaration unless you pass `--approve-capabilities`.

First-party packages are versioned independently of the compiler. `frames` 0.1.0 is the first one published; it needs a Guion release that writes artifact format 64.

## What is not promised

- Speed figures in the documentation are measurements on named machines. They are not guarantees, and your results will differ.
- Features that [status](status.md) describes as preview, not yet done, or not provided are not covered by this policy and may change in any release.
- Support is verified on macOS ARM64 and on Debian 12 (ARM64 and x86_64) with glibc. Other operating systems, CPU architectures, distributions and C libraries have not been verified.
- Nothing here is a promise about 1.0. Before 1.0, any part of the language, library or tooling not covered above may change in a minor release.
