# Support ticket SLA report

Requires Python 3.14 or later. No third-party runtime dependencies.

```sh
uv sync --locked --offline
python3 -m unittest discover -s tests -v
python3 -m zipapp src -p '/usr/bin/env python3' -o dist/sla-report
./dist/sla-report --tickets tickets.csv --policy policy.json --as-of 2026-01-01T12:00:00Z --output report.json
```

The dependency-sync command uses no shared offline wheelhouse because this project
has no third-party dependencies. The supplied `uv.lock` is the dependency lock.
`uv run --locked --offline python3 dist/sla-report ...` is also supported.
For development without uv: `PYTHONPATH=src python3 -m sla_report ...`.

For relocation, copy only `dist/sla-report` and `uv.lock`, preserving their relative
paths. Run `python3 /new/path/dist/sla-report` with the four required options above.
The executable zipapp is named `sla-report`; add its `dist` directory to PATH to
invoke it as `sla-report`. It needs no source checkout or virtual environment.
`delivery.json` supplies the evaluator's relocation command and artifact list.

The output contains `as_of` in UTC with `Z`, `tickets` sorted by team then ID,
and `summary` containing `total` and `breached`. UTC output uses six fractional
digits when microseconds are nonzero. Ticket age is floored using exact duration
arithmetic. A breach requires an open ticket with age strictly above its limit.
Errors go to stderr with a diagnostic code and exit status 2. All inputs are
validated before an output is written; successful writes use atomic replacement.
CSV fields are preserved, but empty or whitespace-only fields are rejected.
Duplicate JSON object keys are rejected. Additional top-level policy metadata is
allowed; only `high` and `low` are accepted inside `response_minutes`.

Local verification used macOS ARM64 Python 3.14.7. The campaign guide names Linux
ARM64 Python 3.14.8 and uv 0.13.4. uv was unavailable locally and a pinned-tool
installation attempt failed because package-index DNS was unavailable, so the
uv sync command and lock compatibility remain for evaluator verification.
All local tests and relocation checks use python3 and standard-library tools.

The initial implementation intentionally awaits the evaluator's follow-up before
adding team filtering. Initial snapshots and evidence are under `outputs/initial`.
The immutable run record leaves actual active seconds, total tool calls, raw usage
evidence and Docker Engine ID pending for the evaluator; these are not estimated.
