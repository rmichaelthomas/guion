# guion_checkpoint_v1_50_ecosystem_direction.md

# CANONICAL CHECKPOINT DOCUMENT
## Guion
### v1.50 — ecosystem direction: growth since v1.49, the goal restated, and a Guion-native ecosystem roadmap

**Status:** LOCKED — EXTENDS Veritas checkpoint v1.49
**Date:** November 3, 2026
**Author:** Rob Thomas / R. Michael Thomas (architect) and Claude (analytical partner)
**Domain prefix:** guion (provisional, pre-vault). It replaces `veritas` after the October 25, 2026 rename. Version numbering continues the Veritas series.
**Session type:** A verification checkpoint (growth since v1.49) combined with a direction-setting session (ecosystem and goal)
**Relationship to prior checkpoints:** Direct continuation of [Veritas checkpoint v1.49: compiler](veritas_checkpoint_v1_49_compiler.md) (October 24, 2026; committed `cb5587b`, October 25). All decisions from v1.1 through v1.49 remain in force except where Part VI below changes them. Part/§ numbering continues from v1.49, which ended at Part III / §5 with one open question.

> *"When I mean general purpose programming language, I fucking mean it."*: the architect, this session

The session opened with a question about concepts: what separates libraries, packages, and an ecosystem from a public release, and how should Guion use these to compete with Python under its north star? The first answer read the north star through the written v0.2 design document. That document treats a managed CPython sidecar as Guion's way to reach libraries, defers registries "as evidence requires," and asks the first release to name the tasks where Python stays better. The answer therefore recommended making the Python bridge the ecosystem fallback and holding off on a registry.

That reading mixed up two things: a shortcut the first release took, and the goal itself. The architect corrected it. The goal is for Guion to replace Python as a general-purpose language, with its own wide standard library, its own ecosystem, and its own releases, without depending on Python. A topic the architect raises is no longer deferred, and early plans don't bind the architect. The real gap was that the governing design document described something narrower than the architect had been saying all along, and the sprints that followed it kept optimizing a bridge the architect never meant as the foundation. This checkpoint does three things: it records how much Guion has grown since v1.49, locks the restated direction, and points to the new design document that holds the roadmap.

## HOW TO READ THIS DOCUMENT

This is Guion checkpoint v1.50. Part IV verifies live repository state at `e70617a` against the last checkpoint, v1.49. Parts V–VIII lock the restated goal, freeze the Python bridge, and adopt the ecosystem architecture and roadmap. The full detail of that architecture and roadmap lives in [`north-star-design-v0-3.md`](north-star-design-v0-3.md), which supersedes v0.2. v0.2 is kept unchanged with a pointer added at the top.

No code changed this session. Package-system mechanics, standard-library module APIs, and array design are deliberately left unlocked; steps 2–4 decide them.

## PART IV — GROWTH SINCE v1.49

**Decision: The verified repository state at `e70617a` is the baseline for the ecosystem roadmap. LOCKED.**

### §6 — Product completion and the rename

