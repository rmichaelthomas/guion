# Guion migration and pre-release implementation plan

> **For agentic workers:** Execute these tasks in order after Rob approves this plan. Record completion in checkboxes and preserve the source checkout until the new repository has passed the final gate.

**Goal:** Move the completed Veritas build into `~/guion` as Guion, preserve its history and evidence, make the language easy for people and agents to learn, and leave a clean private repository ready for a separate pre-release decision.

**Architecture:** First save the two small current release archives and compact reports, then reclaim generated disk space. Copy the lean Git repository into the empty destination and make the name change in a reviewable series of commits. Keep historical Veritas reports clearly labeled; make Guion the name of the current product. Use the existing conformance, bootstrap, release, and evaluation machinery as the source of truth rather than rewriting conclusions.

**Tech stack:** Rust/Cargo workspace; self-hosted language source and bytecode; JSON/TOML contracts; Git and the private GitHub repository `rmichaelthomas/guion`.

**Source documents:** `docs/superpowers/plans/2026-10-24-veritas-complete-product.md`, `docs/north-star-design.md`, `docs/completion-report.md`, `language/spec.md`, and `evaluation/completion/README.md` in the current Veritas checkout.

**Size budget:** The ordinary Git checkout should contain source, documentation, fixtures, and compact text/JSON results only. Aim for under 25 MiB of files in the current tracked tree and under 50 MiB of compressed Git history; stop and review if either budget is exceeded. Keep any single ordinary Git file below 50 MiB. The final core install should stay in the same order of magnitude as the measured 5.2–5.6 MiB compressed release archives and 15–17 MiB extracted releases, with changes explained by the release report. These are review thresholds, not substitutes for correctness.

## Verified starting point

- Source checkout: `/Users/rmichaelthomas/Documents/Codex/2026-10-22/referenced-chatgpt-conversation-this-is-an/work/veritas-foundation`, branch `veritas/north-star-sprint`, HEAD `3ece1ce19a5d2aa8443491ab58c39873a45af1a9`. It has no configured remote and has uncommitted changes plus substantial untracked evidence.
- Destination: `/Users/rmichaelthomas/guion` exists and is empty. GitHub `rmichaelthomas/guion` exists, is private, and reports an empty repository with `main` as its default branch.
- Source size is about 19 GB: `target/` about 15 GB, `evaluation/` about 3.5 GB, `work/` about 142 MB. The Git history is about 27 MB and tracks 1,283 files. These are inventory measurements, not proposed deletion amounts.
- The current tracked working tree is about 147 MiB; about 135 MiB sits in builder evidence, primarily eleven repeated copies of a roughly 12 MiB runtime binary. The ordinary source/docs/tests and compact reports are much smaller. The current core release archives are 5.2 MiB (macOS ARM64) and 5.6 MiB (Debian ARM64), with extracted releases about 15 and 17 MiB respectively.
- The untracked `evaluation/results/independent-v3/` campaign is about 979 MB and contains unique reports and provenance. The ignored `independent-v2/workspace/` and `run-evidence/` contain pinned inputs and a compressed evidence archive; the campaign README explicitly says to retain them. Do not classify these as disposable because they are untracked or ignored.
- `crates/cli/core` is a 13 MB crash core. Nine application `dist/` folders total about 395 MB. The extracted `native/vendor/libical-4.0.5/` tree is about 22 MB; its source tarball is tracked. These are cleanup candidates only after the provenance check below.
- The historical independent-v3 record says 18 of 18 blinded choices favored Python for the older format-33 candidate. The later completion report records three fresh matched builder classes as `either`, `veritas`, `either`, and wins on specified installed compute/data workloads against pinned CPython. Preserve both findings with their dates and product identities; neither becomes a general claim that Guion has replaced Python.
- Process inspection was blocked by this environment, so no claim is made that a build, benchmark, container, or server is running or stopped.

## Proposed decisions for Rob's approval

