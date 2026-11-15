# Concurrency: what the worker count buys

`parallel_map(items, function, limit)` runs `function` on each item with at most `limit` workers at a time. The worker count is a ceiling on how many jobs run at once. Jobs that **wait** (on a socket, a file or another process) and jobs that **compute** both finish sooner with more workers, up to the machine's cores; the measurements below show each.

## Waiting: workers share one listener

A server whose handlers block on a socket scales with workers. Give each worker the same listener and let it accept, read and answer one connection at a time. A client that connects and then stalls holds only the worker that accepted it; the others keep serving. The shipped example is [`docs/agents/examples/concurrent_server.gu`](agents/examples/concurrent_server.gu):

```
guion run --allow-bind 127.0.0.1:8099 concurrent_server.gu 8099 4 10000
```

The arguments are the port, the worker count and how long a worker waits for the rest of a half-sent request (milliseconds). The test `concurrent_server_shares_the_listener_across_workers_and_serializes_with_one` in `crates/cli/tests/stdlib_http_server.rs` pins both directions on every `cargo test` run: with 4 workers a second request is answered within 1 second while a first request is held half-sent; with 1 worker it is not answered in that window and is answered only after the held request times out.

## Computing: workers now scale

Four CPU-bound jobs (a counting loop of 100,000,000 iterations each) through one `parallel_map` call, changing only the limit. The example is [`docs/agents/examples/cpu_pool_scaling.gu`](agents/examples/cpu_pool_scaling.gu); it needs an `instruction_limit` of at least 2,000,000,000 in the project's `guion.toml`:

```
guion run --allow-clock cpu_pool_scaling.gu 1
guion run --allow-clock cpu_pool_scaling.gu 4
```

Measured 2026-11-14 on an Apple M1 Pro, macOS, release build of this tree, median of 5 runs each:

| Limit | Wall time (median) | Runs (ms) |
|---|---|---|
| 1 | 904 ms | 905, 904, 904, 908, 903 |
| 4 | 271 ms | 270, 270, 271, 271, 272 |

Limit 4 took 0.30 times as long as limit 1 for the same four jobs. On 2026-11-13, before the change below, the same jobs took 972 ms at limit 1 and 1,745 ms at limit 4 (1.80 times as long).

**Cause (found with `sample` on the Mac).** The time was not in computing. It was in three small functions that update shared counters: the cancellation poll, and reserving and releasing instruction fuel. Machine code polls for cancellation on every loop iteration and takes fuel from the budget shared by all workers. The shared instruction counter sat on the same cache line as the cancellation flags the polls read, so each fuel update by one worker evicted the line every other worker was reading (false sharing). Machine code also took fuel 256 instructions at a time, and the interpreter running a worker took it one instruction at a time with an atomic operation.

**Change.** The shared instruction counter has its own cache line; machine code takes fuel in blocks of at least 4,096 and hands back what a function does not use; worker interpreters take fuel in blocks too; and the first worker to find the budget empty cancels its siblings.

The same cause made a `parallel_map` that ran out of instruction budget part-way take far longer than the same program with one worker (4 workers, 1,500,000,000 instructions allowed: 76 s; now 0.7 s). When machine code runs out of fuel the function is re-run in the interpreter to find the exact failing instruction, and with four workers that re-run was contended; the test `workers_that_run_out_of_budget_stop_promptly` in `crates/cli/tests/parallel_budget.rs` pins it.

Waiting still scales the same way as before: use a worker limit above one for jobs that wait on a socket, a file or another process, and for jobs that compute.
