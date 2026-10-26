# Build prompt — Richards follow-up, agent examples, x86_64 target, north-star instrument

Target agent: Codex. Repo: `rmichaelthomas/guion`, local `/Users/rmichaelthomas/guion`. Main HEAD at writing: `4bb0680` (merge of PR #3).

## §0 — BRANCH

```sh
git checkout main && git pull
git checkout -b codex/guion-richards-followup-and-agent-examples
```

All work stays on this branch. Do not commit to main. Do not touch `AGENTS.md`.

## §1 — SCOPE

Four independent workstreams in one PR.

**A. Richards follow-up.** This builds on `docs/superpowers/specs/2026-10-26-guion-richards-runtime-profile.md` and the results in `evaluation/completion/results/runtime-efficiency/2026-10-26-richards-profile-summary.md`. Its job is to close the reporting and verification gaps left by PR #3. It makes no new optimization.

**B. Agent example fixes.** Correct the five runnable examples in `docs/agents/` so they teach the patterns the docs recommend. Also remove the friction of turning a file error into a `StdError`.

**C. x86_64 Linux release target.** Promote Debian 12-compatible GNU/Linux x86_64 (glibc) from source checks to a validated release target, using the same matrix ARM64 passed (`docs/pre-release-readiness.md`, sha 95d7313). Today `.github/workflows/ci.yml` (sha 04238a4) runs Ubuntu x86_64 format, lint and tests, labelled "not a release-target gate."

**D. North-star instrument.** The repo already measures the north star. `evaluation/protocol/` runs fresh assistant sessions that choose between the language and Python for nine frozen briefs. Its last campaign (`evaluation/results/independent-v3/choice-summary.json`, sha 5ba45ac) chose Python 18 of 18 times, for the pre-rename Veritas 0.1.0. That is the baseline to beat.

Each record names the conditions under which the agent would switch. For example, `05-webhook-replay-choice-1.json` (sha 051aeff) asks for native HMAC-SHA256 and transactional storage. The runtime now depends on `hmac`, `sha2` and `rusqlite`, so that gap may already be closed.

Workstream D turns those conditions into a backlog, gives agents a single-file language reference, catches Python syntax with diagnostics that name the Guion equivalent, and prepares a rerun under the Guion name.

Out of scope:
- any new runtime optimization;
- changes to language semantics or the bytecode/artifact format;
- musl and Windows targets;
- publishing a release or making the repo public;
- installing tools;
- changes to Python interop.

## §2 — FILE INVENTORY (verified at `4bb0680`)

Read:
- `docs/superpowers/specs/2026-10-26-guion-richards-runtime-profile.md` — 6c0bd9e
- `docs/superpowers/plans/2026-10-26-guion-richards-runtime-profile.md` — 29f4d4b
- `evaluation/completion/results/runtime-efficiency/2026-10-26-summary.md` — 1a34af4
- `language/spec.md`, `language/builtin-signatures.json`, `language/host-api.md`

Modify:
- `evaluation/completion/results/runtime-efficiency/2026-10-26-richards-profile-summary.md` — e42fe0b
- `evaluation/completion/results/runtime-efficiency/2026-10-26-richards-profile.json`
- `evaluation/completion/performance/runner.py` and `test_runner.py`
- `crates/runtime/src/lib.rs` and `crates/runtime/src/native_backend.rs` — profile counters only
- `language/stdlib/fs.gu` — 9cf2e73
- `docs/agents/examples/sqlite_toml.gu` — 7ac7089
- `docs/agents/examples/http_request.gu` — 2cd148b
- `docs/agents/examples/failure_paths.gu` — c9658cc
- `docs/agents/recipes.md` — 948e5be
- `docs/agents/start-here.md` — db660e4
- `docs/agents/index.json` — 8d10158
- `README.md` — fd800fe

Create:
- `docs/superpowers/plans/2026-10-26-guion-richards-followup.md` — the progress plan
- `evaluation/completion/results/runtime-efficiency/2026-10-26-richards-followup.json`
- `scripts/verify-agent-examples.sh`

## §3 — PHASE A1: Report Richards against Python, same cohort

1. Build the release CLI for `4bb0680` and for the PR #3 baseline (`226e884`) into one Cargo target. Use `evaluation/completion/performance/session.py preflight`, locked offline dependencies, and `CARGO_INCREMENTAL=0`.
2. Run Richards with the pinned CPython 3.14.7 reference in the same randomized cohort:
   - 15 samples per arm;
   - fresh uncached source, using `--no-cache`;
   - expected output `9297` and `23246` checked on every sample.
3. Report `candidate/Python` with a 95% interval directly, for the fresh lane.
   - Do not derive it by multiplying against the earlier 0.705 figure.
   - Do not state a Python ratio for the 25-repeat lane unless an equivalent Python repeated reference is built and measured. If no equivalent reference exists, say so explicitly.

Acceptance: the summary contains a Richards candidate/Python fresh-lane ratio with an interval, from one cohort.

## §4 — PHASE A2: Peak RSS before and after

1. In untimed runs, sample `getrusage(RUSAGE_CHILDREN).ru_maxrss` for both binaries, in both lanes, with 5 runs each. This is the method the lifecycle test already uses.
2. Report the medians and the delta.
3. Replace the summary's "peak memory not measured" line with the measured numbers.
4. If the candidate's peak RSS is more than 5% above baseline in either lane, record that as an open finding. Do not fix it in this PR. *(The 5% threshold is Claude's addition.)*

