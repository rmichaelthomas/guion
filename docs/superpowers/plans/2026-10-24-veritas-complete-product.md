# Veritas complete product implementation plan

> **For agentic workers:** Use `superpowers:executing-plans` for inline execution. The user's established workflow overrides per-task approval, fresh-agent, and repeated review-packet defaults. Work through the checkboxes in this document in one existing workspace. Do not create another chat or start another campaign to begin implementation.

**Goal:** Finish a usable self-hosted Veritas compiler and Python-free core toolchain, implement the everyday capabilities identified by the recommendation failures, deliver real applications on both established targets, and earn an evidence-backed recommendation for the defined builder use cases.

**Architecture:** Veritas owns source parsing, module resolution, type checking, lowering, and artifact emission. The existing Rust runtime, verifier, Cranelift backend, and bounded native host services remain implementation infrastructure. Ordinary libraries and application logic use Veritas; mature native libraries supply SQLite, cryptography, Unicode data, and calendar/configuration interoperability through typed APIs. The installed core product neither invokes nor bundles Python.

**Tech stack:** Existing Rust workspace and Cranelift 0.135.0; Veritas compiler and library sources; versioned `.vrb` artifacts; native libraries pinned in Cargo.lock and a native component manifest. Existing Python programs remain comparison baselines and external evaluation tools only. They are not Veritas implementation dependencies.

**Spec:** The binding completion contract below reconciles `docs/north-star-design.md`, `language/spec.md`, `language/host-api.md`, the user's subsequent self-hosting/Python-free requirements, and all 18 rationales under `evaluation/results/independent-v3/recommendations/`. Earlier prototype plans and campaign sequencing are historical. Frozen reports remain unchanged.

**Status:** Complete. Tasks 1–11 passed their verification gates. The final closeout passed with all three goals satisfied on product snapshot `sha256:4d515d48ba1912119df9cb800cc73d6c1a8ddf02ca25a66de4589b51c9258066`. The completion report records the target scope, artifact hashes, benchmark limitations, and install/update/removal instructions.

## 1. Binding completion contract

### 1.1 What this build must deliver

| ID | Required outcome | Evidence required before marking complete |
|---|---|---|
| G1 | A compiler written in Veritas that is the normal compiler for the shipped language | Full core-language conformance, equivalent rejection of invalid programs, stage-1/stage-2/stage-3 artifact equality, complete application graphs compiled by the resulting compiler, and a default installed CLI that has no dependency on the Rust source compiler |
| G2 | Demonstrably competitive execution, with speed wins over Python on the established compute/data work | Correct outputs first; repeated matched measurements from the final installed build; confidence intervals and every workload reported; no hidden Python, canned results, weakened arithmetic, or excluded failed inputs |
| G3 | A product an assistant can reasonably recommend over Python for the target builder work | Complete installed lifecycle; native solutions for all nine known application briefs and follow-ups; fresh builders performing unfamiliar work and changes; explicit recommendations grounded in observed benefit |

The target work is the three classes already established in the design: small automation/data CLIs, concurrent network tools, and maintainable local applications with configuration and persistent state. G3 requires evidence for each class claimed in the release recommendation; the intended closeout covers all three. A success in one class is not permission to mark the other two complete.

This release does not promise to reproduce Python's whole ecosystem or win every possible program. Its boundaries are the full supported language contract, the libraries in §3, the builder lifecycle in Task 8, and the applications and targets below. Those boundaries must not be narrowed during implementation to make a failing task disappear.

**Self-hosted means the complete source-to-artifact compiler is written in Veritas.** A Rust VM, native verifier, or Cranelift dependency does not invalidate that. An incomplete Veritas frontend that delegates unsupported source to Rust does. Port the artifact writer to Veritas as well, so the existing Rust IR assembler is not a required compilation stage in the installed product.

**Python-free core means:** new/check/fmt/lint/test/run/build/package/sync/update/inspect/doc/doctor/install work for core projects without Python or uv installed, and native application packages do not contain or start them. Keep historical Python interoperability available only as an explicit optional compatibility feature. It cannot satisfy any required core-library or application acceptance item. No user types `python` or `python3` to use Veritas.

**Native means executed by the Veritas VM/JIT or a documented in-process native library.** It does not require every operation to be JIT compiled. Each timed report must identify VM/JIT/native-library coverage and any fallback. Source-language compilation cannot silently fall back to the Rust source compiler.

### 1.2 Preserve the existing language while completing it

Cover every executable construct and host operation in the current language contract: arbitrary-precision Int and its division rules; finite Float64; Text/Bytes; records; typed lists/maps; immutable value behavior and copy-on-write alias preservation; Option/Result and propagation; all statements and expressions; complete module graphs and visibility; parallel_map; effect policy; source maps; resource limits; and existing external declarations in the optional compatibility profile.

Add the standard-library import form, typed native resources, safe data access, reusable collection/text facilities, and dependency behavior specified here. Planned constructs not currently promised as executable, such as general closures, do not become hidden prerequisites. A compiler intrinsic for a typed named comparator is sufficient for generic sorting, as with the existing named-worker parallel_map. Do not add a second general type-system redesign to this sprint.

Changes to module visibility may expand support to duplicate **private** names in separate modules through internal qualification. Ambiguous imported public names must remain errors. No runtime-generated source, benchmark-name special cases, precomputed application answers, or application-specific host operations.

### 1.3 Completion decisions

The engineering build is complete only when Tasks 1–10 pass their exit conditions. The north-star claim is complete only when Task 11 supports it. A finished checklist cannot force another builder to prefer Veritas. If the final comparison still favors Python, record the exact capability or usability deficit, fix it within this same plan, and repeat the affected end-to-end comparison. Do not relabel that result as success, manufacture favorable votes, or spin up another large choice campaign.

The frozen 18-choice result remains **18 Python, 0 Veritas**. The old 18-pair protocol is not the execution dependency for this completion plan. New completion evidence must be labeled as a different evaluation with different scope, not scored retroactively under that protocol. No additional campaign-review packet is required before building missing product capabilities.

## 2. Current starting point and evidence boundaries

Inspected in the current workspace on 2026-10-24:

- `language/self_host/compiler.vr` emits JSON IR; `crates/cli/src/bytecode_ir.rs` assembles it in Rust. The compiler entry expects one source path. The self-host project manifest still points to `parser_smoke.vr`.
- `load_program` in the Veritas compiler uses a seen list and concatenates declarations. That does not establish the specified distinction between cycles and repeat imports or complete module visibility. Compiler paths use assertions for source failures. Full semantic checking must be demonstrated, not inferred from compiling the compiler.
- The normal CLI imports `veritas_compiler::compile_project`. Both CLI and runtime currently declare the source compiler as a normal dependency; inspected runtime uses are in tests/examples and can move to development dependencies.
- The runtime maps guest calls onto recursive host calls. The CLI reserves a 64 MiB thread stack as a workaround. This is a concrete runtime completion task.
- Existing bootstrap evidence reproduces IR across three stages and builds inventory and Richards programs. The Veritas compiler is not yet the full normal product compiler.
- Latest recorded one-host author calibration: Richards artifact 44.8 ms vs Python 70.4 ms; N-body 36.3 vs 200.0; Mandelbrot 9.7 vs 39.3; generated 100k-row JSON 107.2 vs 111.7. These are inputs to final verification, not new measurements or universal speed claims.
- The 18 choice rationales consistently identify native library gaps. Two matched build pairs were attempted; the original reports have evaluator defects. The later correction-6 source/package diagnostic succeeds for one task, but is not a scored campaign.
- The latest session recorded 216 passing workspace tests. The sprint log and protocol amendment do not yet describe all later Python-CWD fixes; read source and the correction-6 report when reconciling those records.
- `veritas.lock` is already used for Python dependency metadata. A native dependency implementation must migrate it explicitly; it cannot overwrite the old format.
- Source changes and v3 evidence are currently uncommitted. Preserve them. Build caches were recently deleted; rebuild once and retain the working cache through the sprint.

