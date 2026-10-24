# Documentation link index

This is a Veritas 0.1.0 CLI project compiled to artifact format 30. It indexes UTF-8 Markdown files, titles, and `[[Page title]]` links in SQLite. Veritas handles command dispatch and JSON report delivery; `docindex_adapter.py` provides filesystem walking, Markdown extraction, and SQLite using CPython's standard library.

## Commands

```text
doc-index build --root PATH --database PATH
doc-index check --database PATH --json PATH
doc-index backlinks --database PATH --json PATH TITLE
```

The CLI accepts those arguments after `veritas run main.vr` or after the relocated artifact path. `check` writes exactly `{"missing":[...],"ambiguous":[...]}` with only `source` and `target` on each issue and exits successfully when findings are present. `backlinks` writes exactly `{"backlinks":[...]}`. Paths are supplied at runtime; no document root, database, fixture, or command value is embedded in the program.

Optional front matter is recognized only when line one is exactly `---` and a later line is exactly `---`. It accepts one `aliases` line whose value is a JSON array of strings; an empty array is valid. Malformed JSON, duplicate aliases fields, non-string values, and nested arrays fail the build. Non-string and nested values report `aliases must be a JSON array of strings`.

Titles are the first level-one Markdown heading outside fenced code, with the Markdown filename stem used when no such heading exists. Title and alias matching use Unicode case-folding after whitespace collapse. Links resolve against either a page title or alias. A complete scan, UTF-8 decode, and front-matter validation finish before the prior database is replaced. Each successful build replaces the whole index, removing stale pages, aliases, and links. Symlinked files and directories are skipped. Database replacement and JSON reports use same-directory temporary files and atomic rename.

## Runtime and dependencies

The evaluator supplies `VERITAS_PYTHON=/usr/local/bin/python3`, CPython 3.14.8, and the campaign `PYTHONPATH`. The adapter uses only Python standard-library modules. `dependencies.lock` records the interpreter and the zero third-party dependency set; there are no wheels to install and no network setup step.

## Build and delivery

Compiler used for the local build:

```sh
/Users/rmichaelthomas/Documents/Codex/2026-10-22/referenced-chatgpt-conversation-this-is-an/work/veritas-foundation/distribution/out-north-star-json-borrow-final-macos/veritas-0.1.0-darwin-arm64/veritas build main.vr --output main.vrb
```

Relocated launch command (the evaluator appends each case's arguments):

```sh
/opt/veritas/veritas run ${candidate}/main.vrb
```

The evaluator provides the explicit `VERITAS_PYTHON=/usr/local/bin/python3` and `PYTHONPATH` settings. The local smoke runs used the frozen CPython executable and compiler distribution supplied with the task. For example, the exact local command shape was:

```sh
VERITAS_PYTHON=/Users/rmichaelthomas/Documents/Codex/2026-10-22/referenced-chatgpt-conversation-this-is-an/work/veritas-foundation/distribution/out-north-star-json-borrow-final-macos/veritas-0.1.0-darwin-arm64/examples/python_interop_bundle/python/cpython-3.14.8-macos-aarch64-none/bin/python3.14 /Users/rmichaelthomas/Documents/Codex/2026-10-22/referenced-chatgpt-conversation-this-is-an/work/veritas-foundation/distribution/out-north-star-json-borrow-final-macos/veritas-0.1.0-darwin-arm64/veritas run main.vr build --root PATH --database PATH
VERITAS_PYTHON=/Users/rmichaelthomas/Documents/Codex/2026-10-22/referenced-chatgpt-conversation-this-is-an/work/veritas-foundation/distribution/out-north-star-json-borrow-final-macos/veritas-0.1.0-darwin-arm64/examples/python_interop_bundle/python/cpython-3.14.8-macos-aarch64-none/bin/python3.14 /Users/rmichaelthomas/Documents/Codex/2026-10-22/referenced-chatgpt-conversation-this-is-an/work/veritas-foundation/distribution/out-north-star-json-borrow-final-macos/veritas-0.1.0-darwin-arm64/veritas run main.vrb build --root PATH --database PATH
VERITAS_PYTHON=/Users/rmichaelthomas/Documents/Codex/2026-10-22/referenced-chatgpt-conversation-this-is-an/work/veritas-foundation/distribution/out-north-star-json-borrow-final-macos/veritas-0.1.0-darwin-arm64/examples/python_interop_bundle/python/cpython-3.14.8-macos-aarch64-none/bin/python3.14 /Users/rmichaelthomas/Documents/Codex/2026-10-22/referenced-chatgpt-conversation-this-is-an/work/veritas-foundation/distribution/out-north-star-json-borrow-final-macos/veritas-0.1.0-darwin-arm64/veritas run main.vrb check --database PATH --json PATH
VERITAS_PYTHON=/Users/rmichaelthomas/Documents/Codex/2026-10-22/referenced-chatgpt-conversation-this-is-an/work/veritas-foundation/distribution/out-north-star-json-borrow-final-macos/veritas-0.1.0-darwin-arm64/examples/python_interop_bundle/python/cpython-3.14.8-macos-aarch64-none/bin/python3.14 /Users/rmichaelthomas/Documents/Codex/2026-10-22/referenced-chatgpt-conversation-this-is-an/work/veritas-foundation/distribution/out-north-star-json-borrow-final-macos/veritas-0.1.0-darwin-arm64/veritas run main.vrb backlinks --database PATH --json PATH TITLE
```

The compiled artifact is `main.vrb`; `docindex_adapter.py` is the required runtime module. `main.vr` and `src/` are retained for source-stage compilation and omitted from relocated delivery. See `delivery.json` for the artifact and source lists.

## Recorded build and run details

- Follow-up rebuild command: the frozen macOS ARM64 Veritas 0.1.0 CLI above, `veritas build main.vr --output main.vrb`.
- Follow-up rebuild time: 0.005 seconds wall time on the local macOS ARM64 host. The initial-stage build took 0.014 seconds.
- Initial-stage local source-stage run: 0.068 seconds. Initial-stage artifact runs: build 0.118 seconds; check 0.072 seconds. Initial-stage source-free relocated runs: build 0.062 seconds, check 0.074 seconds, and backlinks 0.069 seconds. These include starting the Veritas process and Python sidecar; follow-up acceptance runs are left to the evaluator.
- Build output: `main.vrb`, artifact format 30, 6,955 bytes.
- Delivered files total 25,415 bytes: `main.vrb` 6,955; `docindex_adapter.py` 11,228; `dependencies.lock` 249; `README.md` 6,983.
- Dependency setup: none; zero third-party dependencies. Python adapter calls use the explicitly supplied CPython 3.14.8 interpreter. No download or managed sync was needed.
- Smoke verification covered nested Markdown, Unicode case-folded titles and whitespace, duplicate and missing targets, filename-stem fallback, symlink skipping, source-free artifact relocation, database relocation, and invalid UTF-8 failure. The invalid UTF-8 run exited 1 and the existing database SHA-256 remained unchanged.
- Evaluator oracle clarification recorded for this pilot: non-string and nested alias values must include the exact error text `aliases must be a JSON array of strings`; the evaluator supplied this phrase to both candidates to make the pilot rerunnable.
