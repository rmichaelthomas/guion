# Guion source-run performance workspace

The source-run and launch study uses one repository-local workspace so the
Rust build and generated benchmark inputs stay together. It uses the pinned
Python 3.14.7 standard library and the locked Rust dependencies already
available offline. It does not create a Python virtual environment, download
packages, start a container for the macOS measurements, or write benchmark
scratch under `/tmp`.

## Run a session

Run these commands from the Guion repository root:

```sh
python3 evaluation/completion/performance/session.py preflight
python3 evaluation/completion/performance/session.py baseline
python3 evaluation/completion/performance/session.py compare <before-report.json> <after-report.json>
python3 evaluation/completion/performance/session.py clean
```

`preflight` checks the repository root, Python version, free disk, repo-local
scratch access, and offline Cargo dependencies before a build or benchmark
starts. `baseline` builds `guion-cli` in release mode once for the current
source and tool identity, then reuses that binary while running the
correctness-checked suite, launch measurement, compute source/artifact/Python
lanes, and the file/SQLite agent examples. Each run writes a new report under
`evaluation/completion/results/source-loop/`; existing and frozen reports are
never replaced. Reports include their source and binary identities, exact
commands, tool and target versions, cache state, samples, medians, p95, and
generated scratch size.

The source lanes distinguish the first timed source run, repeated source
processes, prebuilt artifact processes, a first timed Python process, and
repeated Python processes. Correctness is checked for every measured output.
The paired repeated-source, artifact, and Python trials use a deterministic
randomized order (seven repetitions for the added source and application
lanes; the established suite retains its 15 compute and 16 data trials). OS
file-cache state is not artificially cleared and is reported as uncontrolled.
Once the source-run cache is available, the cold source lane uses
`guion run --no-cache`; the repeat lane records cache hits. `compare` reports
median and p95 deltas for launch, source, artifact, Python, application, and
established-suite lanes without changing either input report.

All build and temporary data belongs to the ignored `target/perf-session/`
tree. `clean` refuses to remove it while the recorded session process is
running and deletes no other target directory or report. The release binary
is reused only when the Git revision, build-input fingerprint, Rust tool
versions, target, and binary hash still match.

The lower-level runner keeps its existing default command behavior. Its
optional `--scratch-dir` and `--output` arguments route generated files to an
explicit session location when called by the session harness.

## Evidence

Compact, immutable result cohorts live under
`evaluation/completion/results/source-loop/`. The pre-release performance
reports under `evaluation/completion/results/guion-final/` are historical and
remain unchanged. A report is a workload-specific measurement, not a claim
that Guion is universally faster than Python.
