# Follow-up and relocation evidence

## Later source edit

- Follow-up source edit started: `2026-10-25T10:02:07.713075Z`.
- Old behavior regression and due-soon checks completed: `2026-10-25T10:03:21.273245Z`.
- Follow-up duration: `73.560170 s`.
- The source gained `due-soon --as-of YYYY-MM-DD`. Dates are validated before any database connection. The inclusive window is `[as-of, as-of + checkout_days]`; stable ordering is due date, then case-insensitive book ID, then exact book ID.
- Old behavior regression: all 15 original CLI checks passed again after the source edit. See [old_behavior_regression.json](old_behavior_regression.json).
- Due-soon checks: 16 CLI invocations passed. Date boundary, tie ordering, overdue/out-of-window exclusion, invalid calendar date, invalid format, unchanged database bytes and rows, and no database-directory creation for an invalid date were checked. See [followup_due_soon_checks.json](followup_due_soon_checks.json).
- Deterministic due-date fixture values were set with SQL after books were added and checked out through the CLI; this avoided adding an unrequested backdate option to the command line.

## Relocated launch

- Project: `/private/tmp/veritas-fresh-builders/bookloom-python/📚 Bookloom – bibliothèque`.
- Database: `/private/tmp/veritas-fresh-builders/bookloom-python/caller-data/bookloom.sqlite3`, outside the program directory and selected by the adjacent TOML config.
- The project was moved from `initial-project` into a path containing spaces and Unicode. Add, checkout, list, return, due-soon, invalid-date immutability, and SQLite integrity checks passed there. See [relocation_checks.json](relocation_checks.json).
- Installed-launch measurement: five fresh launcher processes ran `./bookloom list available` from the relocated project. Median elapsed time was `0.066778 s`; individual times are in the relocation log.
- Deliverable footprint: `12,263 bytes` total; `10,798 bytes` of executable source (`bookloom.py` and the launcher).
- Final verification ran 22 CLI invocations from the relocated project, including persistence, transaction-safe failures, due-soon ordering/window behavior, invalid-date immutability, and SQLite integrity. See [final_verification.json](final_verification.json).
- The first final evidence read found that its caller-data separation predicate treated the relative `..` path lexically and reported `false`. I resolved the config path before checking containment and reran the full verification; the final record reports `database_outside_project=true`. The database path is `/private/tmp/veritas-fresh-builders/bookloom-python/caller-data/bookloom.sqlite3`.

## Limitations

- The launcher runs with the installed Python 3.14.7 runtime, but this build did not create a pip or OS package installation. The measured launch time is a direct launcher invocation from the relocated project.
