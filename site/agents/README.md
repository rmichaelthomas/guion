# Guion site: start here

This folder is the working material for the public Guion page. **Only `site/public/` is deployed**, to GitHub Pages at `https://rmichaelthomas.github.io/guion/`. Everything else under `site/` is never served.

Read in this order:

1. [site-brief.md](site-brief.md) — what the page is for, who reads it, how it must sound, and what it must never claim.
2. [build-and-verify.md](build-and-verify.md) — the commands, and what "done" means.
3. [extending.md](extending.md) — how to add a section, a claim or an instrument without breaking the rules.
4. [allhere-toolkit.md](allhere-toolkit.md) — how to find and use AllHereHTML's instruments (the design material).
5. [content-sources.md](content-sources.md) and [instrument-map.md](instrument-map.md) — generated: where every figure comes from, and which instrument is where.

Browser evidence is in [qa/RESULTS.md](../qa/RESULTS.md). A machine-readable index is [index.json](index.json).

## The layout

```
site/src/          the page: shell.html, sections/*.html, styles/*.css, claims.json, content/*.json, playground/
site/tools/        build, check, vendor, serve, wasm build, generators (Node, no dependencies)
site/vendor/       AllHereHTML instrument fragments and CSS, with PROVENANCE.md
site/public/       THE DEPLOYED DIRECTORY (generated, committed)
site/agents/       these documents (not deployed)
site/tests/        node:test files, one per tool or surface
site/qa/           browser results and screenshots (not deployed)
```

`site/public/` is generated. Edit `site/src/`, then rebuild. Do not edit files in `site/public/` or `site/vendor/` by hand.

## Rules that are easy to break

- **No download or install button.** The page names the Linux preview, v0.1.0-preview, and says there is no macOS package yet; downloads live on the GitHub release page.
- **No figure without a source.** A number goes in `site/src/claims.json` with the report that states it, never a bare number in a section file. The tests fail if the report does not say it.
- **Plain English and Developer are two authored views**, not one view with parts hidden. Both live side by side in each section (`.v-plain`, `.v-dev`).
- **No JavaScript except `playground/loader.mjs`**, and it does nothing until the visitor presses Run. No remote request of any kind, no remote fonts.
- **Do not edit the frozen application** in `evaluation/completion/applications/allherehtml/`, and do not create a root `AGENTS.md` or `CLAUDE.md` (or touch any project-level one). These documents live here on purpose.
- **Do not edit AllHereHTML from this repository.** Render its pieces; propose changes there separately.
