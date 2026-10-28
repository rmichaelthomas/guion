# Build and verify

All commands run from the repository root. Node 22 or later; Python is invoked as `python3`, never `python`. Rust 1.97.1 with the `wasm32-unknown-unknown` target is needed only to rebuild the playground (`export PATH="$HOME/.cargo/bin:$PATH"`).

## Commands

| Task | Command |
|---|---|
| Run every test | `node --test 'site/tests/*.test.mjs'` |
| Re-render vendored instruments | `node site/tools/vendor-instruments.mjs` |
| Rebuild the playground Wasm | `sh site/tools/build-wasm.sh` |
| Build `site/public/` | `node site/tools/build.mjs` |
| Check `site/public/` | `node site/tools/check-site.mjs site/public` |
| Regenerate the generated agent documents | `node site/tools/agent-docs.mjs` |
| Preview at the real subpath | `node site/tools/serve.mjs` then open `http://127.0.0.1:8080/guion/` |

Quote the glob: Node 22 treats an unquoted directory as a file.

## The order for a content change

1. Edit `site/src/` (sections, `claims.json`, `instruments.json`, `content/*.json`, styles).
2. `node site/tools/vendor-instruments.mjs` if instruments changed.
3. `node site/tools/build.mjs`.
4. `node --test 'site/tests/*.test.mjs'` — must be all green. The claims check (`checkClaims`) fails on any figure not found in its source.
5. `node site/tools/agent-docs.mjs`, then the tests again: the instrument map and content-sources documents are compared with a regeneration.
6. `node site/tools/check-site.mjs site/public`.
7. Look at it in a browser, both views, at 1440 px and 390 px (the browser checklist is in [qa/RESULTS.md](../qa/RESULTS.md)).

## What the checker enforces

R1 every file in `site/public/` is linked from a page (the `playground/` tree is a declared asset) · R2 every `#anchor` has a target · R3 no absolute, protocol-relative or remote URL, no `@import` · R4 the only script is the playground loader · R5 evidence file hashes match the evidence index · R6 each page is at most 400 KB · R7 `.nojekyll` exists.

## Rebuilding the Wasm

`site/tools/build-wasm.sh` builds `guion-vm-wasm` (browser ABI 1.0) from the workspace and `site/tools/wasm-compiler` (a standalone crate, not a workspace member) against `crates/compiler` and `crates/model`, both `--locked --offline`. It writes `site/public/playground/guion_vm.wasm`, `guion_compiler.wasm` and `wasm.build.json` (toolchain and SHA-256). The prebuilt files are committed. If the lock for the adapter needs refreshing, seed it from the workspace `Cargo.lock`.

## Deploy

Pages deploys from the `gh-pages` branch at its root (Settings > Pages > Deploy from a branch). That branch holds only the contents of `site/public/`, so nothing else in the repository is served. Source branches never reach it by themselves: after a change to `site/` is merged to `main`, run

```sh
node site/tools/publish-branch.mjs --dry-run   # shows the commit it would make
node site/tools/publish-branch.mjs             # fetches, commits origin/main:site/public on top of gh-pages, pushes (never forced)
```

It works on git objects only, so it is safe from any checkout and never touches your working tree. It publishes what is on `origin/main`, refuses a `site/public` without `index.html` or `.nojekyll`, and does nothing if the tree is already published. Pages then rebuilds on its own; confirm with `curl -sI https://rmichaelthomas.github.io/guion/` and check that a private path such as `.../guion/Cargo.toml` is 404.

The earlier Actions-based deploy was removed because the Actions-based Pages deploy cannot run while Actions billing is blocked; it is in git history (PR #22) if you want to restore it, in which case Pages Source must be switched back to GitHub Actions. The page lives under `/guion/`, so every path in `site/public/` is relative.

## Reporting

State what you ran and what you saw. Do not say "passing" without the command's output, and say plainly what was not run (other browsers, screen readers, CI).