### Prior-language lessons to use concretely

Planes' current README documents a self-hosted interpreter and cross-implementation diagnostic comparisons. Its `scripts/run_corpus_selfhosted.py` includes an inert host arrangement: useful for semantic agreement, insufficient for Veritas delivery acceptance. Apply its whole-corpus and diagnostic-parity method, then use actual files, sockets, databases, and process restarts for Veritas integration.

Use `docs/prior-language-build-lessons.md` for the previously inspected Liminate and Decan lessons: complete-input parsing, visible semantics, precise unsupported behavior, and real consumers. The inspected Decan README explicitly lists VTIMEZONE as unsupported, so do not assume its calendar adapter closes Veritas's calendar gap. Other repositories are read-only references; do not port their domain semantics or copy a simulated host into product acceptance.

## 3. Recommendation failures become required implementation

Each row owns reusable APIs, documentation, and application use. Implementing the final report directly in a host function fails this requirement.

| Recorded obstacle | Required reusable capability | Tasks | Real consumers |
|---|---|---|---|
| CSV requires a Python parser | Strict streaming CSV reader/writer, quoting, multiline fields, useful row/column failures | 4, 9 | SLA report plus a different import/export program |
| Strict timestamps and dates missing | Gregorian date parsing, exact timestamp profiles, UTC conversion/formatting, pinned timezone data | 4, 5 | SLA, JSONL, calendar, health, auditor |
| Malformed JSON shapes abort the whole invocation | Recoverable typed access, path-aware errors, streaming JSONL with physical line numbers | 4 | JSONL and health processing continue after bad records |
| Version precedence and sorting awkward | Full SemVer 2.0 parsing/comparison, stable generic sorting, bytewise tie-breaks | 4 | Release notes and lock auditor |
| No direct filesystem traversal or full case folding | Bounded no-follow traversal, strict UTF-8, Unicode case folding/whitespace, deterministic order | 4, 5 | Documentation index and release notes |
| Crypto and durable state require a second runtime | HMAC-SHA256 verification on exact bytes and transactional SQLite with typed bound parameters | 5 | Webhook replay and documentation index |
| TOML edits lose formatting or need Python | Comment-preserving typed edits, explicit source preservation on no-op/failure | 5 | Config migrator and package manifest tooling |
| Calendar/timezone integration needs Python | iCalendar parsing with VTIMEZONE/IANA resolution and explicit invalid-time rules | 5 | Calendar conflict tool |
| HTTP/network configuration adds friction | Byte bodies, total deadlines, cancellation, bounded concurrency, clear per-host grants | 6 | Calendar, health and webhook programs |
| Extra runtime and fragile packaging erase benefits | Core packages with no CPython/uv, normal caller data paths, offline relocation, matched targets | 7, 8, 10 | Every shipped application |
| Experimental compiler/tooling increases repair cost | Full self-host compiler, accurate diagnostics, real formatter/lint, indexed API docs, stable lock/update behavior | 2, 3, 7, 8 | Every builder |

## 4. Global constraints and execution discipline

- Repository: `work/veritas-foundation` inside this task workspace. Use this checkout and one existing pinned Linux environment. Do not duplicate the repo or provision another VM.
- Use `python3` for existing external evaluation scripts. Never probe or invoke `python`. Python is not permitted inside core implementation or delivery.
- Preserve uploaded originals, NowLookHere, Planes, Liminate, Decan, existing source edits, and historical campaign records. Do not edit project AGENTS.md files. No session-contract startup is needed in this chat, as requested by the user.
- Default to one implementer; at most one helper for a genuinely independent task if delegated by the user. Two Cargo jobs and one heavy build/container run at a time. Final fresh builder contexts run sequentially, not as a process swarm or new sidebar chats.
- Read the existing environment/permission state once. Request known-required network or loopback access before the operation that needs it. Reuse pinned toolchains. Do not intentionally repeat a known-denied run first.
- Preserve immutable snapshots and the one useful build cache. Cleanup is limited to a completed task's disposable staging area or a concrete storage-pressure event. No recurring global cache purge, archive survey, or hygiene phase after compaction.
- A task's regression checks plus its new acceptance tests are sufficient while implementing. Run the broad suite at the three checkpoints below, then repeat only checks affected by later changes. Do not rebuild both release packages after every source edit.
- Repair blocking correctness defects in the owning task and continue. Style suggestions and optional new features do not create release gates. No per-task reviewer packets or new RC numbering.
- At compaction, preserve: active task, completed checkboxes, changed files, last command/result, remaining failure, and next command. Resume that command. Do not reload the entire historical sprint or restart an environment audit.

### Three checkpoints, not permission pauses

1. **Compiler checkpoint, after Task 3:** full-language compiler coverage and verified bootstrap. Continue to libraries.
2. **Product checkpoint, after Task 9:** all nine native applications, normal toolchain, and persistent-runtime behavior. Continue to final packaging and measurement.
3. **Closeout checkpoint, after Task 11:** installed product, speed results, fresh builder outcomes, recommendation and release record.

These are concise evidence updates. Ask the user only for a real unresolved product decision that contradicts this contract, destructive actions outside the approved scope, credentials, or public publication. A test failure is normally an implementation task.

### Future-forward decisions implemented now

| Future use | Decision in this build |
|---|---|
| Agents repeatedly inspect, edit and repair substantial programs | Stable diagnostic/report schemas, local version-matched API docs, deterministic compiler outputs and machine-readable effects; Tasks 2, 3, 8 |
| Long-running applications and embedded hosts | Explicit invocation/resource lifetime, bounded guest frames, cancellation and repeated-call memory checks; Tasks 5–6 |
| More backends, architectures and native integrations | Versioned artifact/host ABI and target manifests; no platform paths embedded into portable application semantics; Tasks 3, 7, 10 |
| Larger dependency ecosystems and safe upgrades | Content identities, explicit locked graph, transactional migration/update and independent package roots; Task 7 |
| Faster incremental development later | Source/compiler/stdlib/dependency/target hashes recorded now so future caches can invalidate correctly; caching must never change compilation semantics |

These are required foundations. Browser ports, public registries, new schedulers and speculative distributed execution do not become extra projects in this closeout.

## 5. File ownership and interfaces

Existing files remain the authority for current types, artifact encoding and runtime semantics. Split touched functionality into focused modules without an unrelated workspace rewrite.

