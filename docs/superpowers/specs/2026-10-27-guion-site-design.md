# Guion site: landing page and showcase

Date: 2026-10-27. Branch: `site/landing`. Status: **approved 2026-10-27 and built**; the sections below that changed during the build are marked "(changed during the build)". Results are in `site/qa/RESULTS.md`.

## 1. Intent

Guion needs a front door. The visitor should be able to run a Guion program inside the first scroll, understand in plain words what the language is for, and, if they want more, switch the whole page to a dense developer view with signatures, commands, JSON report names, limits and evidence paths. The page is also a showcase, in the way it is made: its design is composed from AllHereHTML's native instruments. The instruments are a design tool here, not content. The page has no section about them, only a one-line credit in the footer.

Pages is already enabled on this repo (legacy build, `main`, `/`; URL `https://rmichaelthomas.github.io/guion/`). The user says it is not live yet but close.

## 2. What the checkpoints and research settled

- **Audience** (Guion north-star v0.2): builders, and the assistants who advise them. The project's own goal is "earning assistant recommendations." The page serves both.
- **Plain-language standard** (Ptah naming-language addendum, 2026-03-24): entry surfaces must not require a developer background; technical precision sits underneath, it is not removed. Plain English is the default view, Developer adds precision.
- **Native first** (AllHere spec): visitor pages ship with no JavaScript, no remote requests, no remote fonts. The only exception is an explicit, visitor-started capability. The playground is that exception (section 8).
- **Pre-release honesty** (Guion changelog: "No public Guion release has been published"): no download or install button. Pre-release is stated plainly.
- **Planes lessons** (`planes/docs/superpowers/specs/2026-08-01-index-as-hub-design.md`): derive the deployed set from the directory tree, not an allowlist; a served page must be linked; mockups live outside the served directory; a hosted "try it" page with no build step works.
- **Language homepage survey** (fetched 2026-10-27): python.org shows static `>>>` transcripts, a link to an interactive shell, and no performance claim at all. julialang.org links "Fast" to a benchmarks page, shows no figures on the home page, has no playground. go.dev has an embedded Try Go editor with samples, and performance appears only as testimonials. ziglang.org and gleam.run make no measured claims; Gleam hides its playground in the footer. Python's performance evidence lives elsewhere (speed.python.org, pyperformance). **None of them puts a method, a named workload, raw samples, or a "when to choose something else" next to a number on the front page.** That is the opening.
- **TAOS** (obligation-layer inception checkpoint, 2026-08-24): a promise has a debtor, a beneficiary, a condition, a lifecycle, and a channel for the beneficiary to object. The page is built that way (section 6) without ever naming the theory.

## 3. Goals, non-goals, anti-goals

Goals
- A person can run a Guion program in the browser within 30 seconds of arriving.
- A non-programmer can say what Guion is for and what it cannot yet do after reading only the Plain view.
- A developer can find the command, signature, report kind, limit, or evidence file for every claim in the Developer view.
- Every number on the page can be checked: it names its workload, host, and scope, and points at a published file.
- All 10 instrument families are used in the design where they earn their place. Which pieces went where is recorded for agents in `site/agents/instrument-map.md`, not shown to visitors.
- Agents can build, extend, and verify the site from `site/agents/` alone.

Non-goals
- Not a download or install page (no public release).
- Not a docs site. Depth stays in the repo docs; the page links to published evidence files, not to private repo URLs.
- Not forcing all 100 instruments onto one page, and not a section that explains or advertises AllHereHTML.

Anti-goals (a failure even if it technically works)
- A speed claim without a scope, or any claim that Guion generally replaces Python.
- Generic SaaS landing furniture: logo walls, testimonials, gradient blobs, feature-card triplets with icons.
- A toggle that only hides some paragraphs. The two views must feel like two different pages.
- Remote fonts, trackers, or any request leaving the origin.

## 4. Repository layout and publishing

```
site/
  README.md              how the site is built and checked
  agents/                documents for agents (not deployed)
  src/                   page shell, sections, content data (not deployed)
  tools/                 build and check scripts, Node only (not deployed)
  vendor/allhere/        snapshot of instrument CSS and markup + PROVENANCE.md (not deployed)
  public/                THE DEPLOYED DIRECTORY
    index.html
    playground/          loader.mjs, compiler and VM .wasm, samples
    evidence/            index.html (a plain, generated list) plus the report and raw-sample files that claims cite, each with SHA-256
    llms.txt, agents.json
    .nojekyll
.github/workflows/pages.yml   uploads site/public only
```

