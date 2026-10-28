# Guion site

Source, tools and tests for the public Guion page, served by GitHub Pages at `https://rmichaelthomas.github.io/guion/`. **Only `site/public/` is deployed**; everything else here is working material and is never served.

```sh
node --test 'site/tests/*.test.mjs'     # all tests (quote the glob)
node site/tools/vendor-instruments.mjs  # render AllHereHTML instruments into site/vendor/allhere/
node site/tools/build.mjs               # assemble site/public/ from site/src/
node site/tools/check-site.mjs site/public
node site/tools/agent-docs.mjs          # regenerate the generated agent documents
node site/tools/serve.mjs               # preview at http://127.0.0.1:8080/guion/
node site/tools/publish-branch.mjs      # publish origin/main:site/public to the gh-pages branch (--dry-run to preview)
sh site/tools/build-wasm.sh             # rebuild the playground .wasm files (Rust 1.97.1 + wasm32 target)
```

What it is: one landing page with a Plain English view and a Developer view (a native radio switch, no script), an in-browser playground that compiles and runs Guion in WebAssembly when the visitor presses Run, a set of "receipts" where every figure is tied to the report it came from, and a plain evidence page that publishes those reports with their SHA-256. It makes no request to anyone else and uses no remote fonts.

- `src/` the page (`shell.html`, `sections/`, `styles/`, `claims.json`, `content/`, `playground/`)
- `tools/` build, check, vendor, serve, Wasm build, generators (Node, no dependencies)
- `vendor/allhere/` instrument fragments and CSS rendered by AllHereHTML, with `PROVENANCE.md`
- `public/` the deployed directory (generated and committed; a test fails if it is stale)
- `agents/` documents for agents: start at [agents/README.md](agents/README.md)
- `qa/` browser results and screenshots: [qa/RESULTS.md](qa/RESULTS.md)

Design: [`docs/superpowers/specs/2026-10-27-guion-site-design.md`](../docs/superpowers/specs/2026-10-27-guion-site-design.md). Plan: [`docs/superpowers/plans/2026-10-27-guion-site.md`](../docs/superpowers/plans/2026-10-27-guion-site.md). Pages is set to deploy from the `gh-pages` branch (root). `site/tools/publish-branch.mjs` puts the contents of `site/public/` on that branch; see [agents/build-and-verify.md](agents/build-and-verify.md#deploy).