| Files | Responsibility |
|---|---|
| `language/self_host/{lexer,parser,compiler}.vr` | Existing entry points, extended to full contract |
| New `language/self_host/{diagnostics,resolver,typecheck,lowering,artifact,format,driver}.vr` | Complete Veritas compilation and formatting pipeline |
| New `language/bootstrap/{compiler.vrb,manifest.json}` | Reproducible compiler seed embedded in the default CLI; manifest binds source, schema and compiler hashes |
| `crates/compiler/src/lib.rs`; new `crates/bootstrap/` | Existing Rust source compiler retained solely as explicit developer bootstrap/reference |
| `crates/model/src/lib.rs`; new `language/artifact-format.md` | Types, host signatures, artifact ABI, verifier, complete public encoding contract |
| `crates/runtime/src/{lib,native_backend}.rs`; new `frames.rs`, `resources.rs` | Guest call frames, cancellation, resource lifetime, native fallback parity |
| New `crates/runtime/src/standard_library/{mod,bytes,fs,unicode,crypto,sqlite,toml,calendar,http}.rs` | Bounded in-process native services; expose general operations |
| New `language/stdlib/{cli,text,collections,json,jsonl,csv,time,semver,fs,crypto,sqlite,toml,calendar,http}.vr` | Typed public standard library; portable logic in Veritas |
| `crates/cli/src/main.rs`; new `compiler_driver.rs`, `native_packages.rs`, `tooling.rs` | Normal self-hosted compiler selection, native dependency lifecycle, builder commands |
| `crates/cli/src/dependencies.rs`, `crates/runtime/src/python_bridge.rs` | Optional Python compatibility feature, isolated from the core build |
| New `xtask/{Cargo.toml,src/main.rs}` and `.cargo/config.toml` | Single Rust development runner for bootstrap, conformance, release and completion evidence |
| New `language/conformance/`, `language/stdlib/tests/`, `crates/*/tests/` additions | Semantic, diagnostic, native-service and installed-product checks |
| New `evaluation/completion/{applications,performance,builder,results}/` | New regression applications and final evidence; never mixed into frozen v3 scores |
| `distribution/`; new `distribution/native-components.lock.json` | Core packages for macOS ARM64 and Debian-compatible Linux ARM64, native dependencies/data/licenses |
| `docs/reference.md`, `docs/compatibility-and-migrations.md`, `language/{spec,host-api}.md`, `README.md` | One accurate shipped product contract and repair workflow |

### Interface rules used by the tasks

All signatures below are **planned interfaces**, not claims that the current compiler accepts them. Types and intrinsics enter both compilers and the artifact verifier together.

- `CompileDiagnostic`: `code: Text`, `message: Text`, `source: Text`, `line: Int`, `column: Int`, `end_line: Int`, `end_column: Int`, `hints: List[Text]`. Locations use one-based Unicode-scalar columns, with byte offsets maintained internally.
- `CompileFailure`: `diagnostics: List[CompileDiagnostic]`.
- `CompileRequest`: `entry: Text`, `project_root: Text`, `stdlib_root: Text`, `package_roots: Map[Text, Text]`. Host policy supplies permitted roots independently; request values cannot enlarge authority.
- `CompileOutput`: `artifact: Bytes`, `source_digest: Text`, `diagnostics: List[CompileDiagnostic]`.
- Veritas entry: `compiler_compile_project(request: CompileRequest) -> Result[CompileOutput, CompileFailure]`; `compiler_assemble(ir: Json) -> Result[Bytes, CompileFailure]`; `compiler_format(source_name: Text, source: Text) -> Result[Text, CompileFailure]`.
- Shared ordinary-library failure record `StdError`: `code: Text`, `message: Text`, `path: Text`, `line: Int`, `column: Int`; zero positions mean not applicable. Existing public FileError/JsonError/HttpError APIs remain compatible; wrappers convert them to StdError when composing library operations.
- `std:` imports resolve only to the verified shipped library, e.g. `import "std:csv"`; `pkg:` imports resolve only through the project lock, e.g. `import "pkg:report_helpers/render.vr"`. Existing relative imports stay inside their project/package root. Public APIs use prefixes (`csv_`, `time_`, etc.) to work with current unqualified imports.
- Native resources `FileReader`, `Db`, `Transaction`, `TomlDocument` are opaque runtime-owned types, not forgeable integers. They are invalid as serialized constants, Python values, or parallel-worker inputs/results. Closed/stale/cross-instance handles produce typed errors. All remaining handles close at invocation end; uncommitted transactions roll back on error/cancellation.
- `SqlValue` is a standard record with `kind: Text` and payload fields `integer: Int`, `real: Float64`, `text: Text`, `blob: Bytes`. Constructors validate kinds `null|integer|real|text|blob` and SQLite integer range; do not silently truncate arbitrary-precision integers. Rows contain ordered column names and `List[SqlValue]` values. SQL uses bound parameters, never string substitution.
- `SqlRow` has `columns: List[Text]` and `values: List[SqlValue]` of equal length. `DirectoryEntry` has `path: Text` and `kind: Text` (`file|directory`); paths are relative to the granted traversal root. `SourceLine` has `number: Int` and `text: Text`; `CsvRow` has `line: Int` and `fields: List[Text]`.
- `Date` has `year: Int`, `month: Int`, `day: Int`; `Instant` has `unix_microseconds: Int`. `SemVer` has `major/minor/patch: Int`, `prerelease/build: List[Text]`, and `original: Text`. Library constructors validate these records before using them; source-visible record construction cannot bypass validation.
- `CalendarEvent` has `uid: Text`, `start/end: Option[Instant]`, `start_date/end_date: Option[Date]`, `status: Text`, `all_day: Bool`, and `recurrence_rule: Option[Text]`. Timed and all-day fields are mutually exclusive. Preserve the event recurrence rule as data; the conflict application's contract does not expand event recurrence. `HttpResponse` has `status: Int`, `headers: Map[Text,Text]`, and `body: Bytes`.
- The development runner starts as a small dispatcher in Task 1; each task adds only its own verification command. It reuses existing test/build entry points and has no new orchestration service or evaluation framework. A missing required subcommand is a failure, never a skipped pass. Reports include source revision plus working-tree digest, seed/stdlib/runtime hashes, target, exact command, exit status and artifact identities.

## 6. Implementation tasks

### Task 1 — Establish the executable completion contract

**Files:** new `xtask/`, `.cargo/config.toml`, `language/conformance/manifest.json`, `evaluation/completion/README.md`, `evaluation/completion/status.json`; modify workspace `Cargo.toml`, `language/spec.md` and `.gitignore` only as needed for the bootstrap seed.

**Consumes:** current Type/Op enums, compiler source, current language/host contracts, existing acceptance briefs and 18 choice rationales.
**Produces:** `cargo xtask conformance`, `bootstrap`, `stdlib`, `runtime`, `product`, `release`, `performance`, `builders`, `closeout`; each writes a structured report under `evaluation/completion/results/`. `status.json` records current task, next command, evidence paths and unsatisfied goal IDs.

Only inventory, status/schema handling and existing-command dispatch are implemented here. The named subsystem commands are filled in by their owning tasks. Do not spend the opening phase building generic campaign machinery before touching the compiler.

