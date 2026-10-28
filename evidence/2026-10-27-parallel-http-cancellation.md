# Concurrent HTTP cancellation on worker failure

**Date:** 2026-10-27
**Status:** Passed for the named current-host scenarios
**Build:** Guion current main, artifact format 44; macOS ARM64

## What this checks

Guion already had separate evidence for `parallel_map` worker failure, individual HTTP cancellation, and real application fan-out. This check joins those paths: a worker fails while another worker is reading a deliberately incomplete HTTP response.

The local fixture starts two Guion workers. The first receives an HTTP 200 response header and only part of its body, then waits. The second receives HTTP 500; the Guion worker detects the unexpected status and fails. `parallel_map` must stop the group, cancel the request that is still reading, wait for its worker, and report the original worker failure.

## Result

The focused runtime test passed **1/1**. It observed:

- Two HTTP workers ran concurrently.
- `parallel_map` returned a `TaskError` for input index **1** with code **`R_ASSERTION`**.
- The incomplete sibling HTTP read was interrupted, and the local server observed its connection close.
- No external network was contacted; both requests used a local loopback fixture.

Command:

```sh
cargo test --locked --offline --jobs 2 -p guion-runtime --test http parallel_worker_failure_cancels_in_flight_http_sibling_and_closes_socket -- --exact
```

## Existing application evidence

The current-main frozen acceptance already covers application-shaped HTTP concurrency:

- `calendar-conflicts` case `calendar-overlap-and-boundary` passes **6/6** checks while requesting five feeds, including broken and unavailable feeds; the measured maximum is two in flight.
- `health-digest` case `health-mixed-results-and-cap` passes **5/5** checks across healthy, slow, and invalid service responses, with a two-request cap. `health-concurrency-bound` also passes **5/5** checks at that cap.
- The full current-main application acceptance is **127/127** for both Guion and Python.
- The agent-example verification passes **26/26** checks, including ordered results from `concurrent_task.gu` and a reproducible worker failure from `failure_paths.gu`.

The source path is cancellation-aware: `http_get`, `http_request`, `http_request_with_timeout`, and `http_request_bytes` all pass through the shared HTTP transport, which polls the invocation budget while awaiting response chunks. A worker failure sets the local cancellation flag observed by that transport. `parallel_map` joins running worker threads before returning.

## Scope

These results close the named current-host concurrency failure and cancellation scenarios. They do not establish a general async runtime, roll back effects already received by a remote service, or validate artifact format 44 on Linux or other targets. Those target checks remain open.
