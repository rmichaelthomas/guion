# Guion-native temporal reasoning

**Status:** Draft for owner review. No implementation changes are included.  
**Repository basis:** Guion main snapshot 0824e0e (2026-10-26); Decan main snapshot 3a5285d (2026-10-16).

## Intent

Make Guion a language an agent can reach for when a program must interpret local dates and times correctly. Guion should provide the temporal resolver itself as part of its standard library. An agent should not have to install or call Decan to get honest results for times that occur once, do not occur, or occur twice.

Decan contributes a clear semantic lesson: report what the local time means before an application chooses a policy. A nonexistent local time is a gap with no valid instant. A repeated local time is a fold with both possible instants. Guion can adopt that contract and its adversarial fixtures while keeping its own implementation and public API.

## Current behavior and evidence

Guion already has a built-in iCalendar parser backed by pinned libical and packaged timezone data. The dependency lock names tzdata 2026c. A new user-facing adapter or dependence on the host computer’s timezone database is unnecessary for this first change.

The current calendar parser immediately converts a local start and end into one UTC timestamp. The native bridge returns one start and one end value; it does not return the original local time, timezone identifier, or ambiguity classification. The public Guion CalendarEvent record has start and end as optional single Instants and does not include summary, even though the native parser has it and the host API documentation says it is returned.

The focused runtime test currently expects a fall-back event to have a two-hour duration after the parser chooses one interpretation. It expects a spring-forward event at a nonexistent time to have equal start and end instants. That test passes, but its expectations encode the behavior this design changes.

Evidence references:
- Guion host API: https://github.com/rmichaelthomas/guion/blob/0824e0e/language/host-api.md
- Guion public types: https://github.com/rmichaelthomas/guion/blob/0824e0e/language/stdlib/types.gu and https://github.com/rmichaelthomas/guion/blob/0824e0e/language/stdlib/time.gu
- Guion native calendar service and runtime conversion: https://github.com/rmichaelthomas/guion/blob/0824e0e/crates/runtime/src/stdlib_services.rs and https://github.com/rmichaelthomas/guion/blob/0824e0e/crates/runtime/src/lib.rs
- Current calendar edge test: https://github.com/rmichaelthomas/guion/blob/0824e0e/crates/runtime/tests/stdlib_services.rs
- Bundled dependency and timezone-data version: https://github.com/rmichaelthomas/guion/blob/0824e0e/distribution/native-components.lock.json
- Decan gap/fold contract tests: https://github.com/rmichaelthomas/decan/blob/3a5285d/tests/post_c6/resolve-series.test.ts

## Goals

1. Add a reusable Guion standard-library operation that resolves a local civil date and clock time against a named timezone and returns an exact, gap, or fold result.
2. Represent an exact local time as one instant, a gap as no instant, and a fold as both possible instants in chronological order.
3. Make calendar_parse use the same semantics and preserve the original civil time, timezone identifier, event UID, summary, status, and recurrence text. Do not expand recurrence rules in this change.
4. Keep malformed values, unknown timezones, and floating calendar times as explicit errors with stable error codes.
5. Keep timezone behavior deterministic across machines by using Guion’s pinned, packaged timezone data. Make the data version discoverable for reproducibility.
6. Keep parsing pure when the caller provides calendar text. Reading a file remains an explicit file capability; no implicit filesystem or network access is added.
7. Make the behavior easy for an agent to discover from Guion’s standard-library docs, CLI help, and a runnable example.

## Proposed public behavior

Add public records for a local civil date-time, its resolution, and temporal errors. The suggested public shape is:

    LocalDateTime: Date plus hour, minute, second, and microsecond fields
    TemporalResolution: kind, original LocalDateTime, timezone, candidate Instants
    TimeError: stable code and human-readable message

Add a standard-library operation in std:time, provisionally named resolve_local(local, timezone), with a signature equivalent to Result[TemporalResolution, TimeError]. It returns a typed resolution for a valid timezone and an explicit error for invalid local fields or an unknown timezone. Its timezone rules come from Guion’s packaged IANA data. The operation is pure and performs no I/O. Add a time_zone_data_version() query, or equivalent standard CLI/runtime metadata, so reports can identify the rules used.

The runtime guarantees these invariants:
- exact has exactly one instant;
- gap has no instants;
- fold has exactly two distinct instants in ascending order.

A gap or fold is a valid classification, not a parse error. The resolver must never silently pick one side of a fold or shift a gap. A caller that wants to choose an occurrence or adjust a gap must make that policy explicit. Guion currently supports records, Option, and Result but not general user-defined variants; for this first version use a typed public record with a documented closed label and typed candidates. Do not add new variant syntax as hidden scope. If implementation review finds this representation too weak, present the smallest type-system addition separately before implementing it.