- [x] Record the current source state without resetting any changes. Inventory all executable syntax, types, operators, host signatures and CLI commands into the conformance manifest. Each entry has positive, negative and interaction coverage, an owning task, and an explicit implementation status for both compilers.
- [x] Wire existing checks into the runner and make missing tests, skipped required targets and absent reports produce `incomplete`, with a nonzero closeout result. Add `completion_missing_evidence_fails`: remove one referenced report in a temporary fixture and assert closeout fails rather than inventing a pass.
- [x] Add a test that every executable Type/Op/host signature appears in the conformance inventory. Preserve manual cases for syntax and visibility; enum counts alone are not conformance.
- [x] Establish versioned core vs optional-Python profiles, std/pkg import behavior, native resource rules, and planned migration semantics in the language contract. Record this plan's replacement of the old evaluation sequencing in the new completion README.
- [x] Register the known nine briefs as **regression applications**, including their follow-ups. They are now known development cases and cannot be relabeled unseen independent evidence.

**Verify:** `cargo xtask conformance --inventory-only` checks coverage mapping; `cargo xtask closeout --check-schema` validates report schemas without claiming completion. The full closeout must correctly remain incomplete.

### Task 2 — Finish language semantics and diagnostics in the Veritas compiler

**Files:** existing self-host lexer/parser/compiler; new diagnostics/resolver/typecheck/lowering modules; `crates/compiler/src/lib.rs`; `language/conformance/`; new `crates/cli/tests/compiler_conformance.rs`.

**Consumes:** Task 1 conformance manifest and CompileRequest/CompileFailure contract.
**Produces:** a full checked program lowered by Veritas for every current executable source feature, including optional external declarations; accurate source diagnostics; verified std/pkg import resolution.

- [x] Add failures that the current subset can miss: mutation of a `let`, wrong assignment/return/argument type, missing returns, non-Bool conditions, invalid record fields, wrong generic arguments, incomplete match, trailing source garbage, and malformed Unicode escapes. For each, assert a stable code and originating file/span, with no runtime assertion or panic.
- [x] Complete parsing and semantic analysis for every manifest entry: maps, Float64/Bytes, contextual empty collections, `for`, Option/Result constructors, exhaustive match and `?`, parallel named workers, all public builtin signatures and external keyword declarations. Generate builtin signatures from one checked declarative catalog rather than maintaining divergent string dispatch tables.
- [x] Implement resolver state `unseen/visiting/resolved`; repeated imports reuse the module, a visiting back-edge reports a cycle. Resolve symbols per module, enforce pub/private, reject public ambiguity, and qualify internal private symbols. Enforce existing graph/file/depth/size and root-escape limits for project, std and package roots.
- [x] Use expected types and explicit type unification/checks throughout expressions and statement lowering. Invalid user programs return CompileFailure. Assertions may remain only for internal invariants already established by validation.
- [x] Compare both compilers over the full corpus: outputs, exits, effects, integer/float behavior, mutation aliases and diagnostic codes/locations. Where the old Rust implementation contradicts the contract, repair both using a spec-derived expected result; agreement between two incorrect implementations is insufficient.
- [x] Cover two independent projects importing different libraries, and a module graph with duplicate private helper names, public collisions, cycles, Unicode filenames and rejected escaping imports.

**Verify:** `cargo xtask conformance --compilers bootstrap,self-hosted` passes every required case; no unsupported current-language construct or assertion-based user diagnostic remains. Serialization can still use the temporary Rust IR assembler until Task 3.

### Task 3 — Emit complete artifacts and make self-hosting operational

**Files:** new `language/self_host/artifact.vr`, driver/format scaffolding, `language/artifact-format.md`, bootstrap seed/manifest, `crates/bootstrap/`, `crates/cli/src/compiler_driver.rs`; modify model/CLI/runtime manifests, CLI command paths, self-host manifest, and existing bootstrap integration test.

**Consumes:** Task 2 checked lowering; current `Artifact::to_bytes`/reader/verifier contract.
**Produces:** `compiler_compile_project` and `compiler_assemble` returning full `.vrb` bytes; an installed self-hosted compiler; explicit developer-only stage-zero tooling.

- [x] Document and implement encoding for every artifact field, value, type, instruction, source span, dependency identity and resource ceiling. Add bounded byte construction primitives if needed; do not add a host function that secretly performs complete artifact assembly.
- [x] Cross-check emitted bytes with the native artifact reader and malformed-artifact cases (truncation, count/length overflow, bad opcodes, forged handles, invalid branches). Compiler artifacts must pass the same verifier as user artifacts.
- [x] Bootstrap C1 from the existing Rust compiler; use C1 to compile the complete compiler to C2; use C2 to compile it to C3. Compare C2/C3 canonical artifact bytes after documented normalization of source-root metadata only. Do not remove actual instructions or diagnostics from equality checking.
- [x] Use C2 and C3 to compile all conformance projects and complete known application graphs available at this stage. Repeat bootstrap after the final library/compiler additions; two handpicked programs no longer satisfy G1.
- [x] Make the default CLI embed the checked compiler seed and invoke the Veritas compiler for check/run-source/build/test/doc/inspect. Keep stage zero in a separately named developer binary. Move `veritas-compiler` out of default runtime/CLI dependencies, retaining it only for bootstrap and differential tests.
- [x] Preserve an acyclic seed-update workflow: build the explicit Rust stage-zero tool independently of the default CLI; use it to create the first compatible compiler artifact, then reproduce that artifact through Veritas stages before replacing the embedded seed. Increment artifact/host-ABI versions when required, retain documented old-artifact readers, and reject incompatible seeds rather than relying on a development path fallback.
- [x] Replace the self-host manifest's smoke entry with the production compiler entry. Record deterministic source/seed/schema hashes and a reproducible regeneration command. Tampered or mismatched seed identity must fail startup/build clearly.
- [x] Prove the installed compiler builds itself and user projects with stage-zero binaries and sources inaccessible. Add `default_cli_cannot_fallback_to_rust_source_compiler`; a dependency-graph inspection and a forced unsupported-input case must both confirm no fallback.

**Verify:** `cargo xtask bootstrap --stages 3` and `cargo xtask conformance --compilers stage2,stage3` pass. Checkpoint 1 reports the exact self-hosting boundary and any outstanding failures, then work continues.

**Pulled-forward dependency:** The first C2/C3 full-corpus run overflowed the normal host stack during guest-to-guest calls. Per the sequence rule below, Task 6's explicit guest-frame execution work is pulled forward to unblock this gate; the remaining Task 6 work stays in sequence.

### Task 4 — Build the everyday data, text and file library

**Files:** `language/stdlib/{cli,text,collections,json,jsonl,csv,time,semver,fs}.vr`; runtime standard-library bytes/fs modules and `resources.rs`; model builtin catalog; both compilers; `language/stdlib/tests/`; new `crates/runtime/tests/stdlib_data.rs`.

**Interfaces:**

