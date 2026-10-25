# Assignment: Bookloom

**Target class:** maintainable local application with configuration and persistent state

Build a local command-line lending catalog. A TOML configuration file sets `checkout_days` and the database path. Persist books and checkout state in SQLite. Support adding a book, checking it out to a borrower, returning it, and listing available/checked-out books. Reject duplicate IDs, unknown IDs, and attempts to check out an already checked-out book without corrupting prior state. Use transactions for state changes and give clear errors. The database must be created under a caller-selected writable data directory, outside the installed Veritas/Python runtime.

**Follow-up change:** add a `due-soon --as-of YYYY-MM-DD` command that lists checked-out books due within the configured checkout window, in stable order. Invalid dates must not alter database state.

**Acceptance:** exercise normal commands, process restart persistence, duplicate/unknown/already-checked-out errors, due-soon and invalid-date behavior, and a fresh relocated project path containing spaces and Unicode with data in a separate writable caller directory. Deliver source, a sample configuration, and a short operator guide. Python may use only its standard library; Veritas must use the installed native SQLite and TOML services.