## §5 — PHASE A3: Copy-size distribution and branch attribution

1. Add a profile-only histogram of copy-on-write copy sizes, in element count. Buckets: 0–8, 9–32, 33–128, 129+.
   - It must sit behind the existing opt-in profile flag.
   - The disabled path does no counter work.
   - Add a test that the histogram is absent by default and that its counts sum to the total copy count.
2. Report the share of copies and of copy bytes that fall in the ≤8 inline range.
3. Attribute the 1,555,377 dynamic guest conditional branches. Use the macOS built-in `sample` command against a 25-repeat Richards run (no install), plus the existing counters. State whether VM dispatch, native helpers, or allocation dominates the sampled time.
4. Make no optimization in this phase.

Acceptance: the summary has a "Remaining cost" section that names the dominant measured cost and says what the measurements cannot establish.

## §6 — PHASE A4: Clear the blocked verification

1. Run the local HTTP fixture tests and the nine-application acceptance run that loopback denial blocked in PR #3.
2. If they are still blocked by the environment, record the exact error in the summary as a carried-forward known gap. Do not elevate permissions.

## §7 — PHASE A5: Correct the summary and README headline

Rewrite the result line in `2026-10-26-richards-profile-summary.md` and the matching README sentence to state:
- the measured candidate/Python ratio;
- 5.2% fresh / 14.1% repeated against the prior Guion build;
- the RSS result;
- the dominant remaining cost from A3.

Claim nothing beyond the measurements.

## §8 — PHASE B1: File error to StdError

