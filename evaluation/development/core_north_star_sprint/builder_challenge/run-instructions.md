# Queue Digest evaluator instructions

## Evaluator and window

Use a builder who did not participate in the Guion implementation. The time limit is two days maximum. A human builder is preferred for learnability evidence; an isolated agent evaluator is acceptable only when it is clearly labeled as an agent exercise and not broad adoption evidence.

Do not start the evaluation until the user approves the named evaluator and the exact transfer. The approval must cover the challenge brief, acceptance cases, Guion CLI/tooling, and documentation packet listed below. No repository materials have been shared as part of preparing this packet.

## Standard materials

Provide only the normal public builder material and the frozen challenge files:

- the Guion CLI built from the sprint branch;
- `README.md`;
- `docs/agents/start-here.md`;
- `docs/agents/guion-reference.md`;
- `docs/agents/recipes.md` and its referenced runnable examples;
- `docs/reference.md`;
- `language/spec.md`, `language/host-api.md`, and `language/artifact-format.md`;
- `language/builtin-signatures.json` and the standard-library documentation/source normally available to a Guion builder;
- `brief.md` and `acceptance-cases.md` from this directory.

Do not provide an implementation, a hidden solution, or task-specific hints. Do not change acceptance rules during the run. The evaluator may use ordinary installed Guion documentation and tools.

## Environment and recording

Start from a clean project directory with the default core CLI and no network grants. Give the evaluator the brief, the visible acceptance cases, and the two-day limit at the same time. Let them choose their own implementation approach and documentation path.

Record start/end time, elapsed time, attempts, errors, commands, documentation pages consulted, requested clarifications, completed functionality, and each reproducible acceptance result. Preserve the exact source tree and outputs. Afterward, identify the highest-impact obstacle, make an in-scope documentation or core fix if warranted, then rerun the affected acceptance cases independently.

If no suitable evaluator is selected or the user does not approve the transfer, do not substitute the implementer's self-assessment. Record the outcome as incomplete, identify the missing evaluator/approval, and name the next action.
