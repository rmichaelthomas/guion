# Veritas checkpoint v1.28 — Python offline-cache integrity

## Change

Current-host package schema 2 records the digest and byte count of immutable payloads in the bundled uv cache. `verify-package` and the launcher recompute the same inventory before use and reject altered or added cache payload files. This closes the integrity gap for the payloads used to seed offline Python environments.

uv updates its cache bookkeeping during ordinary offline resolution. The digest therefore excludes files named `.lock`, files ending in `.lock`, `.http`, or `.msgpack`; hashing these mutable records made a valid bundle fail after its first launch. Cached archives and wheel/source payloads remain covered. Package schema 1 remains accepted and follows its existing runtime, artifact, resource, and lock checks without requiring cache hash fields.

The cache digest detects corruption; it does not authenticate a publisher. The native Mach-O audit still does not scan native images stored only inside the offline cache, and package signatures remain open work.

## Evidence

- A native-wheel app bundled and relocated with its managed CPython and offline cache. It launched twice from a path containing spaces and Unicode, without source files or external Python setup.
- uv's mutable cache records changed during those offline launches while the immutable payload inventory stayed stable.
- The integration test altered a cached payload and verified that `verify-package` rejected it with `offline Python cache integrity check failed`.
- Runtime tampering continues to produce the runtime-integrity error; the fixture now restores the original `.gitignore` bytes after that check, preventing the runtime test from masking the subsequent cache-tampering check.
- Schema 1 compatibility is checked against the retained RC47 package.

## Limits and next work

This milestone covers the cache payload at package verification time. It does not provide signatures, sandboxing, native-wheel inspection for cache-only wheels, cross-host packages, or an independent recommendation evaluation. Continue the integrated implementation plan; keep the north-star recommendation gate open.