1. Add to `language/stdlib/fs.gu`:
   ```
   pub fn fs_read_text(path: Text) -> Result[Text, StdError]
   ```
   It wraps `read_text`. On error it returns `std_error(file_error_code(e), file_error_message(e))` and fills `path`. *(The name and shape are Claude's addition. Check the language contract for a conflicting name first.)*
2. Import `std:types` as needed. Confirm that importing types from fs does not create a cycle.
3. Add a stdlib test covering both the success path and the missing-file path. It must assert the code and path.
4. If `builtin_catalog`, the doc reports, or a conformance manifest enumerate stdlib functions, update them in the same commit.

## §9 — PHASE B2: Fix the examples

1. **`sqlite_toml.gu`**:
   - Delete `read_settings()` and use `fs_read_text("settings.toml")?`.
   - Replace the unused bindings `created`, `inserted`, `committed` and `closed` with `let _ =`. The `let _ =` form is already valid and used in `failure_paths.gu`.
   - If `guion lint` flags `let _ = expr?`, report the lint output and use whatever form lints clean.
2. **`http_request.gu`**:
   - Rewrite it to use `http_request_bytes`. Take the exact signature from `language/builtin-signatures.json` and `language/host-api.md`; do not guess it.
   - Return a typed `HttpResponse`.
   - Replace `is_ok`/`unwrap`/`error` with `match`.
   - Print the status, then the response body byte count. This follows the user's scope decision to keep the byte-oriented HTTP API and avoid adding a bytes-to-text feature in this pass.
3. **`failure_paths.gu` `denied-http` mode**: switch it to `http_request_bytes`. The printed code must stay `R_NETWORK_DENIED`. If the bytes API returns a different error type or code, update `recipes.md` and `index.json` to match the real output.
4. **`recipes.md`**:
   - Change the HTTP command to the `cargo run --locked -p guion-cli -- run --allow-http-host ...` form, matching the other recipes.
   - Update the HTTP prose to describe the typed byte response.
   - Rewrite "writes go to the last explicitly granted root" to state the actual rule precisely. Read it from `language/host-api.md` and the runtime; do not change the rule.
5. **`start-here.md`**: update the recipe description if anything changed, and mention `fs_read_text`.
6. **`index.json`**: update `expected_output` for any example whose output changed. It must still validate against `index.schema.json`.

## §10 — PHASE B3: Example verification script

Write `scripts/verify-agent-examples.sh`, and commit it. It must:
1. run `check`, `lint` and `fmt --check` on all five `.gu` files;
2. run `file_data_cli`, `sqlite_toml` (with a temporary `--file-root`), `concurrent_task`, and all four `failure_paths` modes;
3. assert stdout and exit status against `index.json`;
4. compile `http_request.gu` only. It must not make a network call.

Output a pass/fail table to stdout and to `agent-examples-verification.md`.

## §10a — PHASE C1: x86_64 Linux target

1. **Inventory every ARM64-only assumption.** Read `crates/cli/src/native_dependencies.rs`, `docs/pre-release-readiness.md`, `evaluation/completion/results/guion-final/README.md`, and the xtask/release scripts the ARM64 matrix used. Grep for `aarch64` across `crates/`, `xtask/`, `distribution/`, `native/` and `evaluation/`. List each target-specific path. Do not assume none exist.
2. **Add the target.** Add `x86_64-unknown-linux-gnu` wherever the ARM64 Linux target is enumerated:
   - target lists;
   - native dependency builds (libical and any others);
   - release packaging and dynamic-library audits;
   - the license inventory.
   The native backend is Cranelift 0.135.0 with `cranelift-native` (`crates/runtime/Cargo.toml`, sha cb8e17f). Its host ISA detection covers x86_64, so no new backend is needed.
   The known blocker is `crates/cli/src/native_dependencies.rs` (sha 4955c57). The Linux audit hardcodes aarch64 in four places:
   - the `ARCH != "aarch64"` guard in `write_linux_report`;
   - the `"AArch64"` machine check in `inspect_elf_image`;
   - the `"linux-aarch64-gnu"` target string;
   - the `["aarch64"]` architectures field.
   Make all four derive from the host architecture. Map ELF `Machine:` values: `AArch64` for aarch64, `Advanced Micro Devices X86-64` for x86_64. Leave the macOS path unchanged.
   Add a unit test for the x86_64 `readelf -h` parse and an `ldd` fixture using `/lib/x86_64-linux-gnu/`.
   Then cover the two other aarch64 references in the step-1 inventory: `crates/model/src/lib.rs` and `crates/runtime/tests/lifecycle.rs`.
3. **Add a CI job** in `.github/workflows/ci.yml` running on the GitHub-hosted x86_64 runner inside a `debian:12` container. It runs the same validation the Debian ARM64 cohort ran:
   - the three-stage self-hosted compiler bootstrap;
   - the 195-case conformance corpus;
   - runtime, HTTP and lifecycle checks;
   - the stdlib suites;
   - all nine regression applications in initial and follow-up forms (18 reports);
   - the Python-free core dependency check;
   - release archive and license inventory;
   - clean-room install, relocation and integrity rejection;
   - dynamic-library audit.
   Leave the existing Ubuntu source-check job as is.
4. **Run it and record the results.** Push, trigger the workflow with `workflow_dispatch`, and download the reports into `evaluation/completion/results/guion-final/` as an x86_64 target record. Update the aggregates so they list three targets.
5. **Update the docs.** Add `x86_64-unknown-linux-gnu` to `current_guion_targets` in `docs/agents/index.json`, and update the supported-systems section of `docs/pre-release-readiness.md`, `docs/status.md` and `README.md`. Do this only for the gates that actually passed.
   - No x86_64 performance claim. Performance remains macOS-only.

Acceptance: every gate the ARM64 Linux matrix passed also passes on x86_64 in CI, with retained reports. Any gate that fails is recorded as failed and the target is not listed as validated.

## §10b — PHASE D1: Agent switch-condition ledger

1. Read all 18 records in `evaluation/results/recommendations/` and the `independent-v3/recommendations/` records. Extract every `rationale` objection and every `conditions` entry.
   - Interpret structured `conditions` as formal switch triggers. In rationale text, extract only explicit counterfactual reconsideration clauses; preserve all rationale text verbatim and do not recast ordinary pros and cons as switch conditions.
2. For each one, determine its status at the current HEAD, using `language/builtin-signatures.json`, `language/stdlib/` and `language/host-api.md`. The status is one of:
   - `closed` — the feature exists, with a cited signature or test;
   - `partial`;
   - `open`.
   Do not mark anything closed without a cited signature or test.
3. Write `evaluation/agent-choice/switch-conditions.json` and a short `switch-conditions.md`. Group them by frequency, and show the brief IDs for each condition.
4. **Make no feature changes for open items in this PR.** The ledger is the backlog.
5. **Update the product-information bundle** so closed items are documented where a choosing agent will see them.

## §10c — PHASE D2: Single-file agent reference

1. First check whether one already exists: search the root, `docs/`, `docs/agents/` and the `distribution/` contents for a single-file reference such as `llms.txt` or `AGENTS-REFERENCE.md`. If one exists, extend it instead of creating a new one.
2. Otherwise, generate `docs/agents/guion-reference.md` from `language/spec.md`, `builtin-signatures.json` and the stdlib `pub` signatures. It holds:
   - the syntax summary;
   - every built-in and stdlib signature;
   - the effect and grant model;
   - the error-code list;
   - the five recipes, inline.
   It must fit in one agent context: keep it under 120 KB. *(The 120 KB limit is Claude's addition.)*
3. Add `cargo xtask agent-reference --check`. It must fail CI when the file is out of sync with its sources.
4. Ship the file in release packages next to the installed CLI, and link it from `start-here.md` and `index.json`.

## §10d — PHASE D3: Python-syntax recovery diagnostics

1. First read `language/spec.md` and the compiler's diagnostic table for existing handling of Python constructs. Extend that handling; do not duplicate it.
2. When the compiler meets common Python forms, emit a stable diagnostic code with the Guion replacement in the message. Cover at least:
   - `def`;
   - `elif` (if Guion uses something else);
   - `None`, `True` and `False` (if they differ);
   - `import x` and `from x import y`;
   - `for i in range(...)`;
   - list comprehensions;
   - f-strings;
   - `try`/`except`;
   - `class`.
   Every replacement must be a valid form taken from the spec. Use no message for a form Guion lacks unless it states the supported alternative.
3. Add one conformance case per diagnostic: Python input, expected code, expected message substring.
4. Add the codes to the reference file from D2.

## §10e — PHASE D4: Prepare the Guion choice rerun

1. **Rebuild the product-information bundle** under the Guion name, with the current docs and the D1 ledger's closed items. Keep the existing protocol's digest, freeze and schedule rules unchanged.
2. **Add two arms, by amending the protocol before any run, as `protocol-corrections.md` requires:**
   - **Multi-model.** The campaign config lists Codex, Claude and at least one other provider, each run under the same schedule. *(The added models are Claude's addition.)*
   - **Cold arm.** Same briefs, no product bundle, and Guion named only as an available option. This measures what models already know without being given docs, which is what "agents reach for it" means in practice.
3. **Run the Codex arm** if this host supports the protocol's fresh-session mechanism, as it did for v3. The Claude and other-provider arms cannot be launched from a Codex session. Render and freeze their prompts and schedule entries, and mark them `not_started` with that reason. Do not substitute other models.
4. Report results only under the protocol's existing summarizer. Make no recommendation claim beyond its rules.

## §11 — CONSISTENCY INVARIANTS

1. Every expected output in `index.json` equals what `scripts/verify-agent-examples.sh` observes.
2. Every command in `recipes.md` uses the same `cargo run --locked -p guion-cli --` prefix.
3. No example uses `unwrap` on a `Result` whose success hasn't been established.
4. Profile counters do no work when profiling is off. A test proves this.
5. Every ratio in the summary and README comes from a cohort recorded in this PR's JSON, or is explicitly attributed to the earlier report.
6. No change to language semantics, runtime limits, or the artifact format.
7. A target appears in `index.json` or the docs as validated only if its full matrix has passed, with retained reports. The compiler-stage identity must match across all three targets.
8. Every `closed` entry in the switch-condition ledger cites a signature or test that exists at the PR's HEAD.
9. Every replacement named in a Python-recovery diagnostic compiles under `guion check`.
10. The agent reference passes `xtask agent-reference --check` at every commit that changes the spec, signatures or stdlib.

## §12 — FAILURE MODES

| # | Mode | Watch for | Prevention |
|---|---|---|---|
| S1 | Scope built on a false absence | A StdError file helper already exists | Grep `language/` and `language/self_host/` for an existing wrapper before B1 |
| S2 | Unrunnable gate | Acceptance blocked by loopback | Record it as a known gap in A4; do not elevate |
| S3 | Human-gate bloat | — | All checks are scripted |
| S4 | Merge mistaken for deploy | — | N/A: no deploy |
| 1 | Cross-cohort ratio | Python ratio computed by multiplying against 0.705 | Same-cohort run only (A1) |
| 2 | Profiler contaminating timing | Counters on during timed samples | Keep timed and profile runs separate |
| 3 | Error code drift | `denied-http` code changes under the bytes API | Docs follow the real output |
| 4 | Network in CI | `http_request.gu` executed | Compile only |
| 5 | Hardcoded aarch64 in the Linux audit | The release audit fails on x86_64 with "currently supports aarch64" | The four `native_dependencies.rs` edits in C1 step 2 |
| 6 | Ubuntu passing mistaken for Debian | Results come from the host runner, not the container | Run the matrix inside `debian:12` and record the OS in each report |
| 8 | Ledger marked closed from memory | `closed` with no citation | Invariant 8 |
| 9 | Diagnostic teaches invalid Guion | The suggested replacement fails to compile | Invariant 9 |
| 10 | Protocol changed after results | Amendments added after any session | Amend and freeze before D4 runs |
| 11 | Old Veritas evidence relabeled | v3 records edited or renamed | v3 is read-only; the new campaign goes in a new directory |
| 7 | Premature target claim | Docs updated before the matrix is green | Invariant 7 |

## §13 — VERIFICATION (agent, no human step)

- `cargo fmt --check`
- strict Clippy on the touched crates
- runtime and CLI tests
- performance harness tests
- the five short-source gates against Python; all must still pass
- `scripts/verify-agent-examples.sh` all green
- the x86_64 Debian CI job green, with its reports retained
- the conformance corpus passes, including the new Python-recovery cases
- `xtask agent-reference --check` green
- the switch-condition ledger validates
- the D4 bundle and schedule pass the protocol's existing validators

Blocking: any red result here, any failing short-source gate, or an example output mismatch. Attach both verification tables to the PR.

## §FINAL

1. Push the branch.
2. Open a PR titled "Richards follow-up, agent examples, x86_64 Linux target, agent-choice instrument". It lists the phases, files, and invariants, and attaches the verification tables.
3. Merge it.
4. Clean up:
   ```sh
   git checkout main && git pull
   git branch -d codex/guion-richards-followup-and-agent-examples
   git push origin --delete codex/guion-richards-followup-and-agent-examples
   ```
5. Remove `target/perf-session/`.
## Execution task map

The numbered tasks below preserve the user-approved sections above and are executed sequentially. Full requirements and acceptance criteria remain in the referenced sections. Progress is recorded in this file and the plan-scoped SDD ledger.

## Task 1: Preflight and shared-interface audit
Read the required source/spec/status files and inspect repository contracts, current test commands, and every cross-phase interface before implementation. Record verified base and any rulings.

## Task 2: Richards same-cohort Python comparison
Execute §3 exactly and record fresh-source candidate/Python ratio and interval in the raw result.

## Task 3: Richards RSS and copy-size/branch profile
Execute §§4–5 exactly, including histogram correctness, disabled-path coverage, and sampled-cost attribution.

## Task 4: Resolve prior blocked checks and publish Richards evidence
Execute §§6–7 and update the dated Richards summary and README only from this pass's measured evidence.

## Task 5: File-error StdError helper and example corrections
Execute §§8–9, preserving language semantics and the five examples' actual output contracts.

## Task 6: Agent-example verification script
Execute §10 and prove the script matches index.json without making a network request.

## Task 7: x86_64 Linux target inventory and native audit support
Execute §10a steps 1–2, with tests first and no premature target-validation claims.

## Task 8: Debian x86_64 release CI matrix
Execute §10a steps 3–4, retain the generated reports, and update target aggregates only for gates that actually pass.

## Task 9: Release-target documentation consistency
Execute §10a step 5 and §11 invariants concerning validated-target claims.

## Task 10: Agent switch-condition ledger
Execute §10b from all source recommendation records, cite every closed condition, validate output.

## Task 11: Single-file agent reference
Execute §10c and add the drift check and package integration.

## Task 12: Python-syntax recovery diagnostics
Execute §10d with test-first compiler diagnostics and conformance cases.

## Task 13: Guion choice-rerun preparation
Execute §10e, preserve existing campaign immutably, and run only the model arm this host can support.

## Task 14: Full verification, review, PR, merge, and cleanup
Execute §§11–13 and §FINAL. Attach verification tables, merge only with accurately recorded gate results, and remove only this plan's scratch workspace and target/perf-session.