1. **Home and history:** `~/guion` becomes the canonical local checkout. Preserve the existing Git ancestry; mark the last Veritas state with an archival tag before Guion rename commits. Keep the old checkout intact until the new private remote and local verification agree.
2. **Public identity:** Use `Guion` in current copy, `guion` for the command and Rust packages, `guion.toml` / `guion.lock` for project files, `.gu` for new source, `.gub` for new bytecode, and `.guion/` for local project state. Historical Veritas evidence retains its original names, bytes, and hashes. The transition is pre-1.0; any temporary legacy readers must be explicit and tested.
3. **License and visibility:** Retain the workspace's declared MIT license, add its missing root license text and third-party notice index, and keep GitHub private through pre-release preparation. Licensing and public release remain separate decisions.
4. **Agent guidance:** Add ordinary repository documentation and machine-readable indexes. This plan does **not** create or edit a project-level `AGENTS.md`; the session instruction requires separate, specific approval for that file.
5. **Evidence retention:** Keep checkpoint documents, source, the final working source version of each representative application, **all scored outcomes including failures**, compact JSON/text reports, manifests, and hashes. Reclaim space by deleting redundant build products, superseded packaged versions, old workspaces, and bulky raw session archives after a file-by-file inventory. This gives up exact replay of some old evaluation sessions; the recorded outcomes and methodology remain. If full replay is later required, the study must be rerun or its raw artifacts restored from another backup if one exists.

## Execution order

### Task 1 — Freeze and inventory the old checkout

- [x] Record HEAD, branch, tracked changes, untracked and ignored files, file sizes, current hashes, and the current completion/report status in `docs/history/migration-2026-10-25.md` in the new repo.
- [x] Identify every file cited by a report, hash manifest, or campaign lock. Classify each item as source, historical record, pinned input, release artifact, generated cache, or redundant copy. Scan prospective Git content for credentials and private user data before any push.
- [x] Inspect process ownership and working directories with an available process tool. Stop only Guion/Veritas builds, benchmarks, local servers, or containers confirmed to belong to this checkout; record what was stopped. If inspection remains blocked, report that explicitly and do not use a blanket kill command.
- [x] **Gate:** A preservation manifest names every large candidate for removal and the evidence that makes it safe or unsafe to remove.

### Task 2 — Preserve the small, irreplaceable record

- [x] Save the four modified tracked files and every intentional untracked source or result, especially independent-v3, before cleanup. Distinguish uncommitted repairs from generated outputs; commit intentional repairs with an accurate message after review.
- [x] Copy the two current core release archives (about 11 MiB together) out of `target/`, verify SHA-256, and record their source commit and target OS. Save any unique small release/acceptance logs needed to interpret the final reports.
- [x] Create a retention manifest for the frozen v1/v2/v3 campaigns and completion work. Keep compact text/JSON reports for every scored attempt, including unfavorable outcomes; protocols, source, fixtures, schedules, hashes, final conclusions, and one final working source version per representative application. For each bulky raw archive or delivered workspace proposed for deletion, record its size, whether it is reproducible, what reported claim it supports, and the loss of exact replay after deletion. Do not copy these bulky artifacts to another local folder by default; that would not reclaim disk space.
- [x] Remove repeated packaged runtime binaries from the **current** tracked tree only after recording their hashes and the reports that cite them. Preserve their existing Git history and the one canonical release artifact needed for reproducibility.
- [x] Verify that retained report files open, that their references and hashes are documented, and that the two current release archives are readable. Do not imply that a saved hash can reconstruct deleted binary evidence.
- [x] **Gate:** The source and compact result record are recoverable and the manifest has no uncategorized large files.

### Task 2A — Reclaim disk space immediately after preservation

- [x] Confirm no process is using the checkout. Delete the ignored `target/` Rust build tree after the two current archives and small unique logs are safe; this should return about 15 GB at once. Rust can recreate the build tree later.
- [x] Remove repeated app `dist/` copies, the crash core, extracted vendor tree, and temporary `work/` output after their source/manifests are retained. Remove the old v2 pinned workspace, 1.1 GB raw evidence archive, and repeated v3 `.delivery/` folders only after Task 2 records the loss of exact replay and verifies the compact reports remain.
- [x] Measure disk space before and after each deletion group and report the actual amount reclaimed. Do not remove tracked source, checkpoint documents, compact reports, or the small current release archives.
- [x] **Gate:** At least the Rust build cache has been reclaimed, with a precise accounting of what was removed and which historical sessions can no longer be replayed byte for byte.

