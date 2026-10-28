# Guion Site Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans (the user chose native, inline execution) to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. **No commit steps:** the user has not asked for commits; leave work uncommitted and report. If asked to commit, run the session-contract pre-commit gate first.

**Goal:** Build the Guion landing page and showcase in `site/`, served by GitHub Pages from `site/public/`, with a Plain English / Developer view switch, a working in-browser playground, claim "receipts", and agent documents.

**Architecture:** Hand-authored page shell and section partials in `site/src/` are assembled by a dependency-free Node build (`site/tools/build.mjs`) into committed output in `site/public/`. AllHereHTML instruments are rendered by AllHere's own `workshop/core.mjs` into vendored fragments with provenance and slotted into the shell. The playground runs two Wasm modules (compiler adapter + `guion-vm-wasm` ABI 1.0) built from this repo. Every figure on the page is a `claims.json` entry checked against the source file it cites.

**Tech Stack:** Node 22 (`node --test`, ESM, no npm dependencies), Rust 1.97.1 with `wasm32-unknown-unknown` (`export PATH="$HOME/.cargo/bin:$PATH"`), plain HTML/CSS, `playwright-cli` for browser QA, GitHub Actions Pages deploy.

**Spec:** `docs/superpowers/specs/2026-10-27-guion-site-design.md` (approved 2026-10-27). **Executed with deviations**: `/developer/` page instead of `#developer`; vendored vs authored instruments; `serve.mjs` built earlier; extra `components.css`; `agents-surface.mjs`, `agent-docs.mjs`, `ratios.mjs`, `highlight.mjs` tools. Reasons are in the execution ledger and in the final report. Read it first; section numbers below ("§") refer to it.

## Global Constraints

- Site source lives in `site/`; **only `site/public/` is deployed**. `site/agents`, `site/src`, `site/tools`, `site/vendor`, `site/qa` are never served (§4).
- All paths in `public/` are **relative**; the site is served under `/guion/` (§4). No URL beginning with `/` or `http`, except `mailto:`.
- Visitor page: **no JavaScript except one module, `playground/loader.mjs`, which does nothing until the visitor presses Run**; Wasm is fetched on that press; **0 remote requests; no remote fonts** (system stacks, Georgia accent) (§2, §8).
- Plain English view is the default; the switch is CSS-only (`:has(#view-dev:checked)`), deep link `/developer/` (a second generated page, Developer pre-checked; replaced `#developer`, see the ledger); setting is not persisted (§7).
- No download/install CTA; the page states "No public Guion release has been published" in plain words (§2).
- Claims: every figure traces to a file; use the 2026-10-27 figures from `README.md` and `docs/status.md`; never broader than the report; show unfavorable results with equal weight (§6).
- "Say it's wrong" is `mailto:rmichael@rmichaelthomas.com` with the claim name in the subject (§6).
- Instruments are a design tool: **no page section about AllHereHTML**; one footer line "Made with AllHereHTML" (§5, §12).
- Palette/identity from the mockup: paper `#e9ede3`, ink `#11251d`, blue `#2148e8`, lily `#d9f987`, orange `#fa784e`, muted `#607569`, line `#afbaad` (§5).
- Motion respects `prefers-reduced-motion`; every instrument keeps its documented fallback; print stylesheet yields a booklet (§5).
- Do not edit `evaluation/completion/applications/allherehtml/`, root `AGENTS.md`/`CLAUDE.md` (do not create them), or any other project-level AGENTS/CLAUDE file.
- Size target: HTML+CSS ≤ 400 KB uncompressed; Wasm ≈ 1.3 MB fetched only on Run (§11).
- Python is invoked as `python3`, never `python`.

## Review Focus

1. **JavaScript disabled:** the view switch still works; the playground shows a readable transcript and a notice, never an empty box. (Task 6, Task 10)
2. **320 px width:** the giant hero wordmark, code blocks, claim tables and the chapter rail must not cause horizontal page scroll. (Task 10)
3. **Hostile playground input:** empty source, source > 64 KiB, an infinite loop, a project import, a file read, a type error, and a missing/404 Wasm file each produce a specific readable message and a re-enabled Run button. (Task 4, Task 10)
4. **Drift:** a figure edited in `claims.json`, a source report changed after copy, or an evidence file edited after hashing must fail the build/check, not ship. (Task 5)
5. **Subpath and fresh-load:** every link works from `/guion/`; opening `/guion/developer/` in a fresh tab lands in Developer view; printing Developer view is legible. (Task 6, Task 10)

---

## File Structure