- **A file is public only if it is under `site/public/`.** A test asserts every file there is linked from `index.html` or is a declared asset (the Planes invariant).
- All paths in `public/` are relative: the site lives under `/guion/`.
- `pages.yml` runs on pushes to `main` that touch `site/**`, uploads `site/public` with the standard Pages artifact actions, and deploys. It needs Pages "Source" set to **GitHub Actions**. That is a repository settings change. I will not make it without your go-ahead; until it is made, legacy Pages keeps serving the whole repository tree publicly. The workflow only needs Actions to run at publish time. The existing Rust CI is unaffected.
- `site/public/` is generated: `site/tools/build.mjs` fills `site/src` into it, and the output is committed so deploys do not need Node. CI (when available) rebuilds and diffs.
- The frozen application `evaluation/completion/applications/allherehtml/` and its CI check are not touched.

## 5. The page

One long page with a sticky chapter rail, a visible view switch, and a print stylesheet that turns it into a field-guide booklet. Visual identity comes from the user's mockup: paper `#e9ede3`, ink green `#11251d`, blue `#2148e8`, lily `#d9f987`, orange `#fa784e`, system sans plus a Georgia accent, the tilted "g" brandmark. The orb illustration can stay if it earns meaning; otherwise it is replaced. Instrument names below are from the registry; the builder confirms each renders cleanly before using it and swaps within the same family if not.

| # | Section | Plain view says | Developer view adds | Instruments |
|---|---|---|---|---|
| 0 | Status bar and nav | "Pre-release. Not downloadable yet." | build/ABI/artifact-format badge | Destination marker, Chapter rail, Return trail |
| 1 | Hero | "guion." and "A language for the work ahead." One honest sentence. | The three-line truth: typed, self-hosted compiler, Rust runtime | Kinetic headline, SVG illustration, Expressive caption |
| 2 | **Try it** | Editor, Run, three starter programs, a sentence on what just happened | Step count, artifact bytes, limits, error codes, compile and run split | Executable code laboratory, Practice prompt, Guided tour |
| 3 | Why another language | Three plain reasons (readable, one toolchain, claims that hold up) | The exact design choices: typed boundaries, inferred locals, Option/Result | Pull quote, Marginalia, Context definition, Inline footnote, Marker highlight |
| 4 | One command, whole job | The path from idea to something you can hand over | The real subcommands with their flags and `--json` report kinds (counted from `crates/cli` at build time, not hard-coded) | Flow map, Story trail, Step card, Reference sheet |
| 5 | Read it, then change it | Real programs with the output beside them | `.gu` source with signatures and artifact opcodes | Tabbed scene, Annotated example, Curtain reveal, Image hotspot |
| 6 | What a program may do | "A program says what it wants before it runs, and you decide." | `guion inspect --json`: `required_effects`, `manifest_grants`, `grant_assessment`, `present_but_target_dynamic` | Layered peek, Exploded diagram, Hidden chamber, Spotlight |
| 7 | **Receipts** | Each claim as a card: what we say, when it holds, what it does not tell you, check it, say it's wrong | Medians, intervals, sample counts, host, format number, file path, SHA-256 | Case dossier, Comparison bridge, Data explorer, Evidence bracket, Source trail, Provenance trail, Underlined claim |
| 8 | Where Python still wins | A short, direct list | The NumPy/orjson bridge numbers and the older recommendation studies, labelled historical | Accordion, Cause-and-effect ladder, Interactive quiz ("which fits this job?") |
| 9 | Made for agents too | "Your assistant can read this page, and the project, without guessing." | `agents.json`, `docs/agents/index.json` shape, `--json` envelopes | Programmable instrument, Source explanation, Clue trail |
| 10 | How it got here | A short true timeline, including that it was called Veritas until 2026-10-25 | Checkpoint chain and commit anchors | Time ribbon, Scroll chapter, Reading checkpoint, Guided bookmark |
| 11 | Footer | Status, license (MIT), third-party notices, contact, one line: "Made with AllHereHTML" | Page weight report: HTML and CSS bytes, JS bytes, Wasm bytes (loaded on Run), remote requests (0) | Exhibition label, Printable booklet (the print stylesheet) |

Families touched across the page: typography, annotation, relationships, reveals, motion, navigation, visual art/media, teaching, publications, computation.

Motion is decorative and optional: every effect respects `prefers-reduced-motion`, and every instrument keeps its documented fallback.

## 6. Receipts: how claims are built (undertone, never named on the page)

Every claim is a card with five fixed parts. The page's vocabulary stays ordinary: **Claim**, **Holds when**, **Doesn't tell you**, **Check it**, **Say it's wrong**.

