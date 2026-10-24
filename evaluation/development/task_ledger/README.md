# Persistent task ledger

This is a small multi-module command-line application written entirely in Veritas. It creates `tasks.json` on the first change and supports `add`, `list`, `done`, `remove`, and `summary`. It uses JSON construction and update APIs, atomic text replacement, module imports, typed file errors, command arguments, and process exit statuses. No Python bridge or third-party package is used by the Veritas app.

From this directory, run:

```sh
veritas run . list
veritas run . add "Ship beta" "Check release notes"
veritas run . done "Ship beta"
veritas run . summary
veritas run . remove "Ship beta"
```

The CLI permits file access only inside this project root. `tasks.json` is ordinary readable JSON. Writes replace the complete file atomically, so readers do not observe half-written JSON. Commands are designed for sequential local use; simultaneous writers can still overwrite each other's complete updates, and a crash can leave an unused temporary file behind.

## Independent check

`python_baseline.py` is an independent standard-library implementation. From the foundation root, run `python3 -m unittest evaluation.tests.test_task_ledger -v` after building `veritas-cli`. The acceptance checks copy both programs into separate temporary directories and compare command output, exit status, and persisted bytes over separate process invocations. They start without a data file and also build a current-format artifact, remove its source files, and verify the artifact still performs and persists the same operations. The baseline is a correctness reference, not an independent evaluation.