```
site/
  package.json                      {"type":"module","private":true}
  README.md                         how to build, check, serve, deploy
  tests/*.test.mjs                  node:test files, one per tool
  tools/
    lib/paths.mjs                   SITE_ROOT, REPO_ROOT, allhereRepo()
    vendor-instruments.mjs          renders AllHere instruments into vendor/allhere/
    build-wasm.sh                   builds both .wasm into public/playground/
    wasm-compiler/                  standalone crate: Cargo.toml, Cargo.lock, src/lib.rs
    claims.mjs                      loadClaims, checkClaims, renderClaimCard
    evidence.mjs                    publishEvidence
    build.mjs                       buildSite
    check-site.mjs                  checkSite
    serve.mjs                       static server at /guion/ for local and QA
  src/
    shell.html                      document shell with {{...}} slots
    styles/{tokens,base,layout,plain,developer,print}.css
    sections/00-bar.html ... 11-footer.html
    instruments.json                slot definitions for vendoring
    claims.json                     claim cards and figures
    playground/loader.mjs           copied to public/playground/
    playground/samples/*.gu + samples.json
  vendor/allhere/                   fragments/*.html, family-*.css, PROVENANCE.md
  public/                           DEPLOYED: index.html, playground/, evidence/, llms.txt, agents.json, .nojekyll
  agents/                           README.md, site-brief.md, allhere-toolkit.md, instrument-map.md,
                                    content-sources.md, build-and-verify.md, extending.md, index.json
  qa/                               RESULTS.md and screenshots
.github/workflows/pages.yml
```

Template syntax used by `shell.html` and sections (resolved by `buildSite`): `{{include:sections/03-why.html}}`, `{{slot:hero-headline}}`, `{{claim:richards}}`, `{{report:jsBytes}}`. An unresolved token throws.

---

### Task 1: Scaffold, paths, test harness

**Files:** Create `site/package.json`, `site/tools/lib/paths.mjs`, `site/tests/paths.test.mjs`, `site/README.md` (stub).

**Interfaces:**
- Produces: `SITE_ROOT: string` (abs path of `site/`), `REPO_ROOT: string`, `allhereRepo(env = process.env): string` (`env.ALLHERE_REPO` or `REPO_ROOT/../allherehtml`, throws `Error("AllHereHTML checkout not found at <path>; set ALLHERE_REPO")` if `workshop/core.mjs` is missing).

- [ ] **Step 1: Write failing test** `paths.test.mjs`: `SITE_ROOT` ends with `/site`; `REPO_ROOT` contains `Cargo.toml`; `allhereRepo({ALLHERE_REPO:'/nonexistent'})` throws a message containing `set ALLHERE_REPO`; `allhereRepo({})` returns a path containing `workshop/core.mjs`.
- [ ] **Step 2:** Run `node --test 'site/tests/*.test.mjs'` → FAIL (module missing).
- [ ] **Step 3:** Implement `paths.mjs`; add `package.json`; write README stub listing the commands in Task 13.
- [ ] **Step 4:** Run `node --test 'site/tests/*.test.mjs'` → PASS.

### Task 2: Instrument vendoring (AllHere → fragments)

**Files:** Create `site/tools/vendor-instruments.mjs`, `site/src/instruments.json`, `site/tests/vendor.test.mjs`, outputs under `site/vendor/allhere/`.

