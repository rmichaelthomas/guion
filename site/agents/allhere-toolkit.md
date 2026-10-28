# AllHereHTML toolkit

AllHereHTML is a separate, local repository of native-HTML "instruments": 100 pieces in ten families (typography and reading, annotation and evidence, relationships and diagrams, reveals and discoveries, motion and time, navigation and journeys, visual art and media, teaching and guidance, publications and objects, computational experiences). Its visitor output is script-free HTML and CSS. This site uses it as design material.

## Where it is

By default the tools look for a sibling checkout, `../allherehtml`, next to this repository. Set `ALLHERE_REPO=/path/to/allherehtml` to use another. The path must contain `workshop/core.mjs`; if not, the tools stop with `AllHereHTML checkout not found at <path>; set ALLHERE_REPO`.

**Never edit AllHereHTML from this repository.** Render its pieces here; make changes to AllHereHTML in its own repository, on its own branch.

## Find a piece

- `instruments/registry/catalog.json` — each instrument's `id` (`ahi.<family>.<name>`), `name`, `family`.
- `instruments/registry/behaviors.json` — each piece's `renderer`, semantic `element`, `interaction` (`progressive-css`, `details`, `anchor`, `none`, …), what content slots it `accepts`, its `behavior` contract and its `fallback`.
- Browse them visually: from the AllHereHTML repository run `python3 -m http.server 8000` and open `http://127.0.0.1:8000/workshop/`, then the Library shelf.

## Use a piece: two modes

Every instrument on this page is one of:

- **Vendored** (`mode: "render"` in `site/src/instruments.json`). AllHere's own `createInstrument` and `renderVisitorDocument` (`workshop/core.mjs`) render it; `site/tools/vendor-instruments.mjs` strips AllHere's per-piece contract note, extracts the family CSS, nests it under `.ahi`, and writes `site/vendor/allhere/fragments/<slot>.html`. The page places it with `{{slot:<id>}}`.
- **Authored** (`mode: "author"`, or just a `data-ahi="<id>"` attribute in a section file). Hand-built markup that follows the registry contract: the semantic element, the native interaction (`<details>`, radio + `:has()`, anchors) and the fallback the registry names.

**Why two modes.** The registry's renderers take four text fields (`heading`, `body`, `caption`, `note`) and several inject fixed labels ("Start / Then / Next", "Here / There", "Before / During", "Another part of the story appears here."). That is right for a demo and wrong for real content such as a 16-command table or a dated timeline. Use vendored output when the piece's structure fits the content, and author the markup when it does not, rather than bending content to the labels. The vendor test fails if AllHere's boilerplate leaks into a vendored fragment.

## Vendor a piece

```sh
node site/tools/vendor-instruments.mjs
```

reads `site/src/instruments.json` (`{ id, mode, instrumentId, content }`), renders every `render` slot and rewrites `site/vendor/allhere/` including `PROVENANCE.md` (AllHere commit, registry hash, per-file SHA-256). Commit the result: the site builds without the AllHere checkout.

To add one: add a slot, run the command, add `{{slot:<id>}}` to a section, rebuild, read the fragment in `site/vendor/allhere/fragments/`, and look at it in a browser. Test the content for fixed labels you do not want (the vendor test's `CHROME` list is where to add them).

## Check the AllHere side

From the AllHereHTML repository (these are its own commands, from its `START_HERE.md`):

```sh
python3 instruments/registry/validate_catalog.py
node --test workshop/test_core.mjs workshop/test_library.mjs workshop/test_ui_shell.mjs workshop/flagships/test_flagships.mjs
```

AllHere's acceptance matrix keeps every instrument "not release-qualified": its per-entry accessibility, responsive, print and cross-browser checks are not complete. This site checks its own use of each piece in a browser (see `site/qa/RESULTS.md`); do not claim more about a piece than that.

## Reference

The two flagship documents in the AllHereHTML repository (`workshop/flagships/exports/`) show the instruments composed into full pages; the Guion exhibition there is a good model for tone. The current mapping of this page to instruments is [instrument-map.md](instrument-map.md).
