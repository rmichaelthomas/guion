# Guion registry protocol (version 1)

This is the contract between the `guion` client and a package registry. The reference implementation is the Worker in `registry/`, which runs on Cloudflare R2 and runs locally with `wrangler dev --local`. Any host that serves the same paths and implements the same publish API is a valid registry. The read side is static files, so a CDN can serve it.

Decisions this implements: Guion checkpoint v1.51 §18 (immutable content-addressed index and objects, per-publisher tokens, name ownership, append-only public hash log, reserved signature field), §19 (source is the canonical package form), §20 (versions are immutable; yank hides from new resolution but keeps the object downloadable) and §21 (name rules).

## Read side (public, static)

| Path | Content |
|---|---|
| `GET /index/<name>.json` | The versions of one package. `<name>` is normalized: lowercase, `_` treated as `-`. |
| `GET /objects/<sha256>.tar` | The package source archive, named by the SHA-256 of its own bytes. Immutable. |
| `GET /log/head.json` | `{ "seq": N, "hash": "<hash of entry N>" }` |
| `GET /log/<seq>.json` | One log entry, `<seq>` zero-padded to 10 digits. Immutable. |

Nothing else is served. Ownership records, version records and token hashes are private.

### Index file

```json
{
  "schema": 1,
  "name": "my-lib",
  "owner": "alice",
  "versions": [
    {
      "version": "1.2.0",
      "sha256": "<archive hash>",
      "size": 4096,
      "yanked": false,
      "published_at": "2026-11-04T06:01:01.841Z",
      "log_seq": 17,
      "capabilities": { "files": ["read"] },
      "dependencies": { "other-lib": "^1.0" },
      "cache": [ { "compiler": "0.2.0", "sha256": "<hash of a .gub attachment>" } ],
      "signature": null
    }
  ]
}
```

Versions are sorted by number. `signature` is reserved and always `null` in 0.x. Clients must ignore fields they do not know.

### Hash log

Each entry is `{ seq, time, action, name, version, sha256, publisher, prev, hash }`. `action` is `publish`, `yank` or `unyank`. `prev` is the previous entry's `hash` (64 zeros for the first). `hash` is the SHA-256 of the entry's JSON text without the `hash` field, with keys in the order listed. The chain is append-only: an entry is created once and never rewritten, and `head.json` names the latest.

## Archives

A package archive is an uncompressed ustar tar of the library source tree: `guion.toml` with a `[package]` table at the root, plus its modules. Entries are regular files and directories only, with relative paths that stay inside the archive. The registry stores and hashes the bytes it is given; it does not unpack them. Clients apply the same extraction limits as for `guion add --url` (path traversal, links and oversized entries are refused) and check that the manifest's name and version match the index.

## Write side (authenticated)

All write calls need `Authorization: Bearer <token>`. Tokens belong to a publisher id and are kept by the registry only as SHA-256 hashes.

### Publish

`PUT /v1/publish/<name>/<version>` with the archive as the request body and these headers:

- `x-guion-sha256`: the archive's SHA-256 (required; the registry recomputes it and rejects a mismatch)
- `x-guion-meta`: JSON `{ "dependencies": {name: requirement}, "capabilities": {...}, "cache": [{compiler, sha256}] }` (optional, 16 KiB at most)

The registry checks, in order: the token; the name rules (the same rules as the client: lowercase, 64 characters or fewer, `-` and `_` identical, standard-library module names, `std`, `guion` and the `guion-` prefix reserved, `@owner/name` reserved and refused); the version is `major.minor.patch`; the archive is non-empty, at most 32 MiB and matches its hash; ownership (the first publish of a name claims it; a different publisher gets 403 `E_NOT_OWNER`, including for spellings that normalize to the same name); and that the version is new (409 `E_VERSION_EXISTS`).

Writes happen in this order, so a failure at any point never leaves a visible half-publish: the object (content-addressed, harmless alone); a pending version record, created only if absent (this is the immutability lock); the log entry (the commit point); the committed version record; the regenerated index. A version appears in the index only once its log entry exists. If a publish fails part-way, the publisher retries the same request: the pending record is resumed if the bytes are identical, and a retry with different bytes is refused.

A successful publish returns `201` with `{ name, version, sha256, log_seq, log_hash }`.

### Yank

`POST /v1/yank/<name>/<version>` and `POST /v1/unyank/<name>/<version>`, owner only. The version stays in the index with `"yanked": true` and its object stays downloadable. New resolution skips yanked versions; a project whose lock already pins one still installs it. Both actions are logged.

### Administration

`POST /v1/admin/publishers` with `Authorization: Bearer <ADMIN_TOKEN>` and body `{ "id": "alice" }` creates a publisher and returns its token once. The administrator token is a deployment secret, never part of the repository.

## Errors

Failures are `{ "error": { "code": "E_...", "message": "..." } }` with an HTTP status: 400 for malformed input (`E_NAME_INVALID`, `E_VERSION_INVALID`, `E_HASH_MISMATCH`, `E_HASH_MISSING`, `E_META_INVALID`), 401 for a missing or unknown token, 403 for a name owned by someone else, 404, 409 for an existing version, 413 for oversized input. Messages never contain a token.

## What a client must verify

For every package it installs, a client checks that the same hash appears in all of: the index entry, the log entry at the index's `log_seq`, the lock file, and the downloaded archive's own bytes. Any disagreement fails closed. It also checks that the log entry's own hash recomputes and that the chain links to the previous entry.

## Not in this version

Signature verification (the field is reserved), namespaces, trusted publishing from CI, token revocation tooling, and organisation ownership.
