# Extending the site

## Add or change a claim

1. Find the report in the repository that states it (`evaluation/completion/results/...` or `docs/status.md`). If no report states it, it does not go on the page.
2. Add or edit the entry in `site/src/claims.json`: `plain` (`say`, `holdsWhen`, `doesntTell`), `dev.rows`, `figures`, `sources` (path plus `needles`, the report's own phrases for the conditions), `evidence` (the files to publish), `group`.
3. Copy figures exactly as the report writes them. A figure goes in a claim, never a bare number in a section file.
4. `{{claim:<id>}}` places the card. Run the tests: a figure, a stated condition, or a multi-digit number in the prose that the sources do not contain fails.
5. Say what the claim does *not* tell the reader, at the same size as the claim, and show the unfavourable result if there is one.

To plot a ratio, add it to `site/src/content/ratios.json`; each value must appear in the report it cites (tested) and the chart plots faster and slower results together.

## Add a section

1. Create `site/src/sections/NN-name.html` as `<section class="section shell" id="name">` (or `darkzone`). Add the include to `shell.html` and a link in `00-bar.html`.
2. Write **both views**: `.v-plain` and `.v-dev` blocks side by side. Developer is denser and uses the repository's terms; it is not Plain with parts hidden.
3. Put styles in `site/src/styles/components.css`; grid children need `min-width: 0` and tracks `minmax(0, 1fr)`, or code blocks stretch the page on phones.
4. Every selected-state control needs a forced-colors rule and a print rule (see the existing ones).
5. Rebuild, test, and look at both views at 390 px and 1440 px.

## Add an instrument

See [allhere-toolkit.md](allhere-toolkit.md). Vendored if its structure fits the content; authored (with `data-ahi`) if not. Then `node site/tools/agent-docs.mjs` refreshes [instrument-map.md](instrument-map.md).

## Lessons carried from the Planes site

These came from the project's earlier Pages site and apply here:

- **Derive the deployed set from the tree, not from an allowlist.** Only `site/public/` is served; being outside it is what keeps a mockup from shipping. An allowlist is how a page once returned 404 for two builds while the deploy stayed green.
- **A served page must be linked.** `check-site.mjs` fails on an orphan. Keep reference mockups outside `site/public/` (they are not deleted: a deleted mockup was once needed again two days later).
- **Reports before words.** The guard that matters is the one that fails when prose and source disagree.

## Things not to do

- Do not create a root `AGENTS.md` or `CLAUDE.md`, and do not edit any existing project-level one. Agent documents for this site live in `site/agents/`.
- Do not add a download, install or "get started" button. The page links nothing off-site; downloads live on the GitHub release page.
- Do not link to private repository URLs as evidence; publish the file instead.
- Do not add JavaScript beyond the playground loader, a remote font, an analytics script, or any request that leaves the origin.
- Do not restyle the vendored fragments by hand; override in `components.css`, which loads after them.
- Do not touch `evaluation/completion/applications/allherehtml/` (frozen evidence) or edit AllHereHTML from here.
