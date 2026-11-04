# Track A: package system and registry

Branch `feat/package-registry`, built on top of `feat/stdlib-wave-1`. Tests run on this machine against the registry under `wrangler dev --local` on loopback. After the user approved deployment, the Worker was deployed to Cloudflare at https://guion-registry.thomasbuild.workers.dev (R2 bucket `guion-registry`) and that URL became the client default. The live registry is empty apart from one publisher record; only the read side was smoke-tested live (`guion add` of an unknown name reports no versions).

## What now works

| Stage | What it does |
|---|---|
| A1 names | One module owns the rules: lowercase, at most 64 characters, `-` and `_` count as the same character, standard-library names, `std`, `guion` and the `guion-` prefix are reserved, and `@owner/name` is refused with a clear message (including `pkg:@owner/name/...` imports in both compilers). New adds follow the rules; existing locks and manifests still load and get a warning. |
| A2 versions | Version ranges (`^1.2`, `~1.2.3`, `>=1.0, <2.0`, `=1.4`, `*`; a bare `1.2.3` still means exactly that) and a resolver that picks the highest satisfying version, backtracks, skips yanked versions unless a lock already uses them, and says which requirements clash and who asked for each. |
| A3 libraries | `guion new --lib NAME` creates a library. `guion check tests.gu` also checks that each exported module has public items. |
| A4 capabilities | A library declares `[capabilities]` (network hosts, file access, environment variables, commands, clock, Python). `guion check` compares that with what static inspection finds in the library's own code: using something undeclared is an error, declaring something unused is a warning. The lock records the declaration, and `guion update` stops when an update widens it until you pass `--approve-capabilities`. |
| A5 identities | Artifact format 60 records each linked package's name, version and content hash in the build metadata. Older formats read and write exactly as before. |
| A6 registry | A written protocol (`docs/registry-protocol.md`) and a reference implementation as a Cloudflare Worker over R2 (`registry/`): public static index, objects named by hash, a hash-chained append-only log, per-publisher tokens stored only as hashes, name ownership, immutable versions, yank and unyank. |
| A7 client | `guion add NAME [--version REQ]`, `guion publish`, `guion yank`. The client checks that the index, the log entry and the downloaded bytes agree, and refuses to install if any of them disagree. |

## What is not done, and why

- **Default registry address (closed after approval).** The client defaults to the deployed Worker; `GUION_REGISTRY_URL` and `[registry] url` still override it.
- **Registry dependencies (closed).** A library may list `name = "^1.0"` under `[dependencies]`. `guion publish` acquires each into a staged copy, runs `guion check` there, and ships a manifest with exact version, URL and hash, so consumers install the whole graph with one `guion add`. The ranges are resolved at publish time, not at install time, and the index's own dependency list stays empty. Covered by `a_published_library_can_depend_on_another_registry_library`.
- **`guion update` does not look for newer registry versions.** A registry package is installed as a pinned archive, so `update` re-fetches the same one. To move to a newer version, run `guion add NAME --version ...` again.
- **Cached `.gub` attachments are not implemented.** The index and publish request reserve a field for them; nothing produces or reads them.
- **The lock records what a library declares, not what inspection found.** The comparison happens at `guion check` and before `guion publish`, so a published library cannot have undeclared use, but the lock does not store the inspection result separately.
- **Hosts in `network = [...]` are not verified.** Static inspection can tell that a library uses the network, not which host it contacts, so the list is a declaration for reviewers.
- **No token revocation or rotation tooling, and no signing.** Tokens are created by the operator through an administrator call. The signature field exists and is always `null`.
- **Deployment.** Done with the owner's approval. The admin token and the first publisher token are in `~/.guion-registry/` on the owner's machine (mode 600), never in the repository. No package has been published to the live registry yet.
- **Plain HTTP is accepted for package URLs on this machine only** (`localhost`, `127.0.0.1`), so the registry can be tested without certificates. Every other address still needs HTTPS.
- A library has no `main`, so its checks, tests and docs go through a small `tests.gu` beside it (`guion check tests.gu`, `guion test tests.gu`, `guion doc tests.gu`).
- The registry tests need `wrangler` and `node`. If `wrangler` is missing the Rust test skips itself with a message; `scripts/verify-package-registry.sh` treats a skip as a failure.

## Evidence

`scripts/verify-package-registry.sh` writes `package-registry-verification.md` with one row per check.
