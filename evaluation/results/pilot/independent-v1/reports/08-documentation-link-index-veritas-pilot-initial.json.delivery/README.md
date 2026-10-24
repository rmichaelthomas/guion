# Documentation link index

This is a Veritas 0.1.0 CLI project compiled to artifact format 30. It indexes UTF-8 Markdown files, titles, and `[[Page title]]` links in SQLite. Veritas handles command dispatch and JSON report delivery; `docindex_adapter.py` provides filesystem walking, Markdown extraction, and SQLite using CPython's standard library.

## Commands

```text
doc-index build --root PATH --database PATH
doc-index check --database PATH --json PATH
doc-index backlinks --database PATH --json PATH TITLE
```

The CLI accepts those arguments after `veritas run main.vr` or after the relocated artifact path. `check` writes exactly `{"missing":[...],"ambiguous":[...]}` with only `source` and `target` on each issue and exits successfully when findings are present. `backlinks` writes exactly `{"backlinks":[...]}`. Paths are supplied at runtime; no document root, database, fixture, or command value is embedded in the program.

Titles are the first ATX heading outside fenced code, with the Markdown filename stem used when no heading exists. Title matching uses Unicode case-folding after whitespace collapse. A complete scan and UTF-8 decode finish before the prior database is replaced. Symlinked files and directories are skipped. Database replacement and JSON reports use same-directory temporary files and atomic rename.

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

## Recorded local build

- Build command: `veritas build main.vr --output main.vrb` using the frozen macOS ARM64 Veritas 0.1.0 CLI above.
- Build time: 0.014 seconds wall time on the local macOS ARM64 host.
- Local source-stage build run: 0.068 seconds. Local artifact run times: build 0.118 seconds; check 0.072 seconds. A source-free relocated run took 0.062 seconds for build, 0.074 seconds for check, and 0.069 seconds for backlinks. These include starting the Veritas process and Python sidecar.
- Build output: `main.vrb`, artifact format 30, 6,955 bytes.
- Delivered files total 21,210 bytes: `main.vrb` 6,955; `docindex_adapter.py` 7,986; `dependencies.lock` 249; `README.md` 6,020.
- Dependency setup: none; zero third-party dependencies. Python adapter calls use the explicitly supplied CPython 3.14.8 interpreter. No download or managed sync was needed.
- Smoke verification covered nested Markdown, Unicode case-folded titles and whitespace, duplicate and missing targets, filename-stem fallback, symlink skipping, source-free artifact relocation, database relocation, and invalid UTF-8 failure. The invalid UTF-8 run exited 1 and the existing database SHA-256 remained unchanged.
