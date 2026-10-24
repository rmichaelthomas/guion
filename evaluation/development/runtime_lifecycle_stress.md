# Persistent Python and NumPy lifecycle stress

The packaged runner measures repeated calls through a single Veritas VM and a persistent Python sidecar. Each call asks NumPy for an integer array, materializes it as the declared Veritas list, sums it in Veritas, and checks both return value and printed output. It reports burst throughput separately from paced scheduling, samples RSS for the Rust process and all descendants (including the Python sidecar), then checks shutdown and use-after-close behavior. Process-tree samples sum each process's RSS; shared pages can therefore be counted more than once, and brief peaks can be missed.

From the package root, prepare the managed Python environment and run the packaged benchmark:

```sh
VERITAS_UV=./toolchains/uv ./veritas sync ./examples/python_interop
python3 evaluation/development/benchmark_runtime_lifecycle.py \
  --binary evaluation/bin/runtime_lifecycle_stress \
  --python examples/python_interop/.veritas/python-bridge/.venv/bin/python3 \
  --array-size 512 --warmups 50 --burst-calls 10000 \
  --paced-calls 250 --pace-ms 10
```

The runner writes a JSON report under `evaluation/results/`. It uses `ps -axo pid=,ppid=,rss=` to discover the root process and descendants, sampling every 50 ms. Restricted sandboxes can deny process inspection; in that case the report records that memory trend could not be collected. A successful run is author-run diagnostic evidence for this bridge path, not an independent language recommendation comparison.

On the RC69 macOS arm64 package extracted under a path containing spaces and Unicode, the 50-warmup, 10,000-burst, 250-call/10-ms-paced run passed exact output and arithmetic checks. The burst p50/p95/p99/max were 0.863/0.980/1.068/1.201 ms at 1,145 calls/s; paced call p95 was 1.327 ms, with p95 scheduling lateness 4.256 ms. Shutdown took 1.153 ms and post-close use returned `R_INSTANCE_CLOSED`. After the first second, the Python sidecar RSS was 36.375 MiB at every one of 137 samples; the Rust process moved from 3.781 MiB to 3.891 MiB, for a process-tree RSS change of 112 KiB. The sampled whole-run process-tree peak was 40.25 MiB. This single run does not establish a general memory bound or an assistant recommendation advantage. Raw results: `evaluation/results/rc69-packaged-lifecycle-stress.json`.