Expose the packaged timezone-data version through a small std:time query or equivalent CLI/runtime metadata. This is for reproducible reports and tests; callers should not have to inspect an internal lock file.

Update calendar_parse to use this semantic contract. Its timed start and end fields must expose an optional TemporalResolution instead of a single chosen Instant. Preserve all-day events as dates rather than manufacturing instants. Preserve UTC timestamps as exact single-instant values. Preserve an event’s UID and summary even when its local start or end is a gap. Preserve RRULE text without expanding it. Floating times without TZID, unknown zones, and malformed calendar data remain explicit errors. The public CalendarEvent record should include summary and separate all-day date fields from timed resolutions.

For iCalendar input with an embedded VTIMEZONE, preserve the current precedence rule: the embedded rules win over an IANA zone with the same TZID. Use those embedded rules to classify exact, gap, and fold values with the same output invariants. The generic std:time resolver needs only named IANA timezones; the calendar parser continues to handle embedded VTIMEZONE components internally.

## Implementation boundaries

Use the existing Guion time and calendar modules, native bridge, pinned libical build, and packaged timezone data as the starting point. The native bridge must expose the original civil value and enough timezone information to classify it and return all valid candidate instants. The Guion runtime conversion must preserve that information instead of reducing it to one integer timestamp. Exact candidate-generation logic is left to implementation review, but it must not depend on libical’s implicit choice for an ambiguous or missing time.

Use Decan’s behavior and adversarial fixtures as a semantic reference and independent oracle. Do not add a Decan runtime dependency or copy its whole implementation. Review any direct code reuse for license and maintenance fit; prefer a small Guion-native implementation over a broad port.

The calendar record change is intentionally breaking before a public Guion release: replace the misleading single-instant start/end view with explicit start/end resolutions, after searching all call sites and migrating them. Do not retain a primary field that silently chooses one instant. Guion’s current README describes a private pre-release, but that does not prove there are no local consumers.

Likely affected surfaces are the std:time public records and native operation, calendar service and native libical bridge, public CalendarEvent type, runtime value conversion, host API documentation, conformance manifest, CLI examples, tests, and release notes.

## Acceptance checks for the implementation

- Unit and CLI tests cover exact, gap, and fold results using the same pinned timezone data.
- A local time in America/Los_Angeles on 2026-03-08 at 02:30 returns gap with no instant.
- A local time in America/Los_Angeles on 2026-11-01 at 01:30 returns fold with 08:30Z and 09:30Z in order.
- A start in the fold and an exact end remain independently represented; no duration is silently chosen.
- A fixture verifies UID, summary, local value, and timezone survive into Guion values and JSON output.
- Generic IANA resolution and embedded VTIMEZONE resolution obey the same cardinality and ordering contract.
- Existing behavior for UTC events, all-day events, embedded VTIMEZONE precedence, RRULE text retention, floating-time errors, unknown-zone errors, and malformed dates is covered.
- The timezone-data version is observable in the documented reproducibility surface.
- CLI reports preserve the existing file-read effect when a calendar source is read from disk and add no new implicit effects.
- The old tests that bless a chosen fold and zero-length gap are replaced with the new contract.
- Docs include runnable examples for the generic resolver and calendar parser and explicitly say no choice is made for a fold.

## Non-goals for this first increment

- Natural-language scheduling interpretation.
- Expanding RRULE or producing future occurrences.
- Automatically choosing earlier/later or shifting a gap forward/backward.
- Replacing the calendar parser or shipping Decan inside Guion.
- Claiming agent preference from implementation correctness alone.

## Design calls and builder details

- **Design call:** keep timezone data packaged and versioned with Guion. The current repository already does this; it gives stable results without asking users to configure their operating system. Update that data through Guion releases.
- **Design call:** make the capability native at the Guion API boundary, with Decan used as a source of semantics and test cases rather than a user-installed dependency.
- **Design call:** add the generic std:time resolver as the first-class feature; calendar_parse then uses it. This makes the behavior available outside iCalendar.
- **Design call:** keep calendar_parse all-or-nothing, matching its current Result[List[CalendarEvent], error] shape. Return the first error in source order and include the UID when it has already been read. Partial-event recovery can be designed separately if a real consumer needs it.
- **Builder detail:** settle exact field names and operation signatures within the current type system, and search all examples and consumers before migrating the pre-release record.
- **Builder detail:** confirm the best libical-supported way to derive zero, one, or two valid candidates from the original civil time without relying on its default policy.
- **Open for owner review:** this first increment is bounded to structured local date-times and iCalendar. Natural-language temporal intent and recurrence expansion remain later investments.

