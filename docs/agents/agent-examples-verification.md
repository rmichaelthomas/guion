# Agent examples verification

Generated 2026-10-27 18:01 UTC by `scripts/verify-agent-examples.sh`.

The HTTP example was checked, linted, and formatted; it was not run and no network request was made.
Runtime stdout and exit status are compared with `verification_cases` in `docs/agents/index.json`.

| Check | Result | Details |
| --- | --- | --- |
| `native_temporal_resolution check` | PASS | exit 0; checked docs/agents/examples/native_temporal_resolution.gu (8 functions) |
| `native_temporal_resolution lint` | PASS | exit 0; linted docs/agents/examples/native_temporal_resolution.gu (no warnings) |
| `native_temporal_resolution fmt` | PASS | exit 0; formatted docs/agents/examples/native_temporal_resolution.gu |
| `file_data_cli check` | PASS | exit 0; checked docs/agents/examples/file_data_cli.gu (1 functions) |
| `file_data_cli lint` | PASS | exit 0; linted docs/agents/examples/file_data_cli.gu (no warnings) |
| `file_data_cli fmt` | PASS | exit 0; formatted docs/agents/examples/file_data_cli.gu |
| `http_request check` | PASS | exit 0; checked docs/agents/examples/http_request.gu (14 functions) |
| `http_request lint` | PASS | exit 0; linted docs/agents/examples/http_request.gu (no warnings) |
| `http_request fmt` | PASS | exit 0; formatted docs/agents/examples/http_request.gu |
| `sqlite_toml check` | PASS | exit 0; checked docs/agents/examples/sqlite_toml.gu (34 functions) |
| `sqlite_toml lint` | PASS | exit 0; linted docs/agents/examples/sqlite_toml.gu (no warnings) |
| `sqlite_toml fmt` | PASS | exit 0; formatted docs/agents/examples/sqlite_toml.gu |
| `concurrent_task check` | PASS | exit 0; checked docs/agents/examples/concurrent_task.gu (3 functions) |
| `concurrent_task lint` | PASS | exit 0; linted docs/agents/examples/concurrent_task.gu (no warnings) |
| `concurrent_task fmt` | PASS | exit 0; formatted docs/agents/examples/concurrent_task.gu |
| `agent_failure_paths check` | PASS | exit 0; checked docs/agents/examples/failure_paths.gu (21 functions) |
| `agent_failure_paths lint` | PASS | exit 0; linted docs/agents/examples/failure_paths.gu (no warnings) |
| `agent_failure_paths fmt` | PASS | exit 0; formatted docs/agents/examples/failure_paths.gu |
| `native_temporal_resolution_run` | PASS | exit 0 (expected 0); stdout 'timezone rules\n2026c\nexact\nexact\nAmerica/Los_Angeles\n2026\n3\n8\n1\n30\n0\n0\n1772962200000000\ngap\ngap\nAmerica/Los_Angeles\n2026\n3\n8\n2\n30\n0\n0\nno instant; caller policy required\nfold\nfold\nAmerica/Los_Angeles\n2026\n11\n1\n1\n30\n0\n0\n1793521800000000\n1793525400000000\ncaller policy: choose earlier occurrence\n1793521800000000\ninvalid date error\nE_DATE\n' (expected 'timezone rules\n2026c\nexact\nexact\nAmerica/Los_Angeles\n2026\n3\n8\n1\n30\n0\n0\n1772962200000000\ngap\ngap\nAmerica/Los_Angeles\n2026\n3\n8\n2\n30\n0\n0\nno instant; caller policy required\nfold\nfold\nAmerica/Los_Angeles\n2026\n11\n1\n1\n30\n0\n0\n1793521800000000\n1793525400000000\ncaller policy: choose earlier occurrence\n1793521800000000\ninvalid date error\nE_DATE\n') |
| `file_data_cli_run` | PASS | exit 0 (expected 0); stdout '3\n' (expected '3\n') |
| `sqlite_toml_run` | PASS | exit 0 (expected 0); stdout 'Guion agents\n' (expected 'Guion agents\n') |
| `concurrent_task_run` | PASS | exit 0 (expected 0); stdout '[1, 4, 9, 16]\n' (expected '[1, 4, 9, 16]\n') |
| `failure_paths_missing_file` | PASS | exit 1 (expected 1); stdout 'R_FILE_NOT_FOUND\n' (expected 'R_FILE_NOT_FOUND\n') |
| `failure_paths_denied_http` | PASS | exit 1 (expected 1); stdout 'R_NETWORK_DENIED\n' (expected 'R_NETWORK_DENIED\n') |
| `failure_paths_sqlite_escape` | PASS | exit 1 (expected 1); stdout 'E_SQLITE_PATH\n' (expected 'E_SQLITE_PATH\n') |
| `failure_paths_worker_failure` | PASS | exit 1 (expected 1); stdout '1\nR_ASSERTION\nassertion failed\n' (expected '1\nR_ASSERTION\nassertion failed\n') |

**Overall: PASS (26/26 checks passed).**
