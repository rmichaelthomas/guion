# Bookloom operator guide

Bookloom is a local lending catalog. It uses Python 3.14's standard library and
stores catalog state in SQLite. No Python packages are required.

## Setup

Keep the program directory and database directory separate. Edit
`bookloom.toml` to set `checkout_days` and `database_path`. A relative database
path is resolved from the config file's directory; the sample puts the database
in a sibling caller-data directory. The database directory is created on first
use.

Run commands from the program directory, or invoke `./bookloom` directly:

```sh
./bookloom add --id bk-101 --title "The Left Hand of Darkness" --author "Ursula K. Le Guin"
./bookloom checkout --id bk-101 --borrower "Avery Chen"
./bookloom list checked-out
./bookloom due-soon --as-of 2026-10-25
./bookloom return --id bk-101
./bookloom list available
```

Use `./bookloom --config /path/to/bookloom.toml ...` to select another config.
Book IDs are unique. Failed duplicate, unknown-ID, and already-checked-out
operations report an error and leave catalog state unchanged. Checkouts receive
a due date based on the configured number of days. `due-soon` includes checked-
out books due from the supplied date through `checkout_days` later, inclusive;
results sort by due date and then book ID. The date must be a real `YYYY-MM-DD`
calendar date. Invalid dates are rejected before the database is opened.