| API | Required behavior |
|---|---|
| `json_try_get(Json, Text) -> Result[Json, StdError]`; `json_try_index(Json, Int)`; `json_try_text/int/bool/float` | Wrong shape, missing member and wrong type are values; distinguish absent from null; bool is not int; retain duplicate-key rejection and exact integers |
| `file_open_lines(Text) -> Result[FileReader, StdError]`; `file_next_line(FileReader) -> Result[Option[SourceLine], StdError]`; `file_close(FileReader) -> Result[Unit, StdError]` | SourceLine has physical `number: Int`, `text: Text`; bounded streaming, strict UTF-8, CRLF handling, closed-resource errors |
| `csv_open(Text) -> Result[FileReader, StdError]`; `csv_next(FileReader) -> Result[Option[CsvRow], StdError]`; `csv_encode(List[List[Text]]) -> Result[Text, StdError]` | CsvRow has starting physical line and fields; quoted commas/newlines, doubled quotes, empty fields, LF/CRLF; malformed quoting is typed failure; applications enforce their header policy |
| `time_parse_date(Text) -> Result[Date, StdError]`; `time_parse_timestamp(Text) -> Result[Instant, StdError]`; `time_format_utc(Instant) -> Text`; `today_local() -> Date` | Date has year/month/day; Instant has exact Unix microseconds. Timestamp profile is the briefs' strict grammar: uppercase T/Z, mandatory offset, valid Gregorian date, seconds 00–59, 1–6 fractional digits, canonical trimmed UTC output. Current local date uses the host time zone and requires an explicit clock grant. |
| `semver_parse(Text) -> Result[SemVer, StdError]`; `semver_compare(SemVer, SemVer) -> Int` | Full SemVer 2.0; record major/minor/patch, prerelease identifier list, build identifier list, original text; no machine-integer overflow or build-metadata precedence |
| `sort_by(List[T], named_compare) -> List[T]` | Builtin generic signature; named `(T,T)->Int` comparator; stable sorting, deterministic equal keys, no mutation of aliases; both compilers validate it and reject comparators with I/O/foreign/resource effects through their call graph |
| `fs_walk(Text) -> Result[List[DirectoryEntry], StdError]` | DirectoryEntry has relative path/kind; bounded traversal, no following symlink files/directories, deterministic output, caller-root policy, explicit inaccessible-file errors |
| `cli_parse(List[Text], List[CliOption]) -> Result[CliArgs, StdError]` | CliOption has name/takes_value/repeatable/required; CliArgs has `values: Map[Text,List[Text]]`, `positionals: List[Text]`; distinguish omitted and supplied-empty options, recognize `--`, reject unknown/duplicate forbidden options |

Common text APIs must include scalar and byte comparison, split/join/replace/trim, strict numeric conversion, and list/map lookup returning Option. Implement reusable JSONL iteration over SourceLine and safe JSON access; do not force callers to collect the whole file. JSONL event policy belongs in applications.

- [x] Add data-driven conformance cases for CSV multiline records, Unicode boundaries, malformed JSON shapes, retained physical line numbers, explicit empty CLI values, leap years, offset-equivalent instants, fractional precision, huge SemVer components and prerelease precedence.
- [x] Introduce the bounded native-resource registry for FileReader before exposing file/CSV iteration. It validates owner, kind and liveness and closes readers on invocation completion/error. Calling a line operation on a CSV-mode reader returns a typed mode error. Task 5 extends this same registry for database and TOML handles.
- [x] Implement portable parsing/formatting and convenience APIs in Veritas, using bounded native byte/file primitives for I/O. Return typed errors for ordinary invalid input; retain runtime failures for resource-budget exhaustion.
- [x] Register new primitives in both compilers, verifier and VM; preserve JIT fallback semantics until optimized. Check public APIs through a second independent consumer in addition to the motivating application.
- [x] Verify that invalid data cannot overwrite an existing output and that large files are consumed incrementally. Keep failed byte/line offsets meaningful for repair.
- [x] Add `unix_time_ms()` and `time_today_local()` as explicit-grant clock operations, expose `today_local()` through `std:time`, document host time-zone behavior and deterministic embedding overrides, and reject clock access by default.

**Verify:** `cargo xtask stdlib --group data` passes through the installed self-hosted compile path, including one file larger than the configured input buffer and adversarial malformed rows. Compiler differential cases remain green for new primitives.

### Task 5 — Add durable storage, configuration, Unicode, crypto and calendar services

**Files:** runtime standard-library unicode/crypto/sqlite/toml/calendar modules and resource registry; corresponding Veritas stdlib modules; Cargo manifests/lock; native component lock; new `crates/runtime/tests/stdlib_services.rs` and library tests.

**Dependency decisions:** use `rusqlite` with bundled SQLite; RustCrypto HMAC with the existing SHA-256 family and its constant-time verification API; `toml_edit`; ICU4X `icu_casemap`, `icu_normalizer` and Unicode property data; libical for iCalendar and embedded VTIMEZONE resolution, with a pinned IANA timezone dataset. Resolve compatible exact versions once against the existing toolchain, record them and their sources/hashes/licenses, then build offline. Do not use unpinned `latest`, download on end-user execution, handwrite cryptography, or borrow Decan's unsupported timezone path. The libical wrapper is narrow C FFI with ownership tests, not arbitrary user FFI.

**Interfaces:**

- `text_casefold(Text)->Text`, `text_collapse_whitespace(Text)->Text`, `text_normalize(Text, Text)->Result[Text,StdError]` with explicit `NFC|NFD|NFKC|NFKD` mode. Normalization is never implicit.
- `crypto_hmac_sha256(key: Bytes, body: Bytes)->Bytes`; `crypto_verify_hmac_sha256(key: Bytes, body: Bytes, expected: Bytes)->Bool`.
- `sqlite_open(Text)->Result[Db,StdError]`, `sqlite_begin(Db)->Result[Transaction,StdError]`, `sqlite_execute(Transaction,Text,List[SqlValue])->Result[Int,StdError]` (affected row count), `sqlite_query(Transaction,Text,List[SqlValue])->Result[List[SqlRow],StdError]`, `sqlite_commit(Transaction)->Result[Unit,StdError]`, `sqlite_rollback(Transaction)->Result[Unit,StdError]`, `sqlite_close(Db)->Result[Unit,StdError]`.
- `toml_parse(Text)->Result[TomlDocument,StdError]`, `toml_has(TomlDocument,List[Text])->Bool`; `toml_get_text/int/bool` return the corresponding `Result[T,StdError]`; `toml_put_text/int/bool` take the document, key path and typed value and return `Result[Unit,StdError]`; `toml_render(TomlDocument)->Text`. Unknown untouched TOML values remain in the document; never coerce them through lossy JSON. Additional typed array/table APIs follow the same contract when required by a reusable consumer.
- `calendar_parse(Text)->Result[List[CalendarEvent],StdError]`. CalendarEvent and SqlRow are defined in §5. Return all-day events explicitly; the conflict application ignores them according to its brief.

