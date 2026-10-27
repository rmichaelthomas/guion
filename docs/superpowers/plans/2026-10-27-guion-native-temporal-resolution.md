# Guion Native Temporal Resolution Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give Guion a built-in, reusable way to report whether a local date and time in a named timezone happens once, never, or twice, and make calendar parsing preserve that result without choosing for the caller.

**Architecture:** Keep the resolver pure and available from `std:time`. Use Guion's existing pinned libical and packaged tzdata to calculate zero, one, or two valid UTC candidates, then return typed Guion records. Route calendar event times through the same classification contract, preserve event identity and source values, and carry the new operation through artifact encoding, runtime, compiler, and self-hosted compiler metadata.

**Tech Stack:** Rust workspace, C libical bridge, Guion standard library, JSON signature catalog, serialized artifact format, existing Cargo tests and conformance runner.

**Spec:** `docs/superpowers/specs/2026-10-27-guion-native-temporal-resolution-design.md`

## Global Constraints

- Preserve all three classifications: `exact` has one candidate, `gap` has none, and `fold` has two distinct candidates in ascending order. Never pick a fold occurrence or shift a gap.
- Use the existing packaged timezone data (`2026c` at the design baseline). The resolver and `calendar_parse` remain pure for text input; file reads remain explicit caller capabilities.
- Keep embedded `VTIMEZONE` precedence, floating-time and unknown-zone errors, all-day dates, and recurrence text behavior. Do not expand recurrence rules or add a Decan runtime dependency.
- Preserve source date/time, timezone, UID, summary, status, and error identity through native and Guion values. Do not retain a public primary start/end field that silently collapses an ambiguous value to one `Instant`.
- Treat the public `CalendarEvent` field change as a pre-release breaking change: search and migrate repository consumers before changing it.
- Keep new artifact opcodes append-only, advance the artifact format, update all decoders and version guards, and regenerate the checked self-host seed from the documented offline command.
- Do not add unrelated features, implicit file/network access, or user-defined variant syntax.

## Review Focus

- Candidate generation must not depend on libical's current implicit fold/gap policy; test zero, one, and two candidates, including embedded `VTIMEZONE`.
- Public types and JSON output must retain source identity and typed error codes without leaking an arbitrary policy choice.
- Calendar end-before-start validation must not reintroduce a hidden choice: compare instants only when both endpoint resolutions are exact; for a gap or fold, preserve both classifications without claiming a single duration or ordering.
- Type checking and execution must agree for the new native operation, including error paths and artifact-format rejection of old-format artifacts containing the new opcode.
- Regenerated catalog and self-host seed must be reproducible and the self-hosted compiler must continue to agree with the Rust compiler.

---

### Task 1: Pin the New Semantics with Failing Runtime Fixtures

**Files:**
- Modify: `crates/runtime/tests/stdlib_services.rs`
- Modify: `crates/runtime/tests/stdlib_data.rs`
- Modify: `crates/cli/tests/stdlib_data.rs`

**Interfaces:**
- Public resolver contract: `resolve_local(local: LocalDateTime, timezone: Text) -> Result[TemporalResolution, TimeError]`.
- `LocalDateTime` fields: `year`, `month`, `day`, `hour`, `minute`, `second`, `microsecond` (all `Int`).
- `TemporalResolution` fields: `kind: Text`, `local: LocalDateTime`, `timezone: Text`, `instants: List[Instant]`.
- `TimeError` fields: `code: Text`, `message: Text`.
- Assert `exact` produces one UTC instant, `gap` produces no instants, `fold` produces both ordered candidates, and invalid fields/unknown zones produce stable typed errors.

- [ ] **Step 1: Add focused failing fixtures** for Los Angeles 2026-03-08 02:30 (`gap`), 2026-11-01 01:30 (`fold`: 08:30Z then 09:30Z), an ordinary exact value, invalid calendar fields, and an unknown timezone. Add equivalent calendar fixtures proving source values and event identity survive and a gap/fold does not acquire a chosen duration.
- [ ] **Step 2: Run the focused tests** with `/Users/rmichaelthomas/.cargo/bin/cargo test --locked --offline -p guion-runtime --test stdlib_services` and `/Users/rmichaelthomas/.cargo/bin/cargo test --locked --offline -p guion-cli --test stdlib_data`; verify the new assertions fail because the API and behavior do not yet exist, while existing unrelated assertions remain identifiable.
- [ ] **Step 3: Add a focused embedded-`VTIMEZONE` fixture** with a repeated local time and assert that its two candidates follow the embedded rules even when the TZID matches an IANA zone; rerun the focused runtime test and confirm it fails for the missing classification behavior.