### Task 3 — Establish the canonical checkout and private remote

- [x] Transfer the Git repository and only the selected source, documentation, fixture, and compact evidence files to empty `~/guion` without resetting or rewriting history. Do **not** bulk-copy build caches, deleted raw evidence, old product archives, or repeated app packages into this ordinary checkout. Compare commit ID, tracked tree, and preserved uncommitted inventory with the source.
- [x] Mark the final pre-rename state with an archival Veritas tag. Create `main` from that state, configure `origin` as `https://github.com/rmichaelthomas/guion.git`, and document the mapping from source branch/commit to Guion commits.
- [x] Verify the remote is still empty/private, scan the exact outgoing Git tree for secrets, personal data, oversized blobs, and the size budget above, then push the reviewed history to private `main`. Confirm the remote commit matches local `main`.
- [x] **Gate:** `~/guion` and GitHub contain the same reviewed history; the original checkout has not been removed.

### Task 4 — Rename the product across executable surfaces

- [x] Rename Cargo package names, executable names, CLI help/errors, installer paths, distribution metadata, environment variables, manifests, locks, project-state directory, source/bytecode extensions, generated project templates, and internal imports. Use a reviewed search inventory; avoid blind global replacement.
- [x] Rebuild the embedded self-hosted compiler seed through its documented bootstrap path. Update any bytecode identity or host-ABI version whose serialized contract actually changed. Check old-format handling against the stated migration policy.
- [x] Update active sample projects, library imports, test fixtures, completion runners, and packaging scripts to Guion names. Keep frozen historical campaign inputs/results and their checksums intact.
- [x] **Gate:** A new project can be created, checked, run, built, packaged, installed, and inspected using only Guion-facing names, and the Rust stage-zero/bootstrap path still reproduces its expected compiler identity.

### Task 5 — Make the documentation navigable and honest

- [x] Replace the root README with a concise Guion overview, working quickstart, supported hosts, current status, link to the language reference, and a clearly bounded Python comparison.
- [x] Write/update `docs/getting-started.md`, `docs/architecture.md`, `docs/reference.md`, `docs/compatibility-and-migrations.md`, `docs/status.md`, `CHANGELOG.md`, `CONTRIBUTING.md`, `SECURITY.md`, `LICENSE`, and third-party notices as appropriate. Derive commands and platform claims from the current build and final reports.
- [x] Add `docs/history/README.md` indexing the 48 historical checkpoint files, original plan, sprint log, completion report, and independent campaigns. Preserve historical Veritas text and source identities; correct broken links through indexes or explicit migration notes rather than retroactively editing frozen results.
- [x] **Gate:** A newcomer can find the first program, language rules, tooling commands, known limits, evidence, and historical decision trail from the root README in two link hops.

### Task 6 — Give agents a reliable Guion programming path

- [x] Add `docs/agents/start-here.md`: concise language mental model, exact setup, first program, project layout, build/test/package commands, diagnostics, common mistakes, and when Python interop is needed.
- [x] Add `docs/agents/recipes.md` with complete checked examples for a file/data CLI, HTTP request with capability grant, SQLite/TOML, and a concurrent task. Link examples to runnable source and their expected output; do not duplicate hand-maintained API signatures.
- [x] Add `docs/agents/index.json` (versioned schema) that points to `language/spec.md`, `language/builtin-signatures.json`, `language/conformance/manifest.json`, CLI JSON report schemas, supported targets, and example paths. Validate that links resolve and published examples compile.
- [x] **Gate:** A fresh agent with only the repo can build a small Guion program without relying on this chat or Python source compilation.

### Task 7 — Preserve and explain the test and speed evidence

- [x] Keep the full final structured reports in `evaluation/completion/results/`, including raw performance samples, intervals, correctness outcomes, memory, size, target hashes, bootstrap, conformance, applications, builders, and closeout.
- [x] Add an evidence index that distinguishes historical v1/v2/v3 recommendation campaigns, nine known regression applications, three later fresh matched builder pairs, and final installed speed tests. Link each headline claim to its exact report and product identity. State ties, regressions, unverified free-threaded Python comparison, and supported hosts.
- [x] Preserve original Veritas-era report names and values. If Guion has new benchmark results after rename, record them as a new cohort instead of overwriting old reports.
- [x] **Gate:** Every number in a public-facing status or performance summary traces to a retained raw report.