- [x] Extend Task 4's native-resource registry: closed, stale, foreign-instance and cross-worker handles fail; rollback/close occurs on errors and cancellation; repeated open/close does not leak descriptors or native allocations. Transactions prohibit nesting and use an explicit finite busy deadline. Verified with owner/stale-handle, cross-worker compile rejection, cancellation rollback, repeated open/close, nested transaction and writer-deadline cases.
- [x] Implement actual SQLite transactions, parameter binding, row decoding and durable restart behavior. Restrict database paths and SQLite side files to granted roots; disable extension loading and SQL path escapes through ATTACH/VACUUM INTO. Bound query work with cancellation/progress hooks and result budgets. Verified by bound SQL persistence, root and SQL-policy failures, cancellation, row/byte limits and invalid UTF-8 cases.
- [x] Implement exact-byte HMAC and 32-byte digest verification without logging secret material. Use published known-answer vectors and negative tests; do not test constant-time behavior using unreliable wall-clock assertions. RFC 4231 known-answer and wrong-length/wrong-digest cases pass.
- [x] Implement full locale-independent Unicode case folding and Unicode whitespace collapse as separate operations. Do not substitute lowercase or silently add normalization. Record the Unicode data version. Pin the Python comparison's Unicode version or explicitly account for dataset-version differences. ICU4X 18.0.0 is recorded; Python comparison reports capture `unicodedata.unidata_version` and separate dataset-version deltas.
- [x] Preserve the TOML original text and return it exactly for no-op edits. For changed documents retain unrelated comments/order/representation; use original spans and targeted edits where a full `toml_edit` serialization would reorder dotted keys or alter final-newline behavior. Reparse the edited document and verify the requested semantic changes before publishing it. Validate all migration input before publishing changes. Wrong types and duplicate/invalid keys return errors. No-op bytes, targeted scalar updates, insertion into an existing table, comments, ordering, CRLF/final-newline behavior and unchanged bytes on invalid edits pass.
- [x] Wrap libical with deterministic ownership and timezone-data lookup. Resolve embedded VTIMEZONE before IANA names, support its transition observances, reject unknown zones and floating event times, and follow documented iCalendar semantics for DST ambiguity/nonexistent times. Apply the application's no-event-RRULE-expansion rule without disabling timezone-transition recurrence. Test offset conversion, folded lines, escaping, invalid dates, missing UID, cancelled/tentative status and end-before-start. Also parse an upstream libical calendar sample and recurrent embedded observances.
- [x] Test two concurrent database writers, crash/reopen around a transaction, invalid input preserving existing data, invalid UTF-8, aliases that case-fold to the same page, TOML no-op bytes, and real calendar samples beyond the existing fixture. All cases pass in `crates/runtime/tests/stdlib_services.rs`.

**Verify:** `cargo xtask stdlib --group services` performs actual SQLite/filesystem/calendar operations with Python absent from the candidate environment. Inspect linked dependencies and package all required data/licenses for both targets.

### Task 6 — Finish runtime calls, HTTP and long-lived behavior

**Files:** runtime frames/resources/lib/native_backend and HTTP module; CLI execution entry; runtime execution/HTTP tests; new `crates/runtime/tests/lifecycle.rs`.

**Consumes:** full compiler and typed resources. **Produces:** explicit guest frame execution and a byte HTTP interface with bounded total deadlines/cancellation.

- [x] Replace recursive VM-to-VM host calls with an explicit heap frame stack, preserving locals, operand state, return continuation, spans, depth and instruction limits. Remove the CLI's large-stack workaround after this passes. Treat native-to-VM callbacks and recursive native paths explicitly; use VM fallback for recursion that cannot obey the same bound safely.
- [x] Test deep valid guest recursion and one-beyond-depth failure from the normal embedding thread, not just the CLI's custom thread. Verify exact budget outcomes across VM, JIT and fallback after nested calls.
- [x] Add `http_request_bytes(method: Text, url: Text, headers: Map[Text,Text], body: Bytes, timeout_ms: Int) -> Result[HttpResponse, HttpError]`, with status, headers and Bytes body. Keep text APIs as checked UTF-8 wrappers. Preserve exact webhook body bytes.
- [x] Enforce one monotonic deadline through DNS/connection/headers/body, bounded response accumulation, redirect credential rules and active cancellation. Use one shared native client/executor per host runtime rather than creating a thread pool per request. Keep blocking public semantics while propagating group cancellation to the underlying request.
- [x] Make runtime-supplied URLs and explicit grants easy to coordinate using `veritas inspect --json` diagnostics and a manifest/CLI grant path. Never silently grant arbitrary destinations or hide the need for a grant.
- [x] Exercise real loopback delayed/trickle/oversize responses, cancellation during body read, bounded worker concurrency and ordering, one worker's failure with completed effects, repeated invocation and close, and open native-resource cleanup. Use fixed repeated batches and demonstrate a memory plateau after warm-up; report samples and host noise rather than inventing a universal RSS cap.

**Verify:** `cargo xtask runtime` passes under normal thread stacks on both targets. No residual worker, socket or database handle remains after explicit close/cancellation. Runtime limits remain semantic limits; OS memory limits are reported separately.

### Task 7 — Complete native library dependencies and remove Python from core delivery

**Files:** CLI native_packages/dependencies/compiler_driver/main; CLI/runtime Cargo feature declarations; standard library loader; compatibility docs; new `crates/cli/tests/native_packages.rs`.

**Consumes:** std/pkg resolution and SemVer/TOML services. **Produces:** native dependency locks and repeatable sync/update without Python.

- [x] Add a versioned lock format that can read the existing Python-only `veritas.lock`, preserve its optional Python section, and add a separate native-package graph. `check/run/build` never migrate or update a lock implicitly. Explicit `sync` performs a documented migration transaction with rollback on failure.
- [x] Define native package manifests with exact package version, relative entry, exported modules, dependencies and SHA-256 content identity. Accept local source paths and explicitly configured HTTPS package archives; exact dependency versions are the initial public contract. Detect conflicting versions/cycles rather than silently choosing one. A public registry service and arbitrary build scripts are not prerequisites.
- [x] Implement `veritas add NAME --path PATH` and `veritas add NAME --url URL --version VERSION --sha256 HASH`, `sync --locked --offline`, and explicit `update`. Resolve transitive exact dependencies, canonicalize/hash source contents, reject escaping archives and imports, and publish lock/cache updates transactionally. After acquisition, copy libraries under the managed package cache; builds cannot depend on the original path's continued existence.
- [x] Add `veritas package --library` and prove a separately authored reusable library can be packaged, added to a new project, built offline, updated, and rolled back after an invalid update. Imports must not accidentally resolve from the Veritas development checkout.
- [x] Make `python-interop` a non-default feature/profile for the existing bridge and dependency manager. A core build reports optional external calls as unavailable when requested; core commands must not discover, install, launch or bundle Python/uv.
- [x] Run the existing bridge/managed-dependency regression suite once under the explicit compatibility feature after the split. Preserve that supported path and its error behavior; exclusion from the core acceptance environment is not permission to silently break existing users.
- [x] Remove source-compiler dependencies from normal CLI/runtime builds and verify the dependency graph again. Core package audits must show only the declared runtime/native assets and stdlib/compiler identity.

**Verify:** `cargo xtask product --group dependencies` succeeds offline with developer Python/uv/stage-zero locations inaccessible. Old Python-profile locks are readable and migrate only through the explicit command; failure preserves the prior lock and runnable environment.

### Task 8 — Finish the everyday installed builder workflow

**Files:** CLI tooling/main/compiler_driver, self-host format module, distribution installer, language examples and shipped docs; new `crates/cli/tests/builder_lifecycle.rs`.

**Consumes:** installed self-host compiler, core libraries and dependency manager. **Produces:** one consistent `veritas` interface for create, inspect, repair, run, test and deliver.

