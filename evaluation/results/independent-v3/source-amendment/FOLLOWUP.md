# Scheduled follow-up: --team NAME

Only application change: the declared adapter accepts one optional `--team NAME` pair, validates every ticket as before, then selects exact case-sensitive team matches. The unchanged Veritas report code constructs the selected ticket array and summary. Default output remains byte-for-byte identical in the regression test. Help text now lists the optional flag. No dependencies, locks, delivery manifest, or Veritas source changed.

Initial snapshot preserved and SHA-256 verified: `8c698c5f8e636deddc408f2776d8c8260f60a7aa7587a9e74c324496a8aee746`. The original runnable package also remains at `work/initial-package`. Application diff: `work/followup.diff`. New tests: `tests/test_team.py`.

## Tests and commands

Commands ran sequentially, offline, with the assigned candidate as the host working directory. Setup reused previously authorized local interpreter metadata and packaging-only readelf. No installation, network download, or human code intervention occurred.

1. Before implementation, ran in `veritas-campaign-veritas:format33`, mounted candidate at `/candidate`, working directory `/candidate`, entrypoint `python3`: `-m unittest discover -s tests -p test_team.py`. Four methods ran; three failed as expected because `--team` was unsupported. Log: `work/logs/followup-red.txt`.
2. Edited only application adapter `sla_io.py`; added team test file. Required flags remain mandatory, duplicate/empty/missing team arguments fail, filtering occurs after validation.
3. In the same frozen image, with `--network none`, `PATH=/candidate/work/bin:/usr/local/bin:/usr/bin:/bin` and `UV_PYTHON_DOWNLOADS_JSON_URL=file:///candidate/work/offline-python/downloads.json`, ran:

```sh
/opt/veritas/veritas check .
/opt/veritas/veritas test .
python3 -m unittest discover -s tests
mv dist/sla-report work/initial-package
/opt/veritas/veritas package --json . --output dist/sla-report
```

Check passed; one native test passed; all 13 test methods passed in 7.755 seconds; normal package creation succeeded. Log: `work/logs/followup-build.txt`.

4. In a separate frozen-image container with only package `/application`, test harness `/checks:ro`, and fresh test-data workspace `/workspace` mounted; working directory `/workspace`, `SLA_COMMAND='["/application/run"]'`, `SLA_TEST_ROOT=/workspace`, ran:

```sh
/application/veritas verify-package --json /application
python3 -m unittest discover -s /checks
```

Package verification passed; all 13 test methods passed in 10.683 seconds. Original source and packaging tools were not mounted. Log: `work/logs/followup-relocated.txt`.

New cases cover team match, multiple tickets and ordering, selected summary counts, team miss, case sensitivity, unchanged default bytes before/after filtering, invalid option forms, and validation of an invalid unselected ticket with output preservation. Official acceptance was not invoked.

## Timing and delivery

Follow-up recorded start: 2026-10-24T14:20:29+00:00; end: 2026-10-24T14:22:30.313962+00:00; elapsed wall time: 121.314 seconds. This is an author-recorded wall interval; official active time remains the coordinator's measurement. All changes occurred after initial completion. Package logical bytes now: 125231449. Bytecode remains 9900 bytes. Lockfiles and dependency inventory are unchanged. Full follow-up metrics are in `work/followup-metrics.json`.

Launch remains `./dist/sla-report/run --tickets PATH --policy PATH --as-of RFC3339 --output PATH`, optionally followed by `--team NAME`. `delivery.json` is unchanged. No known unresolved application failures in author-created tests. Official evaluation remains outstanding.
