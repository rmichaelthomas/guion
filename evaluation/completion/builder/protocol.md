# Fresh builder evaluation protocol

## Scope

This is a bounded, exploratory evaluation of three unfamiliar application briefs: a small data CLI, a concurrent network integration, and a maintainable local application with configuration and persistent state. It produces three matched Python/Veritas pairs and does not estimate general adoption or replace the frozen independent-v1/v2/v3 evidence.

## Builder conditions

- Each language implementation runs in a separate fresh context. The same model (`gpt-6.1-sol`), reasoning effort (`medium`), tool access (local filesystem and shell), brief, fixture data, and 15-minute wall-clock budget apply to every builder. The original 15-minute budget includes the follow-up change.
- Builders receive only the assigned brief and the installed language documentation. They may use only the existing environment, installed Veritas release, and Python 3.14.7 standard library. No external packages, network lookups, author rescue, or comparative benchmark findings are supplied. Any intervention is logged.
- Language order is randomized per brief with the recorded seed in `assignments.json`. Builders execute sequentially. Each uses a unique private work directory under `/private/tmp/veritas-fresh-builders`; prior source and report paths are not included in later prompts.
- Each implementation must handle the initial brief, apply the requested follow-up, run the specified checks, and relocate the project to a path containing spaces and Unicode. The Veritas builder must use the installed release binary recorded in the assignment; Python uses the pinned interpreter.
- Correctness, initial setup, implementation/build/repair, follow-up, relocation, launch time, artifact footprint, and user interventions are separate observations. A fast incorrect result is a failure.
- Builders record an explicit language preference after checking their own delivered application and maintenance experience. A separate assessment will compare the paired outputs only after all six builder contexts finish. No recommendation is treated as a vote or statistical adoption claim.

## Timing and interruption

Record UTC start and finish times from the task log, and self-reported setup/build/follow-up durations separately. If a builder reaches 15 minutes, stop its work and preserve the partial application, failures, and recommendation as unavailable. Do not rescue or silently extend the budget.

## Result schema

Each file in `reports/` includes the assignment ID, language, brief ID, actual start/end, budget, self-reported phase timings, initial and follow-up correctness, relocation outcome, installed launch duration, artifact footprint, interventions, failures, recommendation, and evidence paths. Final class assessments include both builder records and a reasoned recommendation with observed benefit and material regressions.