### Task 2: Add the Typed Resolver to the Language and Artifact Contract

**Files:**
- Modify: `language/stdlib/time.gu`
- Modify: `language/stdlib/types.gu`
- Modify: `language/builtin-signatures.json`
- Modify: `language/self_host/builtin_catalog.gu`
- Modify: `language/self_host/artifact.gu`
- Modify: `crates/model/src/lib.rs`
- Modify: `crates/compiler/src/builtin_catalog.rs`
- Modify: `crates/cli/src/bytecode_ir.rs`
- Modify: `crates/cli/tests/artifact_writer.rs`
- Modify: `crates/vm-core/src/lib.rs`
- Modify: `language/artifact-format.md`

**Interfaces:**
- Add `resolve_local(LocalDateTime, Text) -> Result[TemporalResolution, TimeError]` and `time_zone_data_version() -> Text` to `std:time`.
- Add native ops `TimeResolveLocal` and `TimeZoneDataVersion` at the next two append-only opcode tags (158 and 159); bump `Artifact::CURRENT_VERSION` from 39 to 40.
- Add format-version guards so format-39 artifacts cannot contain either new op; preserve decoding of previously supported artifacts.

- [ ] **Step 1: Add model tests** for opcode round-trip tags 158/159, format 40 encode/decode, and rejection when either opcode appears in a format-39 artifact; run `/Users/rmichaelthomas/.cargo/bin/cargo test --locked --offline -p guion-model` and confirm the new tests fail before implementation.
- [ ] **Step 2: Add a compiler/VM typing test** showing the exact resolver signature type-checks and a malformed argument type is rejected; run `/Users/rmichaelthomas/.cargo/bin/cargo test --locked --offline -p guion-compiler --test pipeline` and `/Users/rmichaelthomas/.cargo/bin/cargo test --locked --offline -p guion-vm-core --test portable_execution`, confirming the positive case currently fails to compile.
- [ ] **Step 3: Implement the append-only model and VM contract**: opcode variants, serialization/deserialization, artifact-version checks, static stack signatures, compiler mapping, IR assembler mapping, and format documentation; keep all older opcode tags unchanged.
- [ ] **Step 4: Add the JSON signatures and self-host catalog/compiler mappings**, then regenerate the checked catalog with `/Users/rmichaelthomas/.cargo/bin/cargo xtask catalog --write` and verify it is clean on a second invocation.
- [ ] **Step 5: Run** `/Users/rmichaelthomas/.cargo/bin/cargo test --locked --offline -p guion-model`, `/Users/rmichaelthomas/.cargo/bin/cargo test --locked --offline -p guion-compiler --test pipeline`, `/Users/rmichaelthomas/.cargo/bin/cargo test --locked --offline -p guion-vm-core --test portable_execution`, **and** `/Users/rmichaelthomas/.cargo/bin/cargo test --locked --offline -p guion-cli --test artifact_writer`; require the resolver signature to compile and artifact compatibility checks to pass before adding runtime behavior.

### Task 3: Implement Pure Timezone Candidate Resolution

**Files:**
- Modify: `native/libical_bridge.c`
- Modify: `crates/runtime/src/stdlib_services.rs`
- Modify: `crates/runtime/src/lib.rs`
- Modify: `crates/runtime/tests/stdlib_data.rs`
- Modify: `crates/cli/tests/stdlib_data.rs`

**Interfaces:**
- Native resolver accepts civil fields plus an IANA TZID and returns a classification code and zero to two UTC-second candidates, with an error code/message for invalid input or unknown zone.
- Runtime operation consumes a `LocalDateTime` record and timezone text; it returns `Ok(TemporalResolution)` for exact/gap/fold and `Err(TimeError)` for invalid fields or unknown zones.
- `time_zone_data_version() -> Text` returns the pinned package version (`2026c` at this repository baseline) without I/O.