At v1.49, Task 3 of the complete-product plan was done and Tasks 4–11 remained. Since then:
- [`completion-report.md`](completion-report.md) records the complete-product plan as implemented for its declared scope. The core CLI embeds the self-hosted compiler. The standard library covers files, CSV/JSONL, text, collections, JSON, date/time, SemVer, TOML, SQLite, Unicode, HMAC, and HTTP. Nine applications run in initial and follow-up forms. Artifact format was 39.
- On October 25 the product was renamed from Veritas to Guion (`6a4fab1`). Source, artifact, manifest, and lock names became `.gu`, `.gub`, `guion.toml`, and `guion.lock`, and historical Veritas records were preserved.
- The three-target release matrix (macOS ARM64, Debian 12 ARM64, Debian 12 x86_64, all glibc) passed 54 of 54 application reports ([`pre-release-readiness.md`](pre-release-readiness.md)).
- All 32 pull requests (#1–#32) merged between October 25 and November 3 (verified with `gh pr list --state all`). The repository has 306 commits since October 25.

### §7 — Runtime and speed

- PR #6 split out `guion-vm-core` and added a `guion-vm-wasm` adapter that shares the production VM.
- Several sprints (PRs #1–#5, #7, #9, #24, #25, #27–#29, #31) produced these engine changes:
  - a source-run cache
  - register-form execution
  - hashed maps and 24-byte values
  - profile-guided optimization (PGO) for release builds
  - plain reference counts for values that stay on one thread
  - short-circuit threading
  - fused map updates
- **Server speed gate (November 3, 2026).** Guion took less time than uv's standalone CPython 3.14.7 on every text, JSONL, and CSV lane, at 20,000 and 200,000 rows, both from source and from artifacts. This held on x86_64 (AMD EPYC 7763; worst upper bound 0.976) and ARM64 (Neoverse N2; worst upper bound 0.859). See the [server speed-gate report](../evaluation/completion/results/real-task-sprint/2026-11-03-server-speed-gate.md).
- Richards measured a 0.375 ratio against CPython 3.14.7 on macOS ARM64 (current-main checkpoint, October 27).
- Artifact format moved from 34 (at v1.49) to 48. The runtime reads formats 14–48 ([`compatibility-and-migrations.md`](compatibility-and-migrations.md)).

### §8 — Packages, targets, release

- **Native package foundation, verified present** at `e70617a` in `crates/cli/src/native_packages.rs`:
  - `guion add NAME --path` and `guion add NAME --url --version --sha256`
  - `guion update`
  - `guion package --library`
  - library manifests with exports and transitive dependencies
  - lock format 2 with content SHA-256
  - the `.guion/packages` cache
  - `pkg:` imports that resolve only through the lock
- **Verified absent** at `e70617a`:
  - Version ranges. Dependencies must give an exact version (`requirement_version`, `validate_exact_version`).
  - A `publish` command (no such branch in `crates/cli/src/main.rs`).
  - A registry (no registry code in `native_packages.rs`).
  - Dependency identities in artifact metadata (`docs/reference.md` lists them as not yet recorded).
- Managed application bundles passed on macOS ARM64 and Debian ARM64/x86_64 for the NumPy/orjson analysis and Feed Digest fixtures.
- `v0.1.0-preview` was published November 3, 2026 as a GitHub pre-release with PGO Linux archives for Debian ARM64 and x86_64. No macOS package is attached (verified with `gh release view v0.1.0-preview`).
- The repository is public (verified with `gh repo view`).

### §9 — The Python bridge record

PRs #13–#18 and #21 worked on the Python bridge between October 27 and 28. That work included raw-byte attachments, copy reductions, a `readinto`-based frame reader, and native-image snapshot gating, which cut the 1,000-call ratio from 4.131 to 1.481. The bridge stayed slower than direct Python in its latest cohorts ([`status.md`](status.md)). Part VI below freezes this record.

### §10 — Public presence and CI

- PRs #22, #23, and #32 built the public site (`site/`, served from `gh-pages`). It has a plain-English view and a developer view, plus the namesake and pronunciation (`docs/namesake.md`).
- PR #26 limited pull-request CI to cheap checks and made the release matrices run only on demand, because of the Actions budget.

## PART V — THE GOAL, RESTATED

**Decision: Guion's goal is to replace Python as a general-purpose language: better than Python across the range Python covers, with its own wide standard library, package ecosystem, registry, and public releases. Assistants recommending Guion is one sign of progress, not the ceiling. Anything Python can do that Guion can't yet do is a work item, not a scope boundary. LOCKED.**

### §11 — What the correction changes

The architect stated this goal throughout the build. The v0.2 document's narrower framing was an early plan, not a restriction the architect chose. From here on, plans don't treat Guion as specialized or limited to task classes, and they don't treat it as subordinate to Python. Raising a topic for discussion ends its deferral. Honest measurement against Python continues, now as a comparison with a competitor.

## PART VI — PYTHON BRIDGE FROZEN

**Decision: The `python-interop` feature is frozen at its current state: optional, off in the core build, and with existing tests kept passing. No further optimization or feature work, and bridge results are not cited as ecosystem or speed evidence. Libraries come from Guion's own standard library and packages. LOCKED.**

### §12 — What this supersedes

This replaces v0.2's "Python compatibility and dependencies" strategy and the bridge performance targets in `status.md` ("response transport and reader wakeup are the next target"). Removing the feature, or keeping it as a porting aid, is left open (question 10).

## PART VII — GUION-NATIVE ECOSYSTEM ARCHITECTURE

**Decision: Guion's ecosystem has five parts: a wide native standard library versioned with the compiler; native arrays and numerics; a package system with version ranges, publishing, and a hosted registry, with per-package capability declarations; first-party packages; and public releases under a written stability policy. Architecture as specified in `north-star-design-v0-3.md`. LOCKED. Mechanics are not locked.**

### §13 — Standard library

The scope rule: stable protocols, file formats, and OS services go in the standard library; fast-changing domains become first-party packages.
- **Wave 1:** processes and subprocesses, paths, environment, signals, the full filesystem API, temp files; regex, base64/hex, URLs, encodings; HTTP server, TCP/UDP, TLS, WebSockets.
- **Wave 2:** decimal, math/statistics, random numbers; gzip, zip, tar; YAML, XML/HTML; logging; async I/O.

Wave 1's domains are verified absent at `e70617a`. Neither `language/builtin-signatures.json` nor `language/stdlib/*.gu` matches regex, subprocess, serve, socket, base64, gzip, or random.

### §14 — Arrays and numerics

Typed n-dimensional arrays with fixed-width element types, vectorized native kernels, views that avoid copying, and parallel kernels on the bounded workers, with data frames after. Benchmarks are against NumPy and pandas.

### §15 — Packages and registry

This builds on the §8 foundation:
1. version ranges and a resolver
2. `guion new --lib` and public-API checks
3. `guion publish`, with versions that can be yanked but never replaced
4. a registry: index, content-addressed storage, name ownership, authenticated publishers, a public log of each version's content hash
5. capability declarations checked by static capability inspection, recorded in the lock, and requiring approval when an update asks for more
6. dependency identities recorded in artifacts

### §16 — First-party packages and release

First-party candidates: a web framework, data frames, a CLI toolkit, database drivers. Release 0.2.0 has installers for all three validated targets, a stability policy, and the registry live.

## PART VIII — ROADMAP AND DESIGN DOCUMENT v0.3

**Decision: `docs/north-star-design-v0-3.md` builds on and supersedes v0.2 and holds the roadmap. Steps in order: (1) the design document and this checkpoint, done; (2) package system and registry in parallel with standard library Wave 1; (3) native arrays and numerics, then data frames; (4) public 0.2.0 release with the registry and first first-party packages; (5) further waves on a regular release schedule. LOCKED.**

### §17 — Pause point

The architect chose to pause after step 1. Work resumes at step 2 using the prompt below.

---

## WHAT IS LOCKED

- Repository state at `e70617a`, as verified in Part IV, is the baseline.
- The goal: replace Python as a general-purpose language with a Guion-native ecosystem (Part V).
- The Python bridge is frozen, with no further investment (Part VI).
- The five-part ecosystem architecture (Part VII).
- `north-star-design-v0-3.md` supersedes v0.2. The roadmap is in steps 1–5, and step 1 is complete (Part VIII).

## WHAT IS NOT LOCKED

- Package-system mechanics: the resolver algorithm, the manifest syntax for version ranges, the registry protocol and hosting, publisher authentication, and whether packages are distributed as source or as artifacts.
- The capability-declaration format and the approval UX.
- API design for each Wave 1 standard-library module.
- Array element types, integer widths, and the boundary between the runtime and the standard library.
- What the stability policy promises, and the release cadence.
- The long-term fate of `python-interop`.

## WHAT IS LOGGED

- The v0.2 document's narrower framing shaped about two days of bridge-optimization sprints (PRs #13–#18, #21). That record is kept, not re-labeled.
- README, `status.md`, and the public site still link to v0.2. The banner at the top of v0.2 sends readers to v0.3. A docs-propagation pass should update those links when work resumes.
- The architect's correction is also saved to Claude's project memory (`project-guion-goal-replace-python`).
- This session's three files were committed directly to `main`, with no PR, at the architect's direction.

---

## UPDATED OPEN QUESTIONS (v1.50 status)

| # | Question | Status |
|---|---|---|
| 1 | Do G1, G2, and G3 pass the final product gates? | Resolved in substance by `completion-report.md` (October 25, 2026), which records the complete-product plan as implemented for its declared scope. That report does not use the G1/G2/G3 labels. |
| 2 | Where is the registry hosted, and how are publishers authenticated (including whether signing is required)? | New |
| 3 | Are library packages published as source compiled by the consumer, or as `.gub` artifacts? | New |
| 4 | Which array element types and integer widths exist, and how do they relate to arbitrary-precision `Int`? | New |
| 5 | Exactly what does the 0.x stability policy promise for source, artifact, and package formats? | New |
| 6 | Package names: flat or namespaced, ownership and dispute policy, reserved names? | New |
| 7 | How does the macOS release ship (archive, installer, notarization)? | New |
| 8 | Where does each module fall between the standard library and first-party packages? | New |
| 9 | What async I/O model fits bounded structured parallel groups? | New |
| 10 | Is `python-interop` kept frozen, kept as a porting aid, or removed? | New |

---

## DOCUMENTS PRODUCED THIS SESSION

| Document | Type | Status |
|---|---|---|
| `docs/guion_checkpoint_v1_50_ecosystem_direction.md` (this document) | Checkpoint | Complete, LOCKED |
| `docs/north-star-design-v0-3.md` | Design document (supersedes v0.2) | Complete, approved direction |
| `docs/north-star-design.md` | v0.2 design document | Unchanged except for a superseded banner at the top |

---

## RESUME PROMPT (v1.50)

*We are resuming Guion from checkpoint v1.50 (November 3, 2026), `docs/guion_checkpoint_v1_50_ecosystem_direction.md`, together with `docs/north-star-design-v0-3.md`, which supersedes the v0.2 design. All decisions from Veritas v1.1 through v1.49 remain in force except where v1.50 changes them. The goal is locked: Guion replaces Python as a general-purpose language with its own wide standard library, package ecosystem, registry, and public releases. It is not limited to task classes and not subordinate to Python. Any capability gap with Python is a work item. The `python-interop` bridge is frozen, with no further work, and is not cited as evidence. Baseline is `e70617a` on `main`: artifact format 48, with the runtime reading formats 14–48. The server speed gate beat CPython 3.14.7 on all text, JSONL, and CSV lanes on x86_64 and ARM64. `v0.1.0-preview` is a Linux-only GitHub pre-release with no macOS package. The repository is public. Native package foundation, verified present in `crates/cli/src/native_packages.rs`: `guion add --path` and `--url --version --sha256`, `guion update`, `guion package --library`, transitive library manifests, lock format 2 with content hashes, the `.guion/packages` cache, and lock-only `pkg:` imports. Verified absent at `e70617a`: version ranges (exact versions only), a resolver, `guion publish`, a registry, and package identities in artifacts. Also verified absent: stdlib regex, subprocess, sockets, an HTTP server, base64, gzip, and random. Seventeen stdlib modules exist. Roadmap step 1 (design document and checkpoint) is done. Resume with steps 2–4. **Step 2** runs two parallel tracks. Track A is the package system: version ranges and resolver, `guion new --lib`, `guion publish`, a registry with index, content-addressed storage, name ownership and authenticated publishers, per-package capability declarations checked by static capability inspection and recorded in the lock, and package identities in artifacts. Track B is standard library Wave 1: processes, paths, environment, signals, the full fs API, temp files, regex, base64/hex, URL, encodings, HTTP server, TCP/UDP, TLS, WebSockets, implemented natively behind typed host operations with conformance cases, `guion doc` output, capability declarations, and Python comparisons. **Step 3** is native typed n-dimensional arrays with fixed-width elements, vectorized and parallel kernels, and views that avoid copying, benchmarked against NumPy, then data frames benchmarked against pandas. **Step 4** is the public 0.2.0 release: installers for macOS ARM64 and Debian 12 ARM64/x86_64, a written stability policy, the registry live, and the first first-party packages (a web framework, data frames, a CLI toolkit, database drivers). Nine open questions are new (2–10), and question 1 is resolved. Questions 2, 3, 5, and 6 must be decided before Track A builds the registry and publish. Question 4 must be decided before step 3, and question 7 before step 4. Open the next session by settling questions 2, 3, 5, and 6 with a decision walkthrough, then write the Track A and Track B build plans. Build all of a multi-stage plan in one pass, committing between stages. Measure locally, ask before running any GitHub workflow, and report results honestly. The architect paused after step 1. The v0.3 design document and this checkpoint are committed on `main`.*
