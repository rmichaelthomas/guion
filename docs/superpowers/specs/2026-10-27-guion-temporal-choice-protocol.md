# Controlled Guion temporal choice and recommendation protocol

**Status:** Approved by owner; study not run.
**Run only after:** the Guion temporal design is approved, implemented, passes its deterministic fixtures, and is available in the same installable workspace used by the agents.

## Purpose

Find out whether independent coding agents reach for Guion when free to choose a language for work involving local dates and time zones. Record why they say they chose a language without confusing that explanation with what they actually did. Measure assigned-language performance separately.

The earlier pilot is a baseline only. Three agents were shown Guion, Python 3, and TypeScript and chose Python for a validator. All three assigned-language controls passed. The input already labeled each record exact, gap, or fold, so the task did not require temporal resolution. The study asked no post-task rationale. Do not pool those results with this experiment.

## Research questions

1. Which language appears in the first runnable implementation when no language is assigned and the prompt does not name candidate languages?
2. Can agents produce the correct exact, gap, and fold results from original local date/time and timezone values?
3. Does Guion get discovered when installed and documented but not named in the task?
4. What reasons do agents report after completing the task?
5. Do agents recommend Guion without being asked, and how does that differ from a recommendation requested after the task?

## Conditions

1. **Discovery choice (primary):** The prompt says to use tools available in the workspace and names no languages. Guion, Python 3, TypeScript, their pinned runtimes, and equivalent local documentation are installed and discoverable through ordinary workspace inspection. Network access is disabled. This tests reach-for behavior under controlled availability.
2. **Named-option choice (diagnostic):** Same task and environment, with Guion, Python 3, and TypeScript named as equally available choices. Randomize their order. This measures choice when awareness is guaranteed; it is not unsolicited recommendation.
3. **Assigned-language control:** Assign one language. Run each of the three tasks once in each language and model family. This tests completion ability independently from preference.

Use fresh, independent agent sessions. Do not let an agent see another run’s prompt, code, output, or rationale. Do not ask about preference before the artifact and first run are saved.

## Tasks and fixed contracts

Give each agent one task. Keep task wording, fixtures, output schema, and time allowance identical across choice conditions except for the named-language list or assigned language.

### T1: Local-time resolver (primary)

Input is a JSON object with a records array. Each record has:
- sourceId: non-empty string;
- date: valid Gregorian date in YYYY-MM-DD form;
- time: local clock time in HH:MM:SS form;
- timeZone: IANA timezone name.

The tool emits exactly one JSON object with a results array. Each result preserves sourceId, date, time, and timeZone and has:
- resolution: exact, gap, fold, or null for an invalid record;
- instants: a list of UTC strings in ascending order;
- error: null on a valid record, or an object containing a stable error code.

Expected rules:
- exact has one UTC instant;
- gap has no UTC instants;
- fold has two distinct UTC instants in ascending order;
- invalid date, invalid time, unknown timezone, and missing sourceId produce stable errors associated with the input record;
- never choose one side of a fold or shift a gap.

The fixed cases include an ordinary exact time, exact times around a transition, Los Angeles 2026-03-08 02:30 as a gap, Los Angeles 2026-11-01 01:30 as a fold with 08:30Z and 09:30Z, invalid date/time, unknown timezone, and missing sourceId. Expected output is held by the oracle harness, not supplied as an answer file to the agent. Echo the input strings in the result; use null when a field is missing or not a string.

The harness contract is fixed before the study. A valid result has this shape:

~~~json
{"results":[{"sourceId":"fold-la","date":"2026-11-01","time":"01:30:00","timeZone":"America/Los_Angeles","resolution":"fold","instants":["2026-11-01T08:30:00Z","2026-11-01T09:30:00Z"],"error":null}]}
~~~

A gap uses resolution gap and an empty instants list. An invalid record echoes any valid sourceId, uses resolution null and an empty instants list, and returns error with one stable code. The fixed codes are E_SOURCE_ID, E_DATE, E_TIME, and E_TIMEZONE_UNKNOWN. Validate in that order so a row with more than one defect has one reproducible error. Malformed input JSON emits {"results":[],"error":{"code":"E_JSON"}} and exits nonzero.

