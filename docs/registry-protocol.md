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
      "signature": null
    }
  ]
}
```

Versions are sorted by number. The per-version `signature` is reserved for publisher signatures and always `null` in 0.x; the registry's own signature is on the log (below). Clients must ignore fields they do not know.

### Hash log

Each entry is `{ seq, time, action, name, version, sha256, publisher, prev, hash, key_id, signature }`. `action` is `publish`, `yank` or `unyank`. `prev` is the previous entry's `hash` (64 zeros for the first). `hash` is the SHA-256 of the entry's JSON text with only the keys from `seq` to `prev`, in the order listed. The chain is append-only: an entry is created once and never rewritten, and `head.json` (`{ seq, hash, key_id, signature }`) names the latest.

### Log signatures

The registry signs every log entry and the head with its ed25519 key. `signature` is the hex ed25519 signature of the UTF-8 text `guion-registry-log-v1:<seq>:<hash>`; `key_id` is the first 16 hex characters of the SHA-256 of the 32-byte public key. Because `hash` covers `prev`, a valid signature on an entry vouches for the whole chain up to it. `GET /v1/signing-key` returns `{ algorithm: "ed25519", key_id, public_key }` for convenience, but a client must not learn the key from the registry it is checking: it is pinned in the client for the default registry and configured for any other. A registry without its key configured (the `SIGNING_KEY` deployment secret, the 32-byte seed in hex) refuses every write with 503 `E_SIGNING_UNAVAILABLE`, so its log never contains an unsigned entry.

To rotate the key, clients first ship trusting both the old and the new public key; then the `SIGNING_KEY` secret changes to the new seed. Entries already in the log keep their old signatures and keep verifying for as long as clients list the old key. A client accepts an entry signed by any key it trusts, and `key_id` says which one was used.

## Archives

A package archive is an uncompressed ustar tar of the library source tree: `guion.toml` with a `[package]` table at the root, plus its modules. Entries are regular files and directories only, with relative paths that stay inside the archive. The registry stores and hashes the bytes it is given; it does not unpack them. Clients apply the same extraction limits as for `guion add --url` (path traversal, links and oversized entries are refused) and check that the manifest's name and version match the index.

## Write side (authenticated)

All write calls need `Authorization: Bearer <token>`. Tokens belong to a publisher id and are kept by the registry only as SHA-256 hashes.

### Publish

`PUT /v1/publish/<name>/<version>` with the archive as the request body and these headers:

- `x-guion-sha256`: the archive's SHA-256 (required; the registry recomputes it and rejects a mismatch)
- `x-guion-meta`: JSON `{ "dependencies": {name: requirement}, "capabilities": {...} }` (optional, 16 KiB at most). Other fields are ignored; earlier clients sent an empty `cache` list.

The registry checks, in order: the token; the name rules (the same rules as the client: lowercase, 64 characters or fewer, `-` and `_` identical, standard-library module names, `std`, `guion` and the `guion-` prefix reserved, `@owner/name` reserved and refused); the version is `major.minor.patch`; the archive is non-empty, at most 32 MiB and matches its hash; ownership (the first publish of a name claims it; a different publisher gets 403 `E_NOT_OWNER`, including for spellings that normalize to the same name); and that the version is new (409 `E_VERSION_EXISTS`).

Writes happen in this order, so a failure at any point never leaves a visible half-publish: the object (content-addressed, harmless alone); a pending version record, created only if absent (this is the immutability lock); the log entry (the commit point); the committed version record; the regenerated index. A version appears in the index only once its log entry exists. If a publish fails part-way, the publisher retries the same request: the pending record is resumed if the bytes are identical, and a retry with different bytes is refused.

A successful publish returns `201` with `{ name, version, sha256, log_seq, log_hash }`.

### Yank

`POST /v1/yank/<name>/<version>` and `POST /v1/unyank/<name>/<version>`, owner only. The version stays in the index with `"yanked": true` and its object stays downloadable. New resolution skips yanked versions; a project whose lock already pins one still installs it. Both actions are logged.

### Administration

`POST /v1/admin/publishers` with `Authorization: Bearer <ADMIN_TOKEN>` and body `{ "id": "alice" }` creates a publisher and returns its token once; calling it again for an existing id issues an additional token. The administrator token is a deployment secret, never part of the repository.

`POST /v1/admin/publishers/<id>/revoke` with the administrator token revokes every token the publisher holds and returns `{ "publisher": "<id>", "revoked": <count> }`. Ownership of names is unchanged, so the administrator can then issue a fresh token.

### Tokens

`POST /v1/tokens/rotate` with a publisher token issues a replacement token for the same publisher, returns it once, and revokes the presented token. `POST /v1/tokens/revoke` revokes the presented token. A revoked token is unknown from then on (401 `E_AUTH_INVALID`). The client exposes these as `guion token rotate` and `guion token revoke`, which use `GUION_REGISTRY_TOKEN`.

## Errors

Failures are `{ "error": { "code": "E_...", "message": "..." } }` with an HTTP status: 400 for malformed input (`E_NAME_INVALID`, `E_VERSION_INVALID`, `E_HASH_MISMATCH`, `E_HASH_MISSING`, `E_META_INVALID`), 401 for a missing or unknown token, 403 for a name owned by someone else, 404, 409 for an existing version, 413 for oversized input, 503 when the registry cannot sign (`E_SIGNING_UNAVAILABLE`) or its log is busy (`E_LOG_BUSY`). Messages never contain a token.

## What a client must verify

For every package it installs, a client checks that the same hash appears in all of: the index entry, the log entry at the index's `log_seq`, the lock file, and the downloaded archive's own bytes. Any disagreement fails closed. It also checks that the log entry's own hash recomputes, that the chain links to the previous entry, and, when it knows the registry's public key, that both entries carry a valid signature from it; a missing or invalid signature fails closed. The Guion client pins the default registry's key and reads another registry's key from `GUION_REGISTRY_PUBLIC_KEY`, then `[registry] public_key` in `guion.toml`.

## Not in this version

Publisher signatures (the per-version field is reserved), namespaces and organisation ownership. Precompiled `.gub` attachments are deferred: the compiler links whole programs from source, so nothing could use them until libraries compile separately.