**Interfaces:**
- Consumes: `allhereRepo()`; AllHere `workshop/core.mjs` exports `createDocument({id,title,theme})`, `createInstrument(catalog, instrumentId, content, nodeId)`, `renderVisitorDocument(document, catalog, {title})` (returns a full HTML string), and `instruments/registry/catalog.json`.
- Produces: `renderSlot(allhere: string, slot: {id: string, instrumentId: string, content: object}): Promise<{html: string, css: string}>` — renders a one-node document, extracts the `<section … data-ahi="…">…</section>` element (re-id'd to `slot-<id>`) and the `<style>` text; `vendor({allhere, slots, outDir}): Promise<{allhereCommit: string, files: {path: string, sha256: string}[]}>` writes `fragments/<slot-id>.html`, `family-<family>.css` (deduplicated, one per family), `PROVENANCE.md` (AllHere `git rev-parse HEAD`, registry `catalog.json` sha256, per-file sha256, generation command). CLI: `node site/tools/vendor-instruments.mjs` reads `src/instruments.json` `{slots:[{id,instrumentId,content}]}`.
- Slot ids for two-view content use `.plain`/`.dev`-style suffixes with a hyphen (`diagram-lifecycle-plain`, `diagram-lifecycle-dev`).

- [ ] **Step 1: Write failing tests** in `vendor.test.mjs`: (a) `renderSlot` for `ahi.typography-reading.kinetic-headline` with heading `"guion."` returns html containing `data-ahi="ahi.typography-reading.kinetic-headline"` and `guion.`, no `<script`, no `http`; (b) one test per family (10) using the slot list in `instruments.json` asserting the same no-script/no-remote property; (c) running `vendor` twice yields identical sha256 lists (determinism); (d) `PROVENANCE.md` contains the AllHere commit hash and `catalog.json` sha256; (e) unknown instrument id rejects with a message containing the id.
- [ ] **Step 2:** Run `node --test 'site/tests/*.test.mjs'vendor.test.mjs` → FAIL.
- [ ] **Step 3:** Implement per Interfaces. First, inspect actual rendered output for 3 instruments to confirm the extraction regexes (section element boundaries; `<style>` contents) — adjust to what AllHere emits. Use `node:crypto` sha256 and `child_process.execFileSync('git', ['-C', allhere, 'rev-parse', 'HEAD'])`.
- [ ] **Step 4:** Author `instruments.json` with the slots needed by Tasks 7–9 following the §5 table: at least one instrument from each of the 10 families, using the instrument names in that table. Slot `content` carries the Plain-view copy or the structured data the renderer needs (read each renderer's `accepts` in `instruments/registry/behaviors.json`). If a candidate renders poorly (empty, broken, unreadable), swap within the same family and note it in `site/agents/instrument-map.md` (Task 12).
- [ ] **Step 5:** Run `node site/tools/vendor-instruments.mjs`, then `node --test 'site/tests/*.test.mjs'vendor.test.mjs` → PASS.

### Task 3: Wasm build (compiler adapter + ABI 1.0 VM)

**Files:** Create `site/tools/build-wasm.sh`, `site/tools/wasm-compiler/{Cargo.toml,Cargo.lock,src/lib.rs}`, `site/tests/wasm-build.test.mjs`; outputs `site/public/playground/{guion_compiler.wasm,guion_vm.wasm,wasm.build.json}`.

**Interfaces:**
- Produces: `guion_compiler.wasm` exports `allocate_source(len)->ptr`, `release_source(ptr,len)`, `compile_source(ptr,len)->0|1`, `result_artifact_ptr/len`, `result_error_ptr/len`, `memory` (the adapter API in AllHereHTML `guion/wasm-compiler/src/lib.rs`, 64 KiB source cap, 1 MiB artifact cap, 1 KiB error cap). `guion_vm.wasm` is `guion-vm-wasm` (ABI 1.0: `guion_wasm_abi_version`, `allocate_input`, `release_input`, `execute_artifact_v1(artifactPtr, artifactLen, requestPtr, requestLen)->0|1`, `result_response_ptr/len`). `wasm.build.json`: `{rustc, cargoLockSha256, files:{name:{bytes,sha256}}}`.

- [ ] **Step 1: Write failing test** `wasm-build.test.mjs` (skips with message if the `.wasm` files are absent so the suite stays green on machines without Rust): loads both modules with `WebAssembly.instantiate(bytes, {})`; asserts `guion_wasm_abi_version() >>> 16 === 1`; compiles `fn main() -> Int:` hello program (use `hello.gu` from `docs/agents/examples` or `README` starter) and runs it via `execute_artifact_v1` with request `{"abi":{"major":1,"minor":0},"entry":"main","arguments":[],"capabilities":[],"limits":{"steps":10000,"outputBytes":65536}}`; asserts response `ok === true` and `stdout` equals `Hello, Guion!\n`; asserts `wasm.build.json` sha256 values equal the files' actual hashes.
- [ ] **Step 2:** Run → FAIL (no wasm).
- [ ] **Step 3:** Copy AllHere's `guion/wasm-compiler/src/lib.rs` and `Cargo.lock` into `site/tools/wasm-compiler/`; write its `Cargo.toml` as a standalone crate (`[workspace]` empty table so it is not a member of the repo workspace) with path deps `guion-compiler = { path = "../../../crates/compiler" }`, `guion-model = { path = "../../../crates/model" }`, `crate-type = ["cdylib","rlib"]`. Keep the adapter's own Rust tests (`cargo test --manifest-path site/tools/wasm-compiler/Cargo.toml --locked --offline`). Write `build-wasm.sh` (POSIX sh, `set -eu`): builds `guion-vm-wasm` from the workspace and the adapter, both `--release --locked --offline --target wasm32-unknown-unknown --target-dir "$REPO_ROOT/target/site-wasm"` (inside the ignored `/target/`), copies to `site/public/playground/`, writes `wasm.build.json`.
- [ ] **Step 4:** `export PATH="$HOME/.cargo/bin:$PATH"; sh site/tools/build-wasm.sh && cargo test --manifest-path site/tools/wasm-compiler/Cargo.toml --locked --offline && node --test 'site/tests/*.test.mjs'wasm-build.test.mjs` → PASS. If `--locked` fails because the lock file needs regenerating, regenerate once offline and keep the result.

### Task 4: Playground loader and samples

**Files:** Create `site/src/playground/loader.mjs`, `site/src/playground/samples/{hello,option,records,denied,loop,mistake}.gu`, `site/src/playground/samples/samples.json`, `site/tests/playground.test.mjs`.

**Interfaces:**
- Consumes: Task 3 modules.
- Produces (exported, DOM-free, testable): `loadModules(compilerBytes: Uint8Array, vmBytes: Uint8Array): Promise<Modules>`; `compile(modules, sourceText: string): {ok: true, artifact: Uint8Array} | {ok: false, code: string, message: string}`; `run(modules, artifact: Uint8Array, limits?: {steps: number, outputBytes: number}): {ok: boolean, stdout: string, steps: number, value?: unknown, code?: string, message?: string}` using `execute_artifact_v1` with `capabilities: []` and default limits `{steps: 10000, outputBytes: 65536}`; `describe(result): {headline: string, detail: string}` — plain-words summary used by the page. DOM wiring at module end, guarded by `typeof document !== 'undefined' && document.querySelector('#pg-run')`: on Run click fetch `./guion_compiler.wasm` and `./guion_vm.wasm` (relative), compile, run, write `#pg-output`, `#pg-status`, `#pg-meta` (steps, artifact bytes, limits); sample `<select id="pg-sample">` loads `samples/<name>.gu`; Run is disabled while working and always re-enabled.
- `samples.json`: `[{"id","title","blurb","expect":{"ok":bool,"stdout"?:string,"code"?:string}}]`.

- [ ] **Step 1: Write failing tests** (`playground.test.mjs`, uses real Wasm from `site/public/playground/`): each sample in `samples.json` compiled and run through `loader.mjs` functions matches its `expect`; `hello` → `Hello, Guion!`; `denied` (reads a file with the file API) → `ok:false`, `code === "R_HOST_UNSUPPORTED"`; `loop` (unbounded loop) → `ok:false`, a step-limit error code taken from the real response; `mistake` (type error) → `compile` returns `ok:false` with a `code` and a message containing the source line; `compile(modules, "")` → `ok:false`; `compile(modules, "x".repeat(65537))` → `ok:false` message mentions `65536`; a source with a project `import` → code `E_IMPORT_PROJECT_REQUIRED`; `describe` of each failure returns non-empty `headline` and `detail`.
- [ ] **Step 2:** Run → FAIL.
- [ ] **Step 3:** Implement `loader.mjs` (port the byte-copy/allocate/release pattern from AllHereHTML `guion/playground/loader.mjs`, replacing the legacy `execute_artifact` with ABI v1; copy the response before the next call). Write the six samples from verified syntax in `docs/agents/examples/*.gu` and `language/examples/*.gu`; record real observed outputs/codes into `samples.json` after running them (do not guess codes).
- [ ] **Step 4:** Run `node --test 'site/tests/*.test.mjs'playground.test.mjs` → PASS.

### Task 5: Claims, evidence, and drift checks

**Files:** Create `site/src/claims.json`, `site/tools/claims.mjs`, `site/tools/evidence.mjs`, `site/tests/claims.test.mjs`, `site/tests/evidence.test.mjs`.

**Interfaces:**
- `claims.json`: array of `{id, title, plain:{say, holdsWhen, doesntTell}, dev:{rows:[[label,value],…], command?:string}, figures:string[], sources:[{path:string, needles:string[]}], evidence:string[]}`. Paths are repo-relative. `figures` are the exact strings shown on the page; every figure must appear (whitespace-normalized) inside the text of at least one listed source.
- `loadClaims(srcDir): Claim[]` (validates shape, unique ids, throws with the claim id); `checkClaims(claims, repoRoot): {id: string, reason: string}[]` (empty = pass; reasons: `missing source`, `figure not found: <figure>`, `needle not found: <needle>`); `renderClaimCard(claim): string` (HTML: Claim / Holds when / Doesn't tell you / Check it / Say it's wrong, with the Developer table inside `<div class="v-dev">`, and `mailto:rmichael@rmichaelthomas.com?subject=Guion%20claim%3A%20<title>`); `publishEvidence({repoRoot, claims, outDir}): Promise<{entries: {name: string, bytes: number, sha256: string, sourcePath: string}[]}>` copies each cited `evidence` file to `outDir/<basename>`, throws on basename collision, writes `outDir/index.html` (plain generated list: name, size, SHA-256, one-line description from the claim, "reproduce with" command).

- [ ] **Step 1: Write failing tests**: valid minimal claim passes; changing one figure to `"9.99 ms"` makes `checkClaims` return `figure not found: 9.99 ms` for that id; a non-existent source path returns `missing source`; `renderClaimCard` output contains all five labels and the exact `mailto:` address; `publishEvidence` hashes equal `sha256` of source files, `index.html` lists every entry and contains each hash, and a collision between two same-named files throws; editing a copied evidence file after publishing makes a second `publishEvidence` report a different hash for it (the check-site step in Task 10 compares index hashes to files).
- [ ] **Step 2:** Run → FAIL.
- [ ] **Step 3:** Implement `claims.mjs`, `evidence.mjs`.
- [ ] **Step 4:** Author `claims.json` from the sources named in `README.md` and `docs/status.md`. Required claim ids and sources (figures copied verbatim from the files): `richards` (current-main-outcome-checkpoint.md: 27.46 ms vs 72.92 ms, ratio 0.375, interval 0.370–0.379); `short-source` (five tasks, ratios 0.182–0.236, 21 pairs); `nine-apps` (ratios 0.137–0.845, seven pairs); `conformance` (127/127 application cases); `jsonl-200k` (3.81 s, 11.1 MiB vs 17.93 s, 74.4 MiB); `lockfile` (171.82 ms, 40.92 MiB vs 179.87 ms, 47.13 MiB; 16,384 packages / 32,768 diagnostics); `concurrency-cancel` (parallel-http-cancellation report); `bridge-bytes` (140.59 vs 161.60 ms, 13.7%, 2.5% slower, 22% slower than stdlib reference); `bridge-serialization` (ratio 1.012, 1.129×, 1.406×; "did not establish a speed change"); `managed-packages` (byte sizes 157,585,367 / 244,199,858 / 274,853,607); `feed-digest` (172,422,430-byte bundle, feedparser 6.0.14); `targets` (cohort 54 of 54; macOS ARM64 + Debian 12 ARM64/x86_64, glibc; current format-44 checkpoint macOS ARM64 only); `python-choice` (historical: independent recommendation campaign selected Python in all 18 choices for the older candidate) — label as historical, different question from current performance. Every `holdsWhen`/`doesntTell` stays inside the report's own stated scope.
- [ ] **Step 5:** Run `node --test 'site/tests/*.test.mjs'claims.test.mjs site/tests/evidence.test.mjs` → PASS; also run `node -e` to execute `checkClaims` on the real `claims.json` and expect `[]`.

### Task 6: Shell, tokens, view switch, build pipeline

**Files:** Create `site/src/shell.html`, `site/src/styles/{tokens,base,layout,plain,developer,print}.css`, `site/tools/build.mjs`, `site/tests/build.test.mjs`, minimal placeholder `site/src/sections/*.html` (replaced in Tasks 7–9).

**Interfaces:**
- Consumes: `renderClaimCard`, vendor fragments/CSS, `publishEvidence`.
- Produces: `buildSite({srcDir, vendorDir, outDir, repoRoot}): Promise<{files: string[], report: {htmlBytes: number, cssBytes: number, jsBytes: number, wasmBytes: number, remoteRequests: number}}>`; writes `outDir/index.html` (all CSS inlined in one `<style>` using `@layer reset, base, layouts, components, states, responsive, print`), copies `src/playground/loader.mjs` and `samples/` into `outDir/playground/`, writes `.nojekyll`. Resolves `{{include:}}`, `{{slot:}}`, `{{claim:}}`, `{{report:}}`; an unresolved token throws `Error("unresolved token: <token>")`.
- **View switch contract (exact):** at the very top of `<body>`: `<span id="developer"></span>`, `<input type="radio" name="view" id="view-plain" class="sr-only" checked>`, `<input type="radio" name="view" id="view-dev" class="sr-only">`; the visible control is a `<fieldset class="viewswitch"><legend class="sr-only">Reading view</legend>` with `<label for="view-plain">Plain English</label><label for="view-dev">Developer</label>`. CSS: `body:has(#view-dev:checked) .v-plain{display:none}`, `body:not(:has(#view-dev:checked)) .v-dev{display:none}`, plus `body:has(#developer:target) …` equivalents that also activate Developer styles; focus ring on the labels when their input has `:focus-visible`. Developer styling (§7): tighter scale tokens (`--step-*` smaller), mono labels, visible 12-column grid on wide screens, marginalia open, chapter rail renders as command index. Plain/Developer are separate authored blocks (`.v-plain`, `.v-dev`) in each section.

- [ ] **Step 1: Write failing tests** (`build.test.mjs`, builds into a temp dir): output has `index.html`, `.nojekyll`, `playground/loader.mjs`; build twice → identical bytes (determinism); a section containing `{{slot:nope}}` throws `unresolved token`; HTML contains exactly one `<script` and it is `<script type="module" src="playground/loader.mjs"></script>`; contains the exact radio/legend/label markup above; CSS contains the two `:has(#view-dev:checked)` display rules and a `@media (prefers-reduced-motion: reduce)` block and `@media print`; no `http://`/`https://` anywhere except none (assert zero); `report.remoteRequests === 0`.
- [ ] **Step 2:** Run → FAIL.
- [ ] **Step 3:** Implement tokens (mockup palette, `--paper`, `--ink`, `--blue`, `--lily`, `--orange`, `--muted`, `--line`, `--radius:19px`, mono/serif stacks), base (reset layer from the mockup: `scroll-padding-top`, `:focus-visible` outline 3px blue, selection lily, skip link), layout primitives (`.shell` width `min(1500px,100%)`, `.section`, `.sectionhead`, sticky `.navwrap`), view-switch CSS, print stylesheet (hide nav/switch/playground editor chrome; show both Plain text and Developer tables? **no** — print whichever view is active; `break-inside:avoid` on cards). Implement `build.mjs` and the `report` computation (byte counts of the written files; `wasmBytes` from `public/playground/*.wasm`).
- [ ] **Step 4:** Run `node --test 'site/tests/*.test.mjs'build.test.mjs` → PASS. Run `node site/tools/build.mjs` and `node site/tools/serve.mjs` (Task 10 provides serve; until then `python3 -m http.server 8080 --directory site/public`) and open the placeholder to see the switch work.

### Task 7: Sections 0–5 (bar, hero, Try it, Why, Lifecycle, Read it)

**Files:** Create/replace `site/src/sections/{00-bar,01-hero,02-try,03-why,04-lifecycle,05-read}.html`; extend `site/src/instruments.json`; `site/tests/sections.test.mjs`.

**Interfaces:** Consumes the slot ids you define in `instruments.json` and Task 4 element ids: `#pg-sample` (select), `#pg-source` (textarea, `spellcheck="false"`, `aria-label`), `#pg-run` (button type=button), `#pg-output` (`<pre aria-live="polite">`), `#pg-status`, `#pg-meta`, plus a static `<noscript>` transcript and a "needs JavaScript and WebAssembly" `<p class="pg-notice">` shown by default and hidden by CSS `:has(#pg-run[data-ready])` (the loader sets `data-ready` on load). Section ids (chapter rail anchors): `#top`, `#try`, `#why`, `#lifecycle`, `#read`, `#permissions`, `#receipts`, `#python`, `#agents`, `#story`, `#footer`.

- [ ] **Step 1: Write failing tests** (`sections.test.mjs` over the built page): every id above exists exactly once; sections 01–05 each contain both a `.v-plain` and a `.v-dev` block; the `#try` section contains all six `#pg-*` ids and a `<noscript>`; the hero contains the text `No public Guion release has been published`; no element text contains "download" or "install" as a call to action (assert no `<a>`/`<button>` whose text matches `/^(download|install|get started)/i`); the lifecycle Developer block lists every subcommand returned by `guion --help` output captured into `site/src/content/cli-commands.json` by a tiny script (so the count is not hard-coded).
- [ ] **Step 2:** Run → FAIL.
- [ ] **Step 3:** Write the sections per §5 rows 0–5. Direction: **hero** keeps "guion." with blue dot and "A language for the work ahead." in `clamp()` type that never forces overflow at 320 px (drop `white-space:nowrap` below 480 px); the illustration is an inline SVG that depicts source (`.gu`) becoming artifact (`.gub`) — real code fragments, not decoration; **Try it** is the first interactive thing after the hero: editor left, output and a three-line "what just happened" right, sample chooser, and in Developer view the meta row (steps used / ceiling, artifact bytes, ABI 1.0, limits 64 KiB source, 1 MiB artifact, 64 KiB output); **Why** uses the three mockup principles rewritten for the Ptah test (a person who has never programmed can say what Guion is for and what it cannot yet do); **Lifecycle** is a flow of the real commands with each step's Plain sentence and Developer flags/report kinds; **Read it** shows real programs with captured outputs (from Task 4 samples and `docs/agents/examples`), curtain-reveal from Plain explanation to Developer signature. Copy rules: Plain avoids "AST/ABI/opcode/effect/monad"; Developer is precise and uses the repo's terms (artifact format number read from `crates/model` at build time, not typed by hand).
- [ ] **Step 4:** Run `node site/tools/vendor-instruments.mjs && node site/tools/build.mjs && node --test 'site/tests/*.test.mjs'` → PASS. Inspect the page in the browser (Task 10 procedure) for sections 0–5 in both views before moving on.

### Task 8: Sections 6–8 (Permissions, Receipts, Python)

**Files:** Replace `site/src/sections/{06-permissions,07-receipts,08-python}.html`; extend `instruments.json`; extend `sections.test.mjs`.

- [ ] **Step 1: Write failing tests:** `#permissions` Developer block contains the literal field names `required_effects`, `manifest_grants`, `grant_assessment`, `present_but_target_dynamic` (taken from `docs/agents/start-here.md`); `#receipts` contains one card per claim id in `claims.json` (count equals `claims.length`), each with the five labels and a working `mailto:`; `#python` lists at least the bridge results where Guion is slower and the historical Python-chosen campaign, each with the word "historical" or its own scope sentence; `#receipts` links to `evidence/index.html` and every evidence file link resolves in `public/`.
- [ ] **Step 2:** Run → FAIL.
- [ ] **Step 3:** Write sections per §5 rows 6–8 and §6. **Permissions:** an interactive "grant ladder" (native `<details>`/`:has()`) showing the same program with zero, file-root, and one-host grants, with the real `guion inspect` JSON in Developer view; end with the playground's own proof (link to the `denied` sample via `#try` with `?`-free anchor: set the select via a `:target`-driven CSS-only hint, not script). **Receipts:** claim cards in a two-column editorial layout; a CSS-only filter by workload class (radio inputs, no script); a comparison bridge of ratios on a log-scale bar with the interval whiskers drawn in inline SVG from `claims.json` numbers; the Richards dossier. **Python:** direct, short, no hedging adjectives; includes the unmeasured free-threaded Python comparison and "Debian ARM64/x86_64 managed packages are functional evidence, not speed claims."
- [ ] **Step 4:** Rebuild; run `node --test 'site/tests/*.test.mjs'` → PASS; view both modes.

### Task 9: Sections 9–11, `llms.txt`, `agents.json`

**Files:** Replace `site/src/sections/{09-agents,10-story,11-footer}.html`; add generation of `public/llms.txt` and `public/agents.json` to `build.mjs`; extend tests.

**Interfaces:** `agents.json`: `{product:"Guion", status:"pre-release; no public release has been published", generated:"<ISO date from claims.json>", claims:[{id,title,say,holdsWhen,doesntTell,figures,evidence:[relative evidence URL],contact}], docs:{agentIndex:"docs/agents/index.json (private repository)"}}`; `llms.txt`: short Markdown (what Guion is, what it is not, status, claims with conditions, link to `agents.json`, contact).

- [ ] **Step 1: Write failing tests:** `agents.json` parses, has one entry per claim with non-empty `holdsWhen` and `doesntTell`; `llms.txt` states "No public Guion release has been published" and contains no claim figure that is absent from `claims.json`; the footer contains `MIT`, the text `Made with AllHereHTML` exactly once, `rmichael@rmichaelthomas.com`, and a Developer block with the four report numbers from `report`; `#story` timeline has the three dated entries from the mockup corrected to the repo (2026-10-22 north-star as Veritas; 2026-10-25 renamed Guion; 2026-10-27 current checkpoint) and no others invented.
- [ ] **Step 2:** Run → FAIL. **Step 3:** Implement. Agents section (Plain): "Your assistant can read this project without guessing" with the copyable prompt in a `readonly` textarea; Developer: the `agents.json` shape and `--json` envelope kinds from `docs/agents/index.json`. **Step 4:** Rebuild; tests PASS.

### Task 10: Site checker, local server, and browser QA

**Files:** Create `site/tools/check-site.mjs`, `site/tools/serve.mjs`, `site/tests/check-site.test.mjs`, `site/qa/RESULTS.md` and screenshots.

**Interfaces:** `checkSite(publicDir: string): Promise<{errors: string[], report: object}>` with rules: R1 every file in `public/` is reachable from `index.html` (via `href`/`src`, transitive through `evidence/index.html`) or listed in `wasm.build.json`/`.nojekyll`; R2 every `href="#x"` has a matching `id`; R3 no `src`/`href`/`url()`/`@import` begins with `/` or `http` (allow `mailto:`); R4 the only `<script>` is the module loader; R5 evidence hashes in `evidence/index.html` equal actual file hashes; R6 `index.html` ≤ 400 KB. `serve.mjs`: `node site/tools/serve.mjs [port=8080]` serves `site/public` at `/guion/` (404 outside) with correct MIME for `.wasm` and `.mjs`.

- [ ] **Step 1: Write failing tests** with fixture dirs: an orphan file → R1 error; a dangling `#nope` → R2; `<img src="/x.png">` → R3; a second `<script>` → R4; a tampered evidence file → R5; passing fixture → no errors. **Step 2:** FAIL. **Step 3:** Implement both tools. **Step 4:** `node --test 'site/tests/*.test.mjs'` PASS; `node site/tools/check-site.mjs site/public` exits 0 on the real build.
- [ ] **Step 5: Browser QA** (load the `playwright-cli` skill; serve with `serve.mjs`; use `http://127.0.0.1:8080/guion/`). Record command, result, and screenshot path for each in `site/qa/RESULTS.md`, including failures and anything unavailable (mark pending, never assume): (1) view switch by click and by keyboard (Tab to the fieldset, arrow keys, Space); (2) fresh load of `/guion/#developer` shows Developer; (3) JS disabled: switch works, playground notice visible, no console errors; (4) playground: Run `hello`, edit source, `mistake`, `denied` (expects `R_HOST_UNSUPPORTED`), `loop` (step limit), empty editor, 70 KiB paste, rename `guion_vm.wasm` temporarily to force a 404 → readable error and Run re-enabled; (5) widths 320, 360, 768, 1280, 1920 for both views — no horizontal page scroll; (6) browser zoom 200% and 400%; (7) `prefers-reduced-motion: reduce`; (8) forced-colors; (9) print to PDF of both views; (10) console clean and **network log shows zero requests before pressing Run and only the two `.wasm` after**; (11) accessibility snapshot: single `h1`, landmark structure, every control named, contrast spot-check of lily-on-ink, blue-on-paper and muted-on-paper. Fix each failure in the owning task's files, rebuild, rerun the failed item, record both runs. Chromium only unless Firefox/WebKit are installed.

### Task 11: Visual review pass

**Files:** Modify `site/src/styles/*.css`, sections as needed; screenshots in `site/qa/`.

- [ ] **Step 1:** Screenshot hero, Try it, Receipts, Permissions at 1440 and 390 in both views. **Step 2:** Review against §3 anti-goals (no feature-card triplets with icons, no gradient blobs, no testimonial furniture; Developer view must look structurally different, not merely longer). Use the `critique` and `polish` skills' guidance on hierarchy, spacing, and type; fix findings. **Step 3:** Re-run `node --test 'site/tests/*.test.mjs'` and `node site/tools/check-site.mjs site/public` after every style change.

### Task 12: Agent documents and AllHere pointer

**Files:** Create `site/agents/{README,site-brief,allhere-toolkit,instrument-map,content-sources,build-and-verify,extending}.md`, `site/agents/index.json`, `site/tests/agents.test.mjs`; in AllHereHTML (separate repo, branch `docs/guion-site-pointer` from `main`, **uncommitted, left for review**): add a "Guion public site" paragraph to `START_HERE.md` and `README.md`.

**Interfaces:** `instrument-map.md` is generated by `node site/tools/vendor-instruments.mjs --map` (adds that flag): a table of page section → slot id → instrument id → family → renderer, plus swaps made in Task 2. `index.json` mirrors the style of `docs/agents/index.json`: `{kind:"guion_site_agent_index", schema_version:1, documents:{…relative paths…}, commands:{build:…, check:…, vendor:…, wasm:…, serve:…, test:…}}`.

- [ ] **Step 1: Write failing test** `agents.test.mjs`: every path in `index.json` exists; every command string in `index.json` appears verbatim in `build-and-verify.md`; `instrument-map.md` lists every slot in `instruments.json` and covers all 10 families; every relative link in `site/agents/*.md` resolves; no file in `site/agents/` is referenced by `public/`.
- [ ] **Step 2:** FAIL. **Step 3:** Write the documents for an agent with no conversation context. `allhere-toolkit.md` must cover: where the AllHere checkout is (`ALLHERE_REPO`), how to list the registry (`instruments/registry/catalog.json`, `behaviors.json`), how to read a piece's contract and fallback, how to render one (`renderSlot`), the Workshop at `python3 -m http.server 8000` → `/workshop/`, the flagship exports as reference, AllHere's own verify commands, and the rule that AllHere files are never edited from this repo. `extending.md` carries the Planes lessons (served set derived from the tree; a served page must be linked; mockups live outside `public/`), the claims rule (add a source needle, never a bare number), and the two-view authoring rule. `site-brief.md` carries purpose, audience, voice, the Ptah test, the anti-goals, and the contact. **Step 4:** Write the AllHere pointer (`git -C /Users/rmichaelthomas/allherehtml switch -c docs/guion-site-pointer`), leaving the edit uncommitted. **Step 5:** `node --test 'site/tests/*.test.mjs'` PASS.

### Task 13: Pages workflow, README, final verification

**Files:** Create `.github/workflows/pages.yml`; finish `site/README.md`.

- [ ] **Step 1:** Write `pages.yml`: `on: push: branches:[main], paths:['site/**']` and `workflow_dispatch`; `permissions: contents: read, pages: write, id-token: write`; `concurrency: group: pages, cancel-in-progress: false`; steps: checkout (`actions/checkout@v7.0.1`, as `ci.yml` uses), `node --test 'site/tests/*.test.mjs'` (Node 22 via `actions/setup-node`), `node site/tools/check-site.mjs site/public`, `actions/configure-pages`, `actions/upload-pages-artifact` with `path: site/public`, `actions/deploy-pages`. Before pinning, read each action's latest release with `gh api repos/actions/<name>/releases/latest --jq .tag_name` and pin to a released tag. Validate YAML with `python3 -c "import yaml,sys; yaml.safe_load(open('.github/workflows/pages.yml'))"` (if `yaml` is missing, use `ruby -ryaml -e` or skip with a note).
- [ ] **Step 2:** Final verification, in order, with outputs recorded in `site/qa/RESULTS.md`: `node site/tools/vendor-instruments.mjs && node site/tools/build.mjs && git status --short site/public` (rebuilding must produce no change); `node --test 'site/tests/*.test.mjs'`; `node site/tools/check-site.mjs site/public`; `cargo test --manifest-path site/tools/wasm-compiler/Cargo.toml --locked --offline`; confirm `cargo xtask agent-reference --check` and `python3 evaluation/completion/check_allherehtml.py --binary target/debug/guion` are unaffected (run the first; run the second only if `target/debug/guion` exists).
- [ ] **Step 3:** Report to the user: what is built, what is verified with evidence, what is pending (browsers not run, CI not run), the exact list of files that would be public (`site/public/**` + the evidence list) for their veto, and that publishing requires merging `site/landing` to `main` (not done) after which Pages source is switched to GitHub Actions via `gh api -X PUT repos/rmichaelthomas/guion/pages -f build_type=workflow` (authorized by the user; execute only after the workflow is on `main`), then confirm the first deploy and `curl` the live URL and an unrelated path (e.g. `Cargo.toml` must now return 404).
