# Concurrency: what the worker count buys

`parallel_map(items, function, limit)` runs `function` on each item with at most `limit` workers at a time. The worker count is a ceiling for **waiting**, not a multiplier for **computing**.

## Waiting: workers share one listener

A server whose handlers block on a socket scales with workers. Give each worker the same listener and let it accept, read and answer one connection at a time. A client that connects and then stalls holds only the worker that accepted it; the others keep serving. The shipped example is [`docs/agents/examples/concurrent_server.gu`](agents/examples/concurrent_server.gu):

```
guion run --allow-bind 127.0.0.1:8099 concurrent_server.gu 8099 4 10000
```

The arguments are the port, the worker count and how long a worker waits for the rest of a half-sent request (milliseconds). The test `concurrent_server_shares_the_listener_across_workers_and_serializes_with_one` in `crates/cli/tests/stdlib_http_server.rs` pins both directions on every `cargo test` run: with 4 workers a second request is answered within 1 second while a first request is held half-sent; with 1 worker it is not answered in that window and is answered only after the held request times out.

## Computing: more workers is not faster

Four CPU-bound jobs (a counting loop of 100,000,000 iterations each) through one `parallel_map` call, changing only the limit. The example is [`docs/agents/examples/cpu_pool_scaling.gu`](agents/examples/cpu_pool_scaling.gu):

```
guion run --allow-clock cpu_pool_scaling.gu 1
guion run --allow-clock cpu_pool_scaling.gu 4
```

Measured 2026-11-13 on an Apple M1 Pro, macOS, release build of this tree, median of 5 runs each:

| Limit | Wall time (median) | Runs (ms) |
|---|---|---|
| 1 | 972 ms | 983, 972, 970, 981, 972 |
| 4 | 1,745 ms | 1,665, 1,670, 1,782, 1,745, 1,853 |

Limit 4 took 1.80 times as long as limit 1 for the same four jobs. Do not raise the worker count to speed up computing; use it where workers spend their time waiting for a socket, a file or another process. The cause of the slowdown is not established here; only the timings are.