### Task 8 — Clean generated material, then validate the release candidate

- [x] Check that Task 2A's cleanup is complete and update `.gitignore` narrowly so future generated output does not reappear as untracked noise. Keep the source and compact reports intact.
- [x] Run format, lint, unit/integration, full language conformance, three-stage bootstrap, both supported runtime/application targets, package/relocation, security and license checks, and the repository's `cargo xtask closeout` sequence. Record exact commands, host/tool versions, pass/fail counts, hashes, and any deviations. Re-run the final installed performance suite under the Guion identity and compare it to historical results without erasing them.
- [x] Add continuous integration for checks that can run honestly on GitHub runners; mark target-specific checks that require the existing macOS ARM64 and Debian ARM64 environments as separate gates rather than reporting them green when skipped.
- [x] **Gate:** No required report is missing, no required target is skipped without a documented limitation, and the new Guion checkout passes its declared pre-release matrix.

### Task 9 — Finish the private pre-release handoff

- [x] Create `docs/pre-release-readiness.md` with achieved scope, supported systems, known limitations, remaining product work, install/update/remove guidance, evidence links, source/seed/runtime/stdlib identities, and a cold-start resume prompt for another agent.
- [x] Review links, names, Git status, remote parity, tags, secret/large-file scan, license notices, and evidence archive recovery one last time. Record an immutable pre-release candidate commit and the exact remaining blockers.
- [x] Only after successful parity and recovery checks, clear any remaining reviewed generated material from the old Codex checkout. Keep its small source/history snapshot until Rob separately chooses whether to archive or remove it.
- [x] **Gate:** The private Guion repository is a complete, independently understandable handoff. Publishing a release, making the repo public, deleting the original checkout, and changing project-level `AGENTS.md` are outside this plan.

## Review focus

1. A frozen report points to a path moved or renamed during cleanup: preserve the referenced bytes/path or document and verify a redirect without changing the original report.
2. An ignored directory contains the sole copy of a pinned input or raw evidence: the preservation manifest must catch it before deletion.
3. The self-hosted seed or artifact reader still embeds `veritas` paths/identities after the CLI rename: bootstrap and installed-package checks must exercise the released path.
4. A benchmark appears to improve because source-run, warm-run, and prebuilt-artifact measurements are conflated: keep the measurements and baselines separate.
5. CI reports green while a required ARM64 target did not run: closeout must retain a visible incomplete state rather than a false pass.

## Done means

`~/guion` is the canonical clean checkout; private GitHub `main` matches it; Guion is consistent in new code, CLI, packages, manifests, and user docs; historical Veritas documents and compact result records remain; agents have a runnable learning path; and the pre-release readiness report states precisely what passed and what remains open. Deleted raw historical sessions are marked as non-replayable. No public release occurs under this plan.

## 2026-10-25 closeout addendum

Rob subsequently requested that the original Veritas checkout be fully retired and the disk issue resolved. This supersedes Task 9's earlier checkout-retention boundary. Its source commit is in Guion `main` ancestry. The old checkout's unique small working-session files, exact source patch, and pre-migration v3 README were preserved under `docs/history/veritas-working-sessions-2026-10-25/`; generated bytecode files were recorded by path, size, and hash, then discarded with the checkout. The old checkout has been removed. Private GitHub `main` matches local commit `bb20ceb6cbce19aa3a0b3de48e2286e35dbca194` and the checkout is clean.

The idle Colima VM was stopped gracefully after confirming no running containers. The deleted build-cache handle count is zero. The final whole-volume free-space reading was 41,860,736 KiB (about 39.92 GiB); this snapshot includes other system activity and should not be treated as a precise project-only recovery amount.

The final application cohort contains 18 passing reports on each of the two required targets, 36 passing reports total, with zero failed reports. The free-threaded Python comparison was not run because the required Python executable was unavailable; it has no pass or fail result. The pre-release tag remains on the validated candidate commit; the later documentation closeout commit is on private `main`.