- [x] Implement the complete command sequence: `new`, `check`, `fmt`, `lint`, `test`, `run`, `build`, `package`, `verify-package`, `add`, `sync`, `update`, `inspect`, `doc`, `doctor`, and `install`. Keep existing working commands and structured reports; add missing commands without introducing another launcher.
- [x] Replace line-only formatting with token/syntax-aware formatting that preserves comments and string contents and is idempotent. `fmt --check` never writes. Lint initially reports unused imports/locals and unreachable statements with stable codes; correctness errors belong to check. No heuristic rewrite of user programs.
- [x] Generate API docs and capability inspection from the checked compiler representation, including source parameter names, types, effects, stdlib availability and optional profile requirements. Shipped examples are compiled and tested using the installed compiler.
- [x] Normalize command exits, JSON report schema, stderr/stdout separation, and diagnostics. Invalid data must retain file/field/path context. A failed subprocess, test, package verification or application exit must remain a failure in the enclosing command.
- [x] Run a complete new-project exercise from a directory containing spaces and Unicode: create project, add a library, introduce/fix a compiler error using only shipped docs, format/lint/test, build, package, install and invoke by the generated app name. Do not invoke a checkout binary behind the installed command.
- [x] Keep new-project templates minimal. Do not include solved versions of acceptance applications or hide their business logic in the standard library.

**Verify:** `cargo xtask product --group lifecycle` passes using only the installed core product and a pinned offline package cache. Missing Python, uv and Rust on the user PATH must not affect success.

### Task 9 — Build the complete application set through public APIs

**Files:** `evaluation/completion/applications/{sla-report,jsonl-summary,release-notes,calendar-conflicts,webhook-replay,health-digest,config-migrator,doc-index,lockfile-auditor}/`; new completion application runner and tests.

**Consumes:** all library/toolchain work. **Produces:** nine independently runnable Veritas projects with initial and follow-up behavior, ordinary manifests and tests, packaged via the normal command.

- [x] Implement every initial and follow-up requirement from the nine existing briefs. Use native libraries and Veritas application logic. The historical briefs allow Python bridges, but this completion contract does not allow them for these projects.
- [x] Reuse the existing expected-output/oracle logic as regression input; remove evaluator assumptions about root `main.vr`, executable `/tmp`, or hidden developer runtimes in the new runner. Keep the historical runner/results unchanged. Assert correct delivery working-directory semantics and resource access.
- [x] Exercise real input directories, CSV/JSONL files, SQLite databases, TCP loopback servers, complete HTTP response bodies, moved package directories and separate process restarts. Unit doubles may test isolated failure branches; no simulated filesystem/network/database satisfies this application gate.
- [x] Add deterministic generated inputs with recorded seeds and multiple sizes, malformed data and adversarial boundary cases. Calculate expected results independently of the Veritas implementation. Include quoted CSV fields, strict timestamp failures, VTIMEZONE transitions, original-byte signatures, concurrent transaction behavior, Unicode aliases, TOML comment/no-op preservation and whole-directory validation before filtering.
- [x] Ensure fixes belong in reusable libraries where appropriate. A workload-local workaround for a missing required library API is an unfinished task, even if its output happens to pass.
- [x] Exercise maintainability: apply each documented follow-up as a real source edit, rerun old behavior, then package the changed application. Record changed files and any runtime/stdlib changes needed. A runtime change for an application-specific rule is a design defect to remove.
- [x] Rerun full self-host compiler conformance and stage equality after all new APIs. Run one consolidated engineering review of the actual product; resolve correctness/delivery issues in this same plan.

**Verify:** `cargo xtask product --group applications --stage initial,follow-up --target both` passes every required acceptance case with zero hidden-Python use. Checkpoint 2 reports all nine applications, not only the easiest successes.

### Task 10 — Package once, verify both targets, and close the speed goal

**Files:** distribution scripts/installer/auditors and native component manifest; xtask release/performance; `evaluation/completion/performance/{manifest.json,runner,inputs}/`; final installed acceptance tests.

**Consumes:** Task 9 product. **Produces:** one canonical core distribution per supported target and reproducible final performance evidence.

- [x] Move core packaging/license/dependency/archive audit work needed by a source build into Rust tooling. Existing Python audit scripts may remain historical or optional external verifiers; they are not a core installer, compiler, or package dependency.
- [x] Build matching macOS ARM64 and Debian-compatible Linux ARM64 releases from the same source state. Bundle compiler seed, standard library, target-native dependencies, timezone/Unicode data, licenses, local docs and metadata. Record actual OS/libc constraints. Signing/notarization, Windows, browser and phone deployment are outside this previously established target contract.
- [x] Run installer and source-free application relocation from a path with spaces/Unicode, with original source and development caches inaccessible and the candidate environment unable to execute Python/uv. Verify read-only install roots plus writable caller data, changed working directory, artifact tampering, dependency mismatch and repeated launches. Inspect actual dynamic library dependencies, including libical/SQLite support assets.
- [x] Establish the final performance manifest before timing: existing scheduler/numeric/image/parallel workloads, durable checked-in JSON analysis input generator, and actual CPU/data phases of the native applications. Preserve original required arithmetic, parsing, validation and output semantics. Retain original upstream source provenance and label translations accurately.
- [x] Measure source-run end-to-end latency, prebuilt-artifact execution, long-lived warm execution, peak memory and package size separately. Use a release build, fixed inputs, alternating/randomized candidate order and the same host/session. Fifteen measured trials and existing 16-pair data trials are starting sample sizes; retain all samples and bootstrap intervals, with seed/method recorded.
- [x] Compare to the pinned CPython baseline and reasonable idiomatic Python implementations, including a native-backed data approach when relevant. Compare CPU concurrency to threads, processes and a verified free-threaded CPython build; do not present a GIL-only win as the whole current-Python comparison. If that baseline is unavailable, finish other work and report the specific concurrency claim as unverified.
- [x] G2 completion requires correct outputs and reproducible speed wins on the established scheduler/numeric/image/parallel compute set and the real data workload, with a 95% interval for the median Veritas/Python time ratio below 1 for each advertised win. Report ties and regressions. Network waits are not evidence of compute speed. If a required workload fails, profile the real installed path, repair the shared bottleneck and rerun the affected workload plus impacted correctness checks.
- [x] Reverify hashes after execution; normal runs must not mutate immutable package contents. Keep one current final artifact per target plus the prior recovery artifact. Do not generate a new directory for every internal compile.

**Verify:** `cargo xtask release --target both` and `cargo xtask performance --installed` pass required release checks and produce complete raw reports. Clean-room language/product independence must be demonstrated; a PATH-only assertion is insufficient if another executable path still reaches Python or stage zero.

### Task 11 — Fresh builder evidence and one final handoff

**Files:** `evaluation/completion/builder/{protocol.md,assignments.json,reports}/`, `docs/completion-report.md`, README and shipped documentation; no edits to frozen independent-v1/v2/v3 records.

**Consumes:** final installed release and evidence from Tasks 1–10. **Produces:** a bounded fresh evaluation, explicit recommendation scope, and a maintainable release handoff.

