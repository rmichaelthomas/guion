# Assignment: Field Kit Log

**Target class:** maintainable local application with configuration and persistent state

Build a local command-line inspection log for a small field-equipment team. A TOML file selects the SQLite database path. Support adding an equipment item, recording a `pass` or `fail` inspection with a short note, and listing inspection history in stable order. Each new inspection must record the current local calendar date automatically; the normal command must not ask the operator to enter a date. Reject duplicate item IDs, unknown items, and invalid results without changing prior state. Store the database under a caller-selected writable data directory that already exists and stays outside the installed runtime.

**Follow-up change:** add `list --failed-only`, showing only failed inspections while preserving the original unfiltered list behavior.

**Acceptance:** exercise item creation, pass/fail inspection records, automatic local-date stamping, process-restart persistence, duplicate/unknown/invalid-input errors, the follow-up filter, old behavior after the edit, and relocation to a path containing spaces and Unicode with data in a separate caller-selected directory. Deliver source, a sample TOML configuration, and a short operator guide. Python may use only CPython 3.14.7's standard library. Veritas must use the installed native SQLite/TOML services and the documented, explicitly granted local-clock API. No third-party dependencies or network access.
