# Bookloom operator guide

Bookloom is a local command-line lending catalog. It stores book and checkout state in SQLite and reads `checkout_days` and `database_path` from `bookloom.toml`.

## Run it

From the directory containing this project, choose a writable data directory outside the project and pass it as the Veritas file root:

```sh
veritas run --file-root "/path/to/bookloom-data" "/path/to/bookloom-project" add BK-001 "A Long Way to a Small, Angry Planet" "Becky Chambers"
```

The supplied configuration uses the relative database path `bookloom.sqlite`. Veritas directs writes to the last `--file-root`, so the database stays in the caller-selected data directory when the project is copied or relocated. Keep the configuration file with the project and use the same data directory on later runs.

## Commands

```text
add ID TITLE AUTHOR
checkout ID BORROWER
return ID
list [available|checked-out|all]
due-soon --as-of YYYY-MM-DD
```

Quote titles, authors, and borrower names containing spaces. `list` shows every book by title, then ID; filters show available or checked-out books. Checkout dates use SQLite's UTC calendar date and the configured number of checkout days. `due-soon` includes books due on or after the supplied date and no later than `checkout_days` days after it, ordered by due date, title, then ID.

Commands that change state run inside a SQLite transaction. Duplicate IDs, unknown books, and invalid checkout transitions return an error and roll back. `due-soon` validates its date before opening the database; invalid dates return an error without changing database bytes.

## Configuration

```toml
checkout_days = 14
database_path = "bookloom.sqlite"
```

`checkout_days` must be a positive integer. Set `database_path` to a relative filename or path within the supplied data root.
