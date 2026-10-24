# Development workloads

These are known development cases for checking ordinary source behavior. They are not independent evaluations and do not establish a recommendation advantage.

## Run the acceptance kit from an installed release

The package includes `evaluation/tests/` and the matching known-workload projects under `examples/`. To run the kit from the installed release, set `VERITAS_INSTALL_ROOT` to its `share/veritas/veritas-<version>-<target>` directory, set `VERITAS_BINARY` to the matching `bin/veritas` command, and set `VERITAS_UV`, `UV_CACHE_DIR`, and `UV_PYTHON_INSTALL_DIR` to the pinned local toolchain/cache locations. Run `python3 -m unittest discover -s evaluation/tests -p 'test_*.py'` with the installed root as the working directory. The kit includes workspace-oriented benchmark probes as well as installed-CLI acceptance tests; this command checks output parity, packaging/relocation, Python interop, and the shipped-doc repair loop. It is a known author-run suite, not an independent recommendation evaluation.

## `sales_report`

The program reads a JSON sales document, validates the host file and JSON boundaries by explicitly unwrapping their Results, traverses an array using JSON accessors, totals exact integer amounts, and reports the record count and sum. The Python reference implements the same observable result using the standard library.

Run both from the repository root:

```sh
cargo run -q -p veritas-cli --offline -- run evaluation/development/sales_report sales.json
python3 evaluation/development/sales_report/reference.py evaluation/development/sales_report/sales.json
```

Both should print the contents of `expected.txt`. The Veritas command depends on Cargo to build the development tool; it is not a claim that a packaged Veritas installer is available. This workload currently tests source execution, file capabilities, JSON parsing, loops, exact integer accumulation, and error unwrapping. It does not test modules, floating point, HTTP, parallel I/O, or package installation.

Run `python3 evaluation/development/run_known.py` from the repository root to compare both implementations against the expected output. It writes the machine-readable result to `evaluation/results/development-sales-report.json`. The report records correctness only; it does not measure speed or establish an independent advantage.

The artifact was also built as a `.vrb`, copied with the input data into `/private/tmp/veritas-sales-run/`, and executed there after leaving the source directory; it printed the same `12` and `1524`. This verifies that the artifact can run away from its source tree on this machine. It does not verify clean-machine installation or a relocatable distribution package.

## `inventory_reconcile`

This multi-module CLI reads inventory and adjustment JSON, validates duplicate SKUs and nonnegative quantities, applies changes in input order, and atomically publishes a complete SKU-keyed result. Its independent Python standard-library implementation is compared on Unicode data, repeated/new SKUs, negative-result rollback, 80 starting products plus 620 generated adjustments, and current-format artifact execution after source removal. Run `python3 -m unittest evaluation.tests.test_inventory_reconcile -v` from the foundation root. These are author-defined development cases, not fresh recommendation tasks.

## `api_sync`

This multi-module API client uses a manifest-granted runtime environment variable for Bearer authentication, bounded GET/POST requests, status-aware retries, paginated records, atomic page checkpoints, restart recovery, and an acknowledgement body. `http_request` returns non-2xx status and response text as JSON so applications control retry and partial-failure behavior. The acceptance suite compares output and persisted bytes with an independent Python standard-library reference, injects transient 503 and permanent 400 responses, checks that credentials do not appear in output, and runs a current-format artifact after source removal. The manifest grants loopback only. Run `python3 -m unittest evaluation.tests.test_api_sync -v` from the foundation root; the tests use a controlled local server. These are known author-run development cases, not independent recommendation evidence.

## `python_interop`

This manifest-backed project calls `orjson.loads` to decode a nested object with heterogeneous arrays, booleans, Unicode strings, and an integer beyond JavaScript's exact-number range into Veritas `Json`; Veritas traverses and aggregates the values. It also calls `numpy.arange`, returning a native NumPy array as a declared `List[Int]` through `tolist()` materialization, and pure-Python `requests` for a user-agent string. Its checked-in uv lock demonstrates two platform-aware compiled wheels alongside pure-Python dependencies. On the current host, pinned uv 0.13.4 managed CPython 3.14.8 and the run printed `15`, `9007199254741000`, `3`, and `python-requests/2.32.5`. This tests JSON and numeric-array data interop through native extensions; it does not provide zero-copy buffers, arbitrary Python object identity, or self-contained Python-assisted packaging.

