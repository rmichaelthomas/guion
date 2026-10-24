# API synchronization

This multi-module CLI consumes a paginated JSON API using a Bearer token supplied through the explicitly granted `VERITAS_API_TOKEN` environment variable. An operator may select an initial page with `veritas run PROJECT BASE_URL START_PAGE`; an existing saved cursor always takes precedence so a restart cannot silently change the in-progress window. It retries transient transport and server failures, persists each completed page atomically, resumes from the saved cursor after failure/restart, and sends an idempotent acknowledgement request. Non-2xx statuses remain visible to program logic. The local acceptance service can force transient and permanent errors without using external credentials or network services.

The token is absent from command-line arguments and captured program output. The source can read only the environment variable named in the project manifest, and its network grant includes only loopback for the controlled tests. This explicit grant does not encrypt or store the secret; the launcher or user supplies the value at runtime.

For example, `veritas run . https://api.example.test 4` starts at page 4 when no saved state exists. If a saved cursor is present, the app resumes at that cursor even when another start page is supplied.

Run `python3 -m unittest evaluation.tests.test_api_sync -v` from the foundation root. This is a known development workload, not independent recommendation evidence.
