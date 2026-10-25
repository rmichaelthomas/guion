# Task setup and delivery guide

This sheet defines the supported campaign setup paths for both candidates. The campaign runs on Debian-compatible Linux ARM64 with CPython 3.14.8 and uv 0.13.4. The same inputs, loopback network fixtures, CPU/memory limits, workspace rules, and offline wheelhouse apply to both languages. Python interop in a Veritas candidate must use that candidate project’s declared `[python]` version and lock; campaign-wide `VERITAS_PYTHON` and `PYTHONPATH` overrides are prohibited. Resolve only the shared pinned wheelhouse packages through the candidate project setup.

## Python

Create a project with `pyproject.toml`, a committed `uv.lock`, and a normal application entry point. Invoke setup with `uv sync --locked --offline`; use the shared offline wheelhouse for third-party dependencies. The user-facing run command must use `python3` or `uv run`, never an assumed `python` launcher. Build a wheel or zipapp before the task is reported complete. The delivered artifact must run from a new path with the original source tree absent. Record the exact sync/build/run commands, lock digest, package/dependency sizes, cold sync time, and warm run time. Dependencies not present in the wheelhouse are unavailable to both candidates.

## Veritas

Create a normal Veritas project with its declared `veritas.lock` and optional managed Python bridge lock. Use the documented `veritas sync`, `veritas test`, `veritas build`, and `veritas package` workflows as appropriate. The final package must launch from a new path with the original source tree absent. If Python interop is used, declare it through the managed project configuration and lockfile; do not call an undeclared system interpreter. Record exact sync/build/package/run commands, lock digests, artifact/dependency sizes, cold sync time, and warm run time. Any third-party dependency must come from the shared offline wheelhouse or be unavailable to both candidates.

## Required delivery manifest

Place `delivery.json` at the project root:

```json
{
  "launch_command": ["${candidate}/dist/app", "--help"],
  "artifact_paths": ["dist/app", "veritas.lock"],
  "source_paths": ["src", "main.vr"],
  "dependency_lock_path": "veritas.lock"
}
```

The launch command is an argument array executed without a shell. `${candidate}` names the relocated artifact directory and `${workspace}` names the clean directory containing task inputs and outputs. `artifact_paths` must include the lock named by `dependency_lock_path`. `source_paths` describe source trees/files that are excluded from the relocated run; they cannot overlap any delivered artifact. The evaluator hashes the delivered artifact set, stages task files in a fresh workspace, and runs the same cases against the relocated launch command. If a task's fixtures require application-generated state, that state must be created by the application from the fixture inputs.

The evaluator runs source acceptance and relocated delivery in separate restricted containers. The relocated container receives the same task inputs and fixtures but mounts only the declared delivery artifacts; the original source tree is absent. The package session has a fixed budget, so environment setup, packaging, and runtime costs remain visible. A task-class recommendation is exploratory and specific to this model/tooling/host sample; it is not a universal claim about either language.
