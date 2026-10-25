# Independent case runner

`case_runner.py` executes one declarative candidate case in a fresh temporary workspace, writes the case's input files, runs a configured argv array without a shell, and checks the process result and declared output predicates. In its local default mode it also stages `delivery.json` artifacts and reruns each case; this is a runner unit check, not an OS isolation boundary. The scored `run-case.sh` launcher stages artifacts on the host, runs the source project in one restricted container, and runs the relocated artifact in a second restricted container. The second container mounts only the delivery tree and its generated launch configuration; the original project is not mounted. Both runs receive the same manifest, case inputs, loopback fixture network, candidate UID, and resource limits. A missing lock, artifact, launch command, or source-free launch failure fails delivery acceptance.

```json
{"command": ["uv", "run", "--locked", "--offline", "--project", "${candidate}", "python3", "${candidate}/main.py"]}
```

For a Veritas candidate, pass its installed binary as data rather than embedding it in shell text:

```json
{"binary": "/path/to/veritas", "command": ["${binary}", "run", "${candidate}/main.vr"]}
```

Scored delivery requires `delivery.json` at the candidate project root. Its exact shape is:

```json
{
  "launch_command": ["python3", "${candidate}/main.py"],
  "artifact_paths": ["main.py", "requirements.lock"],
  "source_paths": ["src/"],
  "dependency_lock_path": "requirements.lock"
}
```

This minimal example is for a self-contained, standard-library-only entry script; `main.py` is explicitly shipped as a runnable artifact, and `requirements.lock` may be empty. A packaged application with dependencies must instead include its working launcher, project metadata, application artifact, and complete lockfile. `launch_command` is a non-empty argv array, never shell text. `artifact_paths` must be non-empty and include the dependency lock. `source_paths` must identify source files or trees omitted from the relocated copy; a human reviewer checks that declaration against the project because the runner cannot infer which files are source. Every path must stay inside the candidate project. The runner copies only `artifact_paths` into a clean relocated tree, omits the declared source paths, stages the same case inputs, and runs every case using `launch_command`. It hashes the staged artifact tree and records its byte count. A candidate that can run only while its original source tree is present fails delivery acceptance.

Project-scoped runtimes can set `workspace_root_project: true`. The runner copies the candidate project into the fresh workspace root, then materializes fixtures beside it; `${candidate}` resolves to that same root. This matches a runtime whose file permissions are rooted at its project directory while preserving relative fixture paths. Candidate project files may not collide with fixture input files. Record this mode and its copy/setup time in campaign evidence.

Run one task stage with:

```sh
python3 evaluation/protocol/case_runner.py \
  evaluation/fixtures/independent-v1/manifest.json \
  01-support-sla-report initial candidate-python.json candidate-project/
```

Each fixture case has an `id`, `files` mapping relative paths to text, optional `binary_files` and `binary_mutations` mappings relative paths to base64 bytes, optional workspace-local `symlinks`, optional `command_args` or ordered `steps`, an expected exit code, and non-empty executable checks. Symlink targets must resolve inside the case workspace. A case can run setup commands, then apply text/binary fixture mutations to model an application's follow-up migration. `http_fixtures` starts named loopback-only HTTP servers with deterministic route responses; fixture file text and arguments can refer to them as `${http:name}`. A route can use an ordered response list for retries, `delay_ms` for delayed responses, or `chunks` with `chunk_delay_ms` for slow-trickle timeout cases. Supported checks are `stdout_contains`, `stderr_contains`, `stderr_not_contains`, `file_exists`, `file_absent`, `file_equals`, `file_contains`, `file_bytes_not_contains`, `toml_equals`, `json_equals`, `json_path_equals`, `json_path_absent`, `http_request_count_equals`, `http_max_in_flight_at_most`, and `http_max_in_flight_at_least`. `json_equals` compares the complete parsed JSON document, including its keys and values; `json_equals` and `json_path_equals` preserve JSON types, so `true`, `1`, and `1.0` are distinct. `file_bytes_not_contains` checks raw file bytes without text decoding. `toml_equals` compares parsed TOML values so valid serializer whitespace does not cause false failures; pair it with `file_contains` when comments must also be preserved. Paths are constrained to normalized relative paths inside the temporary workspace, and every execution has a timeout. The JSON report records candidate metadata, per-step and overall elapsed time, HTTP request paths and concurrency, and preserves failed cases. Configured secrets are redacted from captured output and request paths.