- Claim: one plain sentence, one number.
- Holds when: the named workload, host, format, date. Never broader than the report.
- Doesn't tell you: the limit, in the same size type as the claim.
- Check it: the command to reproduce and the published file (`public/evidence/...`, with SHA-256). A visitor with no repository access can still verify.
- Say it's wrong: a `mailto:` link to `rmichael@rmichaelthomas.com` (given by the user for this purpose) with the claim's name pre-filled in the subject. The address is public on the page.

The same shape governs section 6: the program's grants are promises to whoever runs it, with the condition stated and the check shown.

Figures start from `README.md` and `docs/status.md` as of 2026-10-27 (e.g. Richards uncached 27.46 ms vs 72.92 ms, ratio 0.375; 127/127 application cases; 9 applications, ratios 0.137 to 0.845; 54 of 54 target reports; 200,000-row JSONL 3.81 s vs 17.93 s; lockfile auditor to 16,384 packages). The old mockup numbers (195 cases, 36/36, 4.025 s) are superseded. `site/tools/check-claims.mjs` fails the build if a figure on the page is not found in the file its card cites.

Shown with equal weight: bridge results where Guion is slower (1.025× and 1.129× slower than the same Python function on the NumPy/orjson task), the unmeasured free-threaded Python comparison, and the historical studies where an assistant chose Python in 18 of 18 cases for the older candidate. Targets are stated exactly as `docs/status.md` states them: an earlier cohort passed on macOS ARM64 and Debian 12 on ARM64 and x86_64 (all glibc); the current format-44 performance checkpoint ran on macOS ARM64 only. Nothing else is claimed.

## 7. The two views

A native radio pair (`Plain English` | `Developer`) in the sticky bar. No script: the page uses `:has(#view-dev:checked)`. **Deep link (changed during the build):** `https://rmichaelthomas.github.io/guion/developer/` is a second generated page with the Developer radio pre-checked (and `noindex`). A `#developer` target was dropped because it cannot be overridden once the visitor clicks Plain English, which would leave them stuck. The setting is not remembered across visits; remembering needs script, and I am not adding it. The switch is a real `<fieldset>` and is keyboard operable.

Plain is the default, written to the Ptah test adapted for this project: a person who has never programmed can say what Guion is for, and what it cannot yet do.

Developer changes the page's structure, not just visibility:
- type scale and measure tighten; mono labels and a visible 12-column grid appear;
- marginalia open into the margin;
- code gets signatures, effect annotations, error codes, and artifact details;
- claim cards grow a data table (median, interval, pairs, host, format, file, hash);
- the chapter rail becomes a command index.

Both views are authored side by side in the source; neither is derived by stripping the other.

## 8. The playground

The landing page embeds it (section 2). Two Wasm modules in the browser: the Guion compiler adapter produces `.gub` bytes from one source file, and the shared VM runs them. Output, step count, and errors appear beside the editor.

- Zero host capabilities. Reading a file, calling the network, or reading the clock returns the stable `R_HOST_UNSUPPORTED`, and the page explains it with a ready-made "try it" sample. That is the permissions story (section 6) shown, not described.
- Limits shown, not hidden: source size, step budget, output size, allocation, with the real numbers from the module.
- It is visitor-started. The page contains a small module that does nothing until the visitor presses Run; the Wasm files are fetched on that press. The page reports its own JavaScript bytes, Wasm bytes, and remote requests (zero).
- With JavaScript off, the editor is a readable transcript with documented outputs and a clear notice.
- Source of the Wasm: built from this repository's `guion-compiler`, `guion-model`, `guion-vm-core` and `guion-vm-wasm` crates by a script under `site/tools/` (adapted from AllHereHTML's `wasm-compiler` and `scripts/build-*.sh`, which build against this repo). Prebuilt binaries are committed with SHA-256 and the exact toolchain version. A check rebuilds and compares when CI is available.
- This is the first real-browser-engine run of the typed ABI path that the AllHere checkpoint lists as unverified; the verification plan records that result honestly either way.

## 9. Using AllHereHTML

`site/tools/vendor-instruments.mjs` renders the chosen instruments by calling AllHere's own `createInstrument` and `renderVisitorDocument` (the same path the flagship builder uses) from a sibling checkout (`ALLHERE_REPO`, default `../allherehtml`), then writes fragments, family CSS, and `PROVENANCE.md` (AllHere commit, registry hash, per-file SHA-256) into `site/vendor/allhere/`. The page shell in `site/src` places fragments with slots such as `{{instrument:ahi.relationships-diagrams.flow-map#lifecycle}}`. This keeps the page hand-composed while every instrument is a registry-checked native piece. The vendored snapshot is committed, so the site builds without the AllHere repo.