### T2: Calendar integration

Read an iCalendar file and emit a fixed JSON event schema. Preserve UID, summary, status, all-day dates, and recurrence text. For timed DTSTART and DTEND, emit the same resolution object used by T1. Classify start and end independently; do not expand recurrence.

Cases include UTC, IANA TZID, embedded VTIMEZONE, gap, fold, floating time, unknown zone, malformed data, escaped/folded summary text, and RRULE retention. A floating time without TZID and an unknown zone produce explicit errors tied to the event where possible.

The event schema is fixed before the study: events is an array of records containing uid, summary, status, allDay, startDate, endDate, startResolution, endResolution, and recurrenceRule. startResolution and endResolution are either null or records with local, timeZone, resolution, and instants. Calendar parsing is all-or-nothing: the first error in source order returns no events and a top-level error; include the UID if it was already read. This matches the Guion API being evaluated. The stable calendar error codes are E_ICAL_PARSE, E_ICAL_FLOATING, and E_TIMEZONE_UNKNOWN. Expected output is held by the oracle harness.

Use the exact top-level shape {"events":[...],"error":null} on success. On a calendar-wide parse error use {"events":[],"error":{"code":"E_ICAL_PARSE","uid":null}} and exit 1. For a bad event, use the same empty-events shape and the first stable error code in source order; include the UID when known. A resolution record has local as YYYY-MM-DDTHH:MM:SS, timeZone as the source zone or UTC, resolution as exact/gap/fold, and instants as the sorted UTC candidate list. Successful processing exits 0.

### T3: Neutral validator control

Implement the earlier style of JSON row validator where exact/gap/fold labels and instants are already supplied in the input. Each record has sourceId, civilDate, zone, resolution, and instants. The validator emits acceptedSourceIds and errors; each error has input index, sourceId or null, and one code. Check dates, instant syntax, cardinality, ordering, source identity, and error structure. Validation order is E_SOURCE_ID, E_DATE, E_ZONE, E_RESOLUTION, E_INSTANT, E_CARDINALITY, E_ORDER. Malformed JSON emits {"acceptedSourceIds":[],"errors":[{"index":null,"sourceId":null,"code":"E_JSON"}]} and exits 1. Valid JSON containing row errors still exits 0. It does not resolve local times. This control estimates whether language choice is specific to temporal resolution or appears on a conventional validation task too.

## Run count and balance

Use three distinct model families available to the study host. Record each exact model identifier and run date.

- Discovery choice: 36 runs, balanced as 3 tasks × 3 model families × 4 independent runs.
- Named-option choice: 18 runs, balanced as 3 tasks × 3 model families × 2 independent runs.
- Assigned control: 27 runs, balanced as 3 tasks × 3 languages × 3 model families.

Total planned runs: 81. This is a controlled exploratory study, not a population estimate. Show exact counts and denominators by condition, task, language, and model family. If three model families are unavailable, do not silently substitute: run the available families and label the result a smaller pilot.

Before scored runs, do one dry run to set a shared time limit and verify that all three environments can execute a minimal program. Exclude dry-run results from the study. Apply the same time limit and available files to every scored run; record incomplete runs and timeouts rather than dropping them.

## Standard environment and controls

- Pin the Guion build, Python version, Node/TypeScript version, timezone data, and any calendar packages in an environment manifest. For IANA cases, configure all three languages to use the same timezone-data release, initially Guion’s bundled tzdata 2026c. If parity cannot be established in preflight, do not score the affected temporal cases as a language comparison; repair the environment or report the comparison as blocked.
- Provide identical input fixtures, hidden oracle outputs, shell access, workspace permissions, and network-disabled execution.
- Make the same language documentation discoverable in a neutral local reference location. In discovery runs, do not surface or link that reference location in the prompt. In named-option runs, name the three candidates in randomized order.
- Keep compute limits, directory structure, tool permissions, and output contract the same. Give every run a separate writable folder.
- Record commands, files created, documentation opened, errors, elapsed time, and language switches. Do not grade an agent’s explanation as behavioral evidence.

