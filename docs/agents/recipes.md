# Guion recipes for agents

These programs are stored as runnable `.gu` files and compiled with the current Guion CLI. Commands below use the source checkout from its root; with a built `guion` executable, remove `cargo run --locked -p guion-cli --`.

## Read a JSON file from a CLI argument

Source: [`file_data_cli.gu`](examples/file_data_cli.gu). Fixture: [`records.json`](examples/records.json).

```sh
cargo run --locked -p guion-cli -- run docs/agents/examples/file_data_cli.gu records.json
```

Expected output:

```text
3
```

The program reads the named file, parses its JSON array, reports the row count, and returns a nonzero status with an error code if reading or parsing fails. When the input is outside the source directory, pass a granted directory with `--file-root` and keep the guest path relative to that grant.

## Resolve a local time without hiding ambiguity

Source: [`native_temporal_resolution.gu`](examples/native_temporal_resolution.gu).

```sh
cargo run --locked -p guion-cli -- run docs/agents/examples/native_temporal_resolution.gu
```

The example reports the packaged timezone-data version, preserves the original local fields and timezone, and prints every matching UTC candidate. A daylight-saving gap has no candidate; a fold has two in chronological order. The example then demonstrates a caller-owned policy that chooses the earlier fold occurrence. Guion's resolver itself never chooses or shifts a time. An invalid calendar date returns the stable `E_DATE` code. The operation is pure and needs no clock, filesystem, or network grant.

## Make an HTTP request with a narrow grant

Source: [`http_request.gu`](examples/http_request.gu).

```sh
cargo run --locked -p guion-cli -- run --allow-http-host api.example.org docs/agents/examples/http_request.gu https://api.example.org/health
```

Replace the example hostname and URL with the service you intend to call. The hostname grant must exactly match the URL host. `http_request_bytes` returns a typed `HttpResponse` with status, headers, and the exact response bytes. Use `http_response_text` to decode checked UTF-8 or `std:http.http_response_json` to parse JSON directly from those bytes. Non-2xx statuses remain response values; invalid UTF-8 and malformed JSON are recoverable typed errors. Errors go to stderr, and network access remains denied if the grant is omitted.

## Read TOML and write a SQLite record

Source: [`sqlite_toml.gu`](examples/sqlite_toml.gu). Input: [`settings.toml`](examples/settings.toml).

```sh
mkdir -p /tmp/guion-agent-data
cargo run --locked -p guion-cli -- run --file-root /tmp/guion-agent-data docs/agents/examples/sqlite_toml.gu
```

Expected output:

```text
Guion agents
```

The program reads its adjacent TOML file with `fs_read_text`, creates or opens `agent-recipes.sqlite3` under the extra data root, inserts the configured name in a transaction, reads the latest row, and closes the database. Each run inserts another row. CLI reads search the project root before extra `--file-root` roots; writes resolve under the last granted root. With this command, the final root is `/tmp/guion-agent-data`; without an extra root, the project root is used.

The recipe uses `?` to return service errors to `main`, which prints the stable error code and message to stderr and exits nonzero. If a failure occurs after SQLite opens, the runtime closes the invocation-owned handles and rolls back the still-open transaction when the invocation ends.

## Run bounded concurrent work

Source: [`concurrent_task.gu`](examples/concurrent_task.gu).

```sh
cargo run --locked -p guion-cli -- run docs/agents/examples/concurrent_task.gu
```

Expected output:

```text
[1, 4, 9, 16]
```

`parallel_map` runs a named worker function with a limit of three workers, keeps results in input order, and returns a typed `Result`. The program prints the failure code and message if a worker fails.

## Check common failure paths

Source: [`failure_paths.gu`](examples/failure_paths.gu). It has four independent modes so each denial can be reproduced without an external service:

```sh
cargo run --locked -p guion-cli -- run docs/agents/examples/failure_paths.gu missing-file
cargo run --locked -p guion-cli -- run docs/agents/examples/failure_paths.gu denied-http
mkdir -p /tmp/guion-agent-data
cargo run --locked -p guion-cli -- run --file-root /tmp/guion-agent-data docs/agents/examples/failure_paths.gu sqlite-escape
cargo run --locked -p guion-cli -- run docs/agents/examples/failure_paths.gu worker-failure
```

Each command intentionally exits with status 1 and prints the failure code to stdout. The `sqlite-escape` mode prints `E_SQLITE_PATH` because `../outside.sqlite3` leaves every granted root. `denied-http` prints `R_NETWORK_DENIED` before any request reaches the local URL because no host grant is present. `worker-failure` prints three lines: zero-based input index `1`, `R_ASSERTION`, and the assertion message. Work already completed by another worker is not rolled back.

## API and test contracts

The language signatures used in these programs come from `language/builtin-signatures.json`, `language/stdlib/*.gu`, and `language/host-api.md`; this page does not duplicate those signatures. CLI JSON report envelopes are described in [`cli-reports.schema.json`](cli-reports.schema.json). Their emitters and integration tests are indexed in [`index.json`](index.json).
