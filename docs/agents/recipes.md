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

## Make an HTTP request with a narrow grant

Source: [`http_request.gu`](examples/http_request.gu).

```sh
guion run --allow-http-host api.example.org docs/agents/examples/http_request.gu https://api.example.org/health
```

Replace the example hostname and URL with the service you intend to call. The hostname grant must exactly match the URL host. The program prints the response status and response body on separate lines. Those values depend on the endpoint; errors go to stderr. The recipe uses a text request/response, and network access remains denied if the grant is omitted.

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

The program reads its adjacent TOML file, creates or opens `agent-recipes.sqlite3` under the extra data root, inserts the configured name in a transaction, reads the latest row, and closes the database. Each run inserts another row. The database is outside the source checkout because writes go to the last explicitly granted root.

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

## API and test contracts

The language signatures used in these programs come from `language/builtin-signatures.json`, `language/stdlib/*.gu`, and `language/host-api.md`; this page does not duplicate those signatures. CLI JSON report envelopes are described in [`cli-reports.schema.json`](cli-reports.schema.json). Their emitters and integration tests are indexed in [`index.json`](index.json).