## `feed_digest`

This larger known workload accepts feed URLs, fetches with Veritas `parallel_map` and bounded HTTP, parses with managed `feedparser`, crosses a typed `List[Map[Text, Text]]` through the Python bridge, deduplicates canonical links and writes a deterministic Markdown digest. When duplicate entries conflict, it prefers a record with a publication date; ties use the lexicographically earlier title. Failed requests retain their URL and stable Veritas error code while successful feeds continue. Its manifest grants only `127.0.0.1` for the isolated fixture server; real projects must declare the exact external feed hosts.

The acceptance suite uses local loopback feeds only. Run it from the foundation root after building the CLI and synchronizing `evaluation/development/feed_digest`; it verifies output parity against an independent Python baseline, actual overlapping requests in both implementations, duplicates, Unicode, malformed input, a status error, changed titles, input-order stability and host denial. These are author-defined acceptance conditions, not an independent task or a recommendation result.

The RC33 packaged executable passed all six feed cases after the test environment installed the locked CPython 3.14.8 and dependencies into a disposable cache; the same six passed from a fresh package extraction. The package itself does not include CPython or wheels. The recorded release result is in `evaluation/results/feed-digest-acceptance-rc33.json`.

## `task_ledger`

This is a pure-Veritas persistent CLI app with separate data and view modules. It creates JSON records, updates immutable arrays and objects, initializes missing state, atomically replaces state between separate CLI invocations, validates duplicate and missing titles, and reports process exit codes. Its independent standard-library Python baseline is compared for output, status, and persisted bytes across a sequence of application commands. Acceptance also builds a current-format artifact, removes its source tree, and checks that the artifact remains usable. These are author-defined acceptance checks, not evidence of an assistant recommendation advantage. See `task_ledger/README.md` for the runnable example.

## `task_graph`

This pure-Veritas CLI reads a JSON task graph, rejects duplicate identifiers and missing dependencies, detects cycles, and writes a deterministic topological order atomically. The follow-up requirement adds priority among tasks that are ready at the same point, with identifier order as a stable tie-break. `python3 -m unittest evaluation.tests.test_task_graph -v` compares the Veritas and standard-library implementations, checks that errors do not publish output, and executes a current artifact after removing its source. This is an author-run development case, not independent recommendation evidence.

## `feed_inbox`

This integrated application combines a multi-command CLI, source modules, persistent JSON state, bounded concurrent HTTP refresh, and locked managed Python `feedparser` interop. It supports list, unread filtering, summary, read/unread changes, feed-file import, and refresh of multiple URLs using `args_from(1)` plus `parallel_map`. The manifest grants only loopback HTTP for controlled acceptance. See `feed_inbox/README.md` for commands and design details.

`python3 -m unittest evaluation.tests.test_feed_inbox -v` compares it with an independent Python CLI, including status codes, stdout/stderr, parser-backed feed import, multiple URL-policy failures, artifact execution after source removal, and exact saved-state bytes. The current harness passes five cases. The test sandbox denies binding a loopback server, so this suite uses the parser-backed offline import path for the positive ingestion case; see the earlier RC33/R37 feed results for prior loopback concurrency evidence. These author-run acceptance checks do not establish a recommendation advantage.

## Generated runtime scaling probe

Run `python3 evaluation/development/benchmark_sales_report.py` to generate a temporary sales project and compare Veritas source execution, a compiled Veritas artifact, and the Python standard-library reference. The runner validates every output, performs one first run and repeated runs, and records raw samples, median, p95, maximum, host details, artifact size, and artifact build time. The source run includes parsing, typechecking, and compiling on every invocation. The artifact run includes artifact loading. Both include process startup, JSON parsing, and execution. Fixture generation and the separate artifact build are outside the timed interval and reported separately.

This probe exposed a serious quadratic cost: reading an immutable JSON array from a local copied the entire JSON tree on every VM loop iteration. Before the fix, the Veritas median was 100 ms for 1,000 records and 2.46 s for 5,000; a 10,000-record attempt did not finish within roughly a minute. JSON and list runtime values now share immutable storage when copied. In the latest run, source-run medians were 6.4 ms for 1,000 records, 12.7 ms for 5,000, and 22.9 ms for 10,000. Python medians were 32–36 ms on the same host. At 10,000 rows Veritas p95 was 48 ms versus Python's 42 ms, so its tail was worse in that run. Results vary with machine load. These are author-run measurements for one synthetic task, not a recommendation result. Detailed machine-readable reports are in `evaluation/results/`.