`site/agents/` documents the toolkit so an agent can find a piece, read its behavior contract and fallback, render it, and check it. A short pointer is added in AllHereHTML (`START_HERE.md` and `README.md`) on its own branch, `docs/guion-site-pointer`, left for review; AllHereHTML is local-only with no remote.

## 10. Agent documents

In `site/agents/`: `README.md` (start here), `site-brief.md` (purpose, audience, voice, claims policy), `allhere-toolkit.md` (registry, renderers, Workshop, exporter, commands), `instrument-map.md` (generated: section to instrument IDs), `content-sources.md` (every figure to its source file and how to refresh it), `build-and-verify.md` (commands, local serving, QA matrix, deploy), `extending.md` (add a section, an instrument, a claim; lessons from Planes), and `index.json` (machine index in the shape of `docs/agents/index.json`). I will not create a root `AGENTS.md` or `CLAUDE.md`.

Public agent surfaces (deployed): `public/llms.txt` and `public/agents.json` carry the claims with their conditions and evidence URLs, so an assistant recommending Guion does so with the caveats attached.

## 11. Quality bar and verification

Tests and checks (all runnable from the repo with `node` and `python3`):
- build is deterministic: rebuilding produces no diff;
- every `public/` file is linked or a declared asset; every in-page anchor resolves; no absolute paths;
- every figure traces to its cited source (`check-claims`);
- the page makes zero remote requests, has the declared script set only, and the report lists JS/Wasm bytes;
- instrument coverage: all 10 families used, and `instrument-map.md` matches the pieces actually present in the built page.

Browser checks with Playwright (Chromium; Firefox and WebKit when available, otherwise marked pending, not assumed): view switch by mouse and keyboard; `#developer` deep link; JS-off render; playground run, edit, error, host-denial, and step-budget cases; mobile 360 px, tablet, desktop, 200% and 400% zoom; reduced motion; forced colors; print; accessibility audit with no critical findings. Screenshots of both views at desktop and mobile are kept in `site/qa/` with the results, including failures.

Size: target ≤ 400 KB of HTML and CSS uncompressed, plus about 1.3 MB of Wasm (532 KB compiler adapter + 809 KB ABI 1.0 VM) fetched only on Run. Largest contentful content visible without script.

## 12. Calls I made, and what I need from you

Calls I made (change any):
1. One long landing page plus a plain generated evidence index. Depth is the evidence files and the repo docs.
2. Plain is the default view; the setting does not persist (no script).
3. Evidence reports and raw samples cited by claims are copied into `public/evidence/` so "Check it" works for a visitor without repository access. Precedent: julialang.org/benchmarks publishes its chart with the machine, tool versions, raw CSV, source code and a "not comprehensive, a bit dated" caveat; python.org keeps benchmarks on a separate Speed Center rather than its front page. Neither puts them on the front page, and neither is a reason to hide them: ours sit on a linked evidence page. You will see the exact file list before anything is published.
4. Prebuilt Wasm is committed with checksums, instead of being built inside the deploy workflow.
5. No remote or self-hosted web fonts; system stacks with a Georgia accent, as in the mockup.
6. No instrument-coverage display for visitors (your correction); coverage lives in `site/agents/instrument-map.md`.
7. The Spanish word "guion" (screenplay, script) is **not** used as a design motif or claimed as the name's origin, because no project document says so.
8. Light "paper" appearance only, plus dark sections as in the mockup; no OS-driven dark mode (Planes made the same call).

Settled by you:
- Contact for "Say it's wrong": `rmichael@rmichaelthomas.com`.
- Go-ahead to switch Pages to GitHub Actions is given. I will make the change when the workflow is on `main`, so there is no gap between the old and new deployment, and I will report the result.

Still open:
- How far to take the TAOS undertone. I read the obligation-layer checkpoint only. This design uses its structure (condition, check, objection) and nothing else.

## 13. Order of work (one release, not slices)

1. Vendor tool and PROVENANCE; render and inspect every candidate instrument.
2. Wasm build script, playground loader, verification of the typed ABI in a real browser.
3. Page shell, both views, content data with claim sources, `check-claims`.
4. Sections 0 to 11 composed and checked at each breakpoint.
5. Agent documents, `llms.txt`, `agents.json`, AllHere pointer branch.
6. Full QA matrix, fixes, then the Pages workflow. The Pages source switch (authorized) happens when the workflow is on `main`.