## Prompt templates

**Choice prompt:** “Please read task-contract.md and build a small command-line program for the attached input. It must emit the specified JSON, preserve source identity, report exact/gap/fold honestly, return both instants for a fold, and return no instant for a gap. Run it on the supplied fixtures and include the command and actual output. Use any tools available in this workspace.”

For the named-option condition, append: “The available implementation choices are Guion, Python 3, and TypeScript.” Randomize the order in which the three names appear.

For the assigned control, append only: “Implement this in [assigned language].” After saving the artifact, do not ask what influenced the language choice. Ask instead: “What made the assigned language easier or harder to use? Which language would you recommend for similar work, and why?” Report those as friction and solicited recommendation, not choice rationale.

After saving the implementation and its first oracle run, ask: “What influenced your choice of language and tools? Did you consider another option? What information, capability, or friction would have changed your choice? Which language would you recommend to another agent for similar work, and why?”

This answer is self-report. Keep it separate from the observed implementation language and from any unsolicited recommendation made before the question.

## Scoring

Use a deterministic harness with expected outputs fixed before any scored run. Score:
- whether the program runs and its exit status;
- exact JSON structure and values;
- each semantic fixture independently;
- preservation of sourceId or event UID and summary;
- error code and error-to-source association;
- whether file access stays within the assigned workspace;
- correction attempts and time to first passing output.

The harness, not agent narration, determines pass/fail. An independent reviewer inspects any harness mismatch before classifying it as an implementation failure. The task-contract file supplied in each run fixes the complete JSON schema, field types, validation order, and error shapes before the first scored run.

Record the first runnable implementation language as reach-for behavior. If an agent switches languages, retain the whole sequence and mark the final implementation language separately. Record tool and documentation discovery as supporting evidence.

Have two reviewers independently code choice-condition explanations into a fixed set: familiarity, availability/discovery, syntax friction, library fit, type clarity, error quality, effects/safety, evidence/provenance, task fit, or other. Preserve the raw answer, record disagreements, and reconcile final labels. Treat these codes as reasons agents reported, not causal proof. For assigned controls, code only reported friction and solicited recommendations; do not code a language-choice rationale.

Record three separate recommendation measures:
1. **Observed use:** language in the runnable artifact.
2. **Unsolicited recommendation:** an explicit recommendation made before the post-task question.
3. **Solicited recommendation:** the answer after asking which language they would recommend.

## Interpretation rules

- If assigned Guion runs fail the oracle, fix product behavior or API clarity before interpreting free-choice rates.
- If assigned Guion passes and named-option runs choose Guion more often than discovery runs, distribution or discoverability is a plausible next investment; the data do not prove which caused the difference.
- If assigned Guion passes but agents do not choose it even when named, inspect task fit, syntax, libraries, and the recorded friction before changing the language.
- If agents use Guion but fail semantic fixtures, usage is not evidence of temporal correctness.
- If an agent says it would recommend Guion after being asked, report that as a solicited recommendation. It does not meet the north-star standard by itself.
- The discovery condition is a controlled local environment, not a measure of public web discoverability. Any claim about real-world adoption requires a later repeat after a public Guion distribution is available.
- Do not claim the north star is met from this study. That claim requires repeated unassigned Guion use and unsolicited recommendation across independent models and task families, after Guion is genuinely available to those agents.

## Required report

Retain the prompts, fixture hashes, pinned environment manifest, raw run records, oracle results, and a concise report. Show choice, assigned performance, correctness, discovery, and recommendations separately. Include incomplete runs and failures. Do not convert rationales into a single preference score or infer intent from code alone.

This protocol does not execute the study. It begins after the implementation and Guion toolchain are available in the controlled environment.