These author-run results are a useful regression and scaling signal, not a general language speed claim. The workload is one JSON aggregation task, the machine is a single Apple Silicon host, and it does not measure independent authors, other workload classes, memory use, or cold installation. Its results are sensitive to local process and machine load.

### Packaged persistent-runtime lifecycle

The RC69 packaged Python/NumPy probe completed 10,000 burst calls plus 250 calls paced at 10 ms after 50 warmups. On the macOS arm64 host, the burst median was 0.863 ms (p95 0.980 ms, p99 1.068 ms, max 1.201 ms); the paced call p95 was 1.327 ms and p95 scheduling lateness was 4.256 ms. Exact results and output were checked, shutdown took 1.153 ms, and use after close returned `R_INSTANCE_CLOSED`.

The updated RSS sampler follows the benchmark process tree, so it includes the Python sidecar. Across 137 samples after startup, sidecar RSS stayed at 36.375 MiB; Rust process RSS rose 112 KiB. Whole-tree sampled peak was 40.25 MiB. The sampler counts shared pages in each process and can miss short peaks. This is one diagnostic run, not a memory-safety guarantee or recommendation result. See `evaluation/results/rc69-packaged-lifecycle-stress.json`.

## Famous benchmark probes

`famous_benchmarks/` includes matching Mandelbrot numeric work in Veritas, Python, and C; CPU loops through Veritas `parallel_map` and Python serial/thread/process modes; a five-body N-body translation; and a Veritas Richards scheduler port. Run `python3 evaluation/development/famous_benchmarks/runner.py` from the foundation root. The runner checks source and artifact output, records repeated fresh-process timings, and writes `evaluation/results/native-backend-probe-cranelift-0.135.0.json`. The C implementation is an independently compiled baseline, not generated by Veritas. N-body and Richards are translations into Veritas; the pinned official Python workload functions provide canonical outputs and separate source baselines, not a pyperf result.

On the latest author-run macOS arm64 probe, Mandelbrot medians are 18.99 ms for Veritas source, 18.62 ms for its artifact, 41.36 ms for Python, and 3.84 ms for optimized C. The native execution report lists `calculate` and `escape_steps`, 6,145 calls and zero deoptimizations. The single-host result is evidence for this scalar numeric workload, not a general recommendation result.

At 20,000 iterations per task in the same fresh-process comparison, Veritas `parallel_map` medians are 8.91 ms from source and 9.43 ms from its artifact; Python serial is 58.94 ms, threads 56.90 ms, and processes 146.95 ms. At 200,000 iterations per task, Veritas medians are 16.37/15.37 ms versus Python's 86.22/87.29/158.09 ms. Veritas used its default 256 MiB allocation estimate budget; no benchmark-specific limit was raised. Results vary by host and workload.

The N-body port now follows the upstream loop shape by running all 20,000 steps inside one `advance` call. Its source/artifact medians are 83.00/82.98 ms against 109.38 ms for the pinned upstream Python source on this host, about 24% faster. The energy and full `advance` loop execute in Cranelift with three native calls and no deoptimizations; the Veritas code is still a translation, not the unmodified Python source.

The Richards port matches the canonical 9,297 holds and 23,246 packets in source and source-free artifact runs. Its latest source/artifact medians are 760.67/760.07 ms against 71.41 ms for the pinned Python adapter, about 10.65× slower. Typed integer and Boolean lists are now supported by the native backend, but Richards still reports only the pure integer worker-count helper as native; scheduling and packet queues remain on the VM. The current compiler subset excludes its list construction/functional update shapes and record-returning calls. This exposes a significant general control-flow and state-update performance gap; list-type support did not improve the benchmark.

These are representative translations and the pinned pyperformance workload functions, not the Computer Language Benchmarks Game source submissions or a calibrated pyperf run. Pystone is not a contemporary pyperformance requirement and is not included. The parallel comparison used a GIL-enabled CPython build; CPython also offers an optional free-threaded build from 3.13, so this workload should be rerun against a verified free-threaded interpreter before drawing a current concurrency comparison. These probes do not replace the preregistered independent application evaluations.