All nine tasks currently have initial and follow-up cases in this executable shape (127 cases total). A manifest being accepted by the runner proves that its cases can be executed; it does not establish that the tasks are fair, that expected results are correct, or that either has independent review.

## Execution safety

The fresh working directory and argument-array invocation are isolation conveniences, not an operating-system security sandbox. A candidate process can still access the host filesystem, environment, and network available to the runner. Use only reviewed, trusted candidate programs with this local runner. For untrusted generated candidates, place the candidate and runner inside a separately configured OS/container sandbox with restricted filesystem and network access; give both languages equivalent capabilities and retain the sandbox transcript as evidence. Do not claim an isolated temp directory provides containment.

## Restricted Linux campaign runner

The evaluation/sandbox Docker setup runs the case runner inside a pinned Debian/Python image. It uses no external network, a read-only root filesystem, bounded writable temporary filesystems, dropped Linux capabilities, no-new-privileges, and fixed CPU, memory, and process limits. `/tmp` is mounted `noexec`; `/candidate-tmp` is a separate executable tmpfs and is supplied as `TMPDIR` so the runner can relocate and launch packaged executables without weakening `/tmp`'s policy. Their combined size is 896 MiB, allowing a complete candidate workspace up to 768 MiB while keeping the existing 2 GiB container memory limit. Loopback fixtures remain available because the case runner and candidate share the container network namespace. The Veritas image removes the packaged evaluation directory before a candidate can inspect the installation. Both candidate languages run CPython 3.14.8; the image includes its read-only uv-managed runtime so Veritas projects can sync their exact managed Python version offline.

The runner reads the manifest and candidate command as root from root-only files. The --candidate-uid 10001 option gives the case files and candidate project to an unprivileged child process and launches each setup/run command as that UID. The temporary root remains owned by the runner, with a sticky writable permission for candidate output; the candidate receives a scrubbed environment and no Linux capabilities. Limited capabilities are granted to the trusted runner process for workspace ownership and cleanup, then cleared when it changes UID for candidate execution. Candidate code and its own project files remain readable and writable inside the case workspace; host paths, evaluator files, credentials, and the Docker socket are not mounted into the candidate container.

The campaign images include the same read-only dependency bundle and wheelhouse: icalendar 6.3.1, python-dateutil 3.0.0, six 1.17.0, tzdata 2026.5, and tomlkit 0.14.0. Both images also include uv 0.13.4. Python source runs use `uv run --locked --offline --project ${candidate} python3 ${candidate}/main.py`, with no global dependency path, so the candidate's `pyproject.toml` and `uv.lock` determine installed imports. The Veritas project-aware `veritas run` command reads its declared `veritas.lock`; the managed Python bridge remains available only through the Veritas config. Dependencies resolve from the shared offline wheelhouse, but fresh dependency download time is not measured. Review whether this shared preloaded set gives equivalent setup opportunities for each task.

Build the two ARM64 images from the archive and digest pinned in `evaluation/sandbox/campaign-lock.json` with:

    sh evaluation/sandbox/build-images.sh /path/to/veritas-0.1.0-linux-aarch64.tar.gz

Run a scored stage using an assistant-produced project directory and save its JSON report under the campaign evidence root. The launcher preserves the declared delivery tree and a copy of `delivery.json` beside the report. Its command arguments bind the report to pair ID, assistant session ID, candidate version, manifest digest, and schedule digest. The report also records language, task, stage, Docker Engine ID, exact container image ID, package archive digest, base image digest, platform, network mode, privilege boundary, resource limits, source and delivery outcomes, and raw case-runner results. The evidence ledger checks the report hash, assigned identifiers, captured delivery manifest hash, and relocated artifact tree hash. The delivery run mounts only the staged artifact tree, with no access to the original candidate project. Failed candidate attempts remain reportable.

    sh evaluation/sandbox/run-case.sh python 01-support-sla-report initial pair-001 session-001 '3.14.8 with uv' <manifest-sha256> <schedule-sha256> /path/to/candidate evaluation/results/independent-v2/runs/pair-001-python-initial.json

Use the same host, Docker engine, image base, resource limits, and fixture network for both languages. The image targets Debian 12-compatible ARM64 and CPython 3.14.8. This is an execution boundary, not proof that tasks or expected outcomes are fair; the independent reviewer must still approve the protocol before comparative runs.
