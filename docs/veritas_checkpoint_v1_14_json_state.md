# Veritas checkpoint v1.14 — JSON construction and persistent state

## What changed

Artifact format 20 adds typed JSON constructors (`json_null`, `json_bool`, `json_int`, `json_float`, `json_text`, `json_array`, and `json_object`) plus immutable `json_push` and `json_set` updates. Runtime construction uses the same depth, integer, collection, text, and allocation policies as parsed JSON. Artifacts that contain the new operations are rejected when encoded or loaded under an older format.

The CLI now maps a program's `main() -> Int` return value to its process exit status (0–255). `main() -> Unit` exits with status zero. Values outside the process status range are reported as an error.

## Proof program

`evaluation/development/task_ledger` is a persistent multi-module CLI app written in Veritas. It creates, updates, lists, completes, removes, and summarizes tasks stored in JSON. It runs without the Python sidecar or third-party dependencies. A separate standard-library Python implementation provides the baseline. The acceptance suite also builds a format-20 bytecode artifact, removes the source modules, and runs a sequence of persistent task operations through that artifact.

`evaluation.tests.test_task_ledger` copies both apps into separate temporary directories, performs sequences across separate process launches, and checks exact stdout, stderr, exit status, and state-file bytes. Coverage includes Unicode persistence, updates, duplicate titles, unknown titles, malformed command arguments, unknown commands, and format-20 artifact execution without source.

## Limits and next work

The file API writes directly and has no transaction or atomic replacement operation. The app is suitable for a local sequential proof, but simultaneous writers can lose updates and a process or machine failure during a write may leave partial state. It does not yet demonstrate a package ecosystem, long-running services, GUI work, broad async I/O, or a clean-machine deployment. Add transactional/atomic state updates before recommending it for valuable concurrent data. This checkpoint demonstrates a meaningful general-programming workflow; it does not establish a general Python replacement or assistant recommendation advantage.