- [ ] **Step 1: Inspect the pinned libical timezone APIs and select a complete candidate-enumeration method** from its timezone transition representation; require exact local round-trips, candidate deduplication, and sorting. Record why the method covers the relevant transition rules; do not assume the old sparse probe list is complete or call the old single-result policy as the answer.
- [ ] **Step 2: Replace the single-result C helper with a bounded candidate result** and unit-test that the native boundary returns exactly 0/1/2 candidates for the Task 1 gap/exact/fold fixtures, plus deterministic errors for invalid civil fields and unknown zones.
- [ ] **Step 3: Add Rust service conversion and runtime record construction** for typed `TemporalResolution`/`TimeError`, checking microsecond and integer bounds and preserving the original fields exactly.
- [ ] **Step 4: Execute Guion source fixtures through the compiled CLI** and assert typed results, stable error codes, and `time_zone_data_version() == "2026c"`; verify no capability grant or filesystem/network call is recorded.
- [ ] **Step 5: Run** `/Users/rmichaelthomas/.cargo/bin/cargo test --locked --offline -p guion-runtime --test stdlib_data` **and** `/Users/rmichaelthomas/.cargo/bin/cargo test --locked --offline -p guion-cli --test stdlib_data`; require exact/gap/fold/error assertions to pass.

### Task 4: Route Calendar Parsing Through the Same Resolution Contract

**Files:**
- Modify: `native/libical_bridge.c`
- Modify: `crates/runtime/src/stdlib_services.rs`
- Modify: `crates/runtime/src/lib.rs`
- Modify: `language/stdlib/types.gu`
- Modify: `language/stdlib/calendar.gu`
- Modify: `crates/runtime/tests/stdlib_services.rs`
- Modify: `crates/cli/tests/stdlib_data.rs`

**Interfaces:**
- Public `CalendarEvent` becomes `uid`, `summary`, `start_resolution`, `end_resolution`, `start_date`, `end_date`, `status`, `all_day`, and `recurrence_rule`.
- `start_resolution` and `end_resolution` are `Option[TemporalResolution]`; all-day events continue to use date fields and have no timed resolutions.
- Calendar errors remain `Result[List[CalendarEvent], StdError]`, all-or-nothing, first source-order error, with UID where already available.

- [ ] **Step 1: Replace the old public calendar test expectations** (the fold-duration and zero-length-gap assertions) with expected explicit `fold`/`gap` values and tests for summary, UID, original local fields, timezone, RRULE, all-day dates, and error identity; verify the current implementation fails these assertions.
- [ ] **Step 2: Change the native event boundary to return each endpoint's local fields, TZID, classification, and candidates independently**, preserving embedded `VTIMEZONE` precedence and UTC exactness; verify this through Rust FFI service tests and do not convert a fold or gap to one timestamp.
- [ ] **Step 3: Update Rust service types and `service_calendar_event`** to construct the new public record in the declared field order, including summary and separate all-day date data.
- [ ] **Step 4: Preserve ordering validation only when both endpoint resolutions are exact**; when either endpoint is a gap or fold, preserve both classifications and avoid claiming a single duration/order. Add fixtures for exact reversed endpoints and non-exact endpoints.
- [ ] **Step 5: Run** `/Users/rmichaelthomas/.cargo/bin/cargo test --locked --offline -p guion-runtime --test stdlib_services` **and** `/Users/rmichaelthomas/.cargo/bin/cargo test --locked --offline -p guion-cli --test stdlib_data`; require existing UTC, date-only, embedded-zone precedence, RRULE, floating-time, unknown-zone, malformed-input, and new gap/fold coverage to pass.

### Task 5: Publish the API, Version, and Agent-Usable Examples

**Files:**
- Modify: `language/host-api.md`
- Modify: `language/stdlib/time.gu`
- Modify: `language/stdlib/calendar.gu`
- Modify: `language/stdlib/types.gu`
- Modify: `crates/cli/src/main.rs`
- Modify: `crates/cli/tests/cli.rs`
- Modify: `language/conformance/manifest.json`
- Create: `docs/agents/examples/native_temporal_resolution.gu`
- Modify: `docs/agents/index.json`
- Modify: `CHANGELOG.md`

**Interfaces:**
- CLI version output includes the packaged timezone-data version; `std:time` documents `resolve_local` and `time_zone_data_version`.
- The example demonstrates exact/gap/fold and an explicit caller policy branch, without choosing on the caller's behalf inside the resolver.
- Conformance records mark the new operations as implemented in the hosted and self-hosted compiler subsets once both are verified.