- [x] Before dispatch, record three unfamiliar application briefs, one per target class, with correctness/change/delivery criteria and a fixed time/resource budget. They must differ materially from the nine known tasks. The implementing builder sees shipped docs and a brief, not a prewritten solution or comparative benchmark conclusions. Preserve assignments and failures.
- [x] Run one fresh matched Python/Veritas implementation and one follow-up change per brief, sequentially, in the existing environment. This is three matched pairs, not another 18-pair campaign. Match permissions, runtime provisioning, available dependencies and delivery requirements. No author rescue is hidden; record every intervention and required product change.
- [x] Give each language builder a separate fresh context, use the same recorded model/reasoning/tool configuration and budget, randomize execution order before starting, and prevent the second builder from seeing the first solution. A separate final assessment can inspect both outputs. Record setup time, build/repair time, installed execution time and human interventions as different measures.
- [x] Ask each fresh builder/reviewer for an explicit recommendation **after** examining correct installed applications and observed setup, repair, delivery and maintenance effort. Record time, intervention count, failures, artifact footprint and runtime performance separately. Reward neither a faster wrong program nor familiarity alone.
- [x] For each target class, require successful Veritas initial/change/relocation behavior and a reasoned preference supported by at least one observed user benefit without an undisclosed material regression. If Python is preferred, map the specific reason back to Tasks 2–10 and implement the missing capability before re-evaluating that class with a new brief. Do not repeat votes against an unchanged product or turn this small sample into a statistical adoption claim.
- [x] `cargo xtask closeout` verifies G1/G2/G3 evidence links, final hashes, all required application passes, stage equality, release target parity and open-defect status. A material regression, skipped required case, Python-dependent core feature or unsupported recommendation leaves the corresponding goal open.
- [x] Produce one final report with exact achieved scope, command sequence, source/seed/runtime/stdlib hashes, supported platforms, remaining nonblocking limitations, installation/update/removal instructions, reproducible verification commands and maintenance ownership. Commit the completed source/evidence as one release state without discarding prior work; publication or signing still needs explicit authorization.

**Verify:** `cargo xtask closeout` reports all three goals satisfied for the declared scope. If not, its output names the failing task and next implementation action. It must never say the full build is complete because only the compiler or benchmark checkpoint passed.

## 7. Review focus and tests that own it

| Failure likely to escape happy-path examples | Required owner/check |
|---|---|
| Compiler silently accepts wrong types, private imports, or unsupported syntax | Task 2 negative conformance and cycle/visibility cases; Task 3 no-fallback installed compiler test |
| Moved package reads the wrong directory or unexpectedly needs developer Python | Tasks 7–10 offline dependency, external caller data and blocked-executable relocation checks |
| Bad record destroys an existing output/index or aborts an entire batch | Task 4 recoverable JSON/JSONL cases; Task 5 rollback tests; Task 9 malformed-input application runs |
| Cancellation/deep calls leak resources or crash the host | Task 6 ordinary-thread deep-call, native fallback, trickle-response and lifecycle checks; Task 5 handle rollback |
| Formats are subtly wrong: timezone ambiguity, Unicode collisions, TOML no-op drift, signature bytes | Task 5 pinned data/conformance vectors and Task 9 full applications; dependency limitations explicitly tested |

## 8. Work order and recovery rules

Default sequence: **1 → 2 → 3 → 4 → 5 → 6 → 7 → 8 → 9 → 10 → 11**. A small byte primitive needed by Task 3 may be pulled forward from Task 4 with its tests. If full compiler operation exposes a host-stack failure, pull forward Task 6's explicit guest-frame work rather than enlarging the stack workaround. Record the dependency in this plan. Compiler/host/library changes are updated in both compiler paths until stage zero can remain a frozen seed. Rebootstrap after a compiler change, not after every unrelated documentation edit.

The largest implementation work is full compiler semantics, native library/resource services, and installed toolchain integration. Do not describe them as already finished or as a few trivial patches. Existing code and mature native libraries reduce work; they do not remove it. This plan sets no unsupported elapsed-time promise.

At any interruption, update only the active task/status record and the commands/evidence necessary to resume. A failed targeted check keeps its owning task active. Do not restart all tests or re-audit the machine. The user receives checkpoint updates and material architectural findings; implementation continues under the existing authorization.

### Ready-to-use execution instruction

> Implement `docs/superpowers/plans/2026-10-24-veritas-complete-product.md` in the existing Veritas checkout. Begin at the first unchecked task and preserve all current changes. Deliver the full self-hosted compiler, native standard library, installed core workflow, nine real applications, both supported target packages, and final performance/builder evidence. No core feature may delegate to Python or the Rust source compiler. Use one workspace, bounded processes, retained pinned toolchains and one build cache. Run meaningful checks for each changed subsystem and the three defined checkpoints; do not start another RC/review/18-pair cycle. After compaction resume the recorded next command. Do not mark any goal complete on subset evidence, and do not stop after a checkpoint while authorized implementation remains.

## 9. References and dependency caveats

- Local authority: `language/spec.md`, `language/host-api.md`, `docs/north-star-design.md`, `docs/compatibility-and-migrations.md`, and the source files named above.
- Recommendation evidence: `evaluation/results/independent-v3/choice-summary.json` and all 18 recommendation JSON files. Existing brief requirements are in `evaluation/briefs/independent/`.
- [rusqlite official API](https://docs.rs/rusqlite/latest/rusqlite/) supplies typed SQLite access. The final lock must bind an exact compatible version and bundled SQLite identity.
- [toml_edit official documentation](https://docs.rs/toml_edit/latest/toml_edit/) documents comment-preserving editing but also dotted-key ordering and final-newline limitations. Task 5 explicitly tests and covers those gaps rather than assuming perfect preservation.
- [RustCrypto HMAC](https://github.com/RustCrypto/MACs/tree/master/hmac) is the selected HMAC implementation family; use its verification API and pin a version compatible with the selected SHA-256 crate.
- [ICU case folding documentation](https://unicode-org.github.io/icu/userguide/transforms/casemappings.html) distinguishes full case folding from lowercase conversion; [ICU4X case mapping](https://docs.rs/icu_casemap/latest/icu_casemap/) supplies the native implementation family. Pin its data version.
- [libical upstream](https://github.com/libical/libical) and its [timezone component API](https://libical.github.io/libical/v4/apidocs/icalcomponent_8h.html) provide the calendar integration direction. The final version/build/API/license must be recorded; these reference URLs are not an instruction to depend on a moving development branch.

## 10. Plan self-review

- G1 is implemented by Tasks 1–3 and revalidated after Tasks 4–9; normal CLI dependency removal and full artifact emission prevent subset-only self-hosting claims.
- G2 is implemented by shared runtime work and Task 10 against the final installed build, with correctness, baseline parity and raw measurements required.
- G3's product gaps each map to Tasks 4–8, real consumers to Task 9, and fresh evidence to Task 11. The old negative choices are retained and used to drive implementation.
- The existing lock filename, large-stack workaround, Python-based distribution scripts, current module semantics, missing metadata, native library/data packaging and TOML preservation limitations have explicit owners.
- No test fixture, module path, host library, example or benchmark is allowed to substitute for an ordinary installed workflow. Old release cleanup and further research are not on the critical path.
- All named new interfaces and files are planned, not claimed present. Each task has a checkable deliverable, concrete negative cases and one verification entry point. New dependency versions are resolved and pinned once during implementation; none are silently asserted installed today.