- [ ] **Step 1: Add failing CLI/API documentation checks** for timezone-data version reporting, standard-library signature visibility, and example registration; run `/Users/rmichaelthomas/.cargo/bin/cargo test --locked --offline -p guion-cli --test cli` and `/Users/rmichaelthomas/.cargo/bin/cargo test --locked --offline -p guion-cli --test language_conformance -- --test-threads=1`, confirming the new assertions fail.
- [ ] **Step 2: Write the public API text and runnable example**, documenting stable error codes, packaged tzdata versioning, the pure/no-I/O behavior, and the absence of an automatic gap/fold policy.
- [ ] **Step 3: Add conformance manifest entries** for correct generic exact/gap/fold results, typed errors, and calendar preservation; run `/Users/rmichaelthomas/.cargo/bin/cargo test --locked --offline -p guion-cli --test language_conformance -- --test-threads=1`.
- [ ] **Step 4: Add the timezone-data version to `guion --version` metadata** and assert the exact format in CLI tests; preserve the existing artifact-format value.
- [ ] **Step 5: Run the example with the default CLI** and verify the output records all three classifications, original inputs, stable errors, and the pinned timezone-data version.

### Task 6: Regenerate Self-Hosted Metadata and Run Compatibility Gates

**Files:**
- Modify: `language/self_host/compiler-seed.json` (generated)
- Modify: `language/self_host/builtin_catalog.gu` (generated if catalog regeneration changes it)
- Modify: `language/self_host/artifact.gu`
- Modify: `language/artifact-format.md`
- Existing tests: `crates/model`, `crates/compiler`, `crates/vm-core`, `crates/runtime`, `crates/cli`

**Interfaces:**
- Regenerate the compiler seed using the repository's supported offline command: `/Users/rmichaelthomas/.cargo/bin/cargo run --locked --offline -p guion-bootstrap --bin guion-stage0 -- seed .`.
- The stage-zero compiler, hosted compiler, and self-hosted compiler agree on artifacts that contain the new native operations.

- [ ] **Step 1: Update self-hosted opcode/version tables** and run the seed-regeneration command; inspect the generated diff and require the source/schema/artifact hashes to match their inputs.
- [ ] **Step 2: Run focused tests** for `guion-model`, `guion-compiler`, `guion-vm-core`, `guion-runtime` service/data targets, and `guion-cli` data/services/CLI/conformance targets; fix any failures before the full corpus.
- [ ] **Step 3: Run** `/Users/rmichaelthomas/.cargo/bin/cargo xtask conformance --compilers stage2,stage3` **and** `/Users/rmichaelthomas/.cargo/bin/cargo test --locked --offline --workspace`; require the compiler pipeline, portable VM, runtime edge cases, bootstrap equality, and language corpus to pass.
- [ ] **Step 4: Run** `/Users/rmichaelthomas/.cargo/bin/cargo fmt --all -- --check` **and** `/Users/rmichaelthomas/.cargo/bin/cargo clippy --locked --offline --workspace --all-targets -- -D warnings`; correct formatting/lint failures and rerun the failed gate.
- [ ] **Step 5: Review the complete diff** for unchanged legacy opcode tags, no implicit effects, no Decan dependency, no stale public `CalendarEvent.start/end` consumers, and no test that still blesses single-choice gap/fold behavior; record the exact pass/fail commands and commit only intended implementation, tests, docs, and generated seed files.

## Plan Self-Review

- **Spec coverage:** The resolver contract, data version, packaged timezone rules, calendar field migration, embedded-zone precedence, stable errors, caller-owned policy, docs/example, and study readiness each have an owning task.
- **Step scan:** Each implementation path begins with a failing semantic or interface test before code changes; every task names its verification command and expected outcome.
- **Interface consistency:** Public names and record fields are fixed above and used consistently by runtime, calendar, docs, and conformance fixtures. The two native operations consume no ambient capability.
- **Review focus:** The five highest-risk issues—candidate enumeration, source identity, order validation, static/runtime typing, and bootstrap parity—are explicitly tested or inspected.
- **Proportion:** This changes one bounded standard-library capability and its calendar integration. Natural-language scheduling, RRULE expansion, and broader type-system changes remain outside scope.
