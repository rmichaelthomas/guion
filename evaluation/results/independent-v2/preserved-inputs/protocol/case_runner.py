"""Run a candidate command against an isolated case and verify its artifacts.

Candidate configuration is JSON with an argv array, for example:
{"command": ["uv", "run", "--locked", "--offline", "--project", "${candidate}", "python3", "${candidate}/main.py"]}
No shell is involved. Each case gets a fresh working directory and a copied
candidate tree. The runner is an execution primitive; task-specific expected
outputs still need review and must not be mistaken for independent evidence.
"""

from __future__ import annotations

import argparse
import base64
import binascii
import hashlib
from contextlib import contextmanager
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import signal
import shutil
import subprocess
import sys
import tempfile
import threading
import time
import tomllib
from typing import Any


def _safe_path(root: Path, relative: Any) -> Path:
    if not isinstance(relative, str) or not relative:
        raise ValueError("fixture path must be a non-empty string")
    relative_path = Path(relative)
    if relative_path.is_absolute() or ".." in relative_path.parts:
        raise ValueError(f"fixture path must be a normalized relative path: {relative}")
    candidate = (root / relative).resolve()
    if candidate != root and root not in candidate.parents:
        raise ValueError(f"fixture path escapes the case workspace: {relative}")
    return candidate


def _prepare_delivery(root: Path, artifact_root: Path | None = None) -> tuple[Path, dict[str, Any]]:
    """Stage only declared distributables into a clean, source-free candidate tree."""
    manifest_path = root / "delivery.json"
    if not manifest_path.is_file():
        raise ValueError("candidate project must provide delivery.json")
    spec = json.loads(manifest_path.read_text(encoding="utf-8"))
    if not isinstance(spec, dict):
        raise ValueError("delivery.json must be an object")
    command = spec.get("launch_command")
    artifacts = spec.get("artifact_paths")
    sources = spec.get("source_paths")
    lock = spec.get("dependency_lock_path")
    if not isinstance(command, list) or not command or not all(isinstance(x, str) and x for x in command):
        raise ValueError("delivery.json launch_command must be a non-empty string array")
    if not isinstance(artifacts, list) or not artifacts or not all(isinstance(x, str) and x for x in artifacts):
        raise ValueError("delivery.json artifact_paths must be a non-empty string array")
    if not isinstance(sources, list) or not all(isinstance(x, str) and x for x in sources):
        raise ValueError("delivery.json source_paths must be a string array")
    if not sources:
        raise ValueError("delivery.json source_paths must identify the original source tree or files")
    if not isinstance(lock, str) or not lock:
        raise ValueError("delivery.json dependency_lock_path is required")
    owns_artifact_root = artifact_root is None
    artifact_root = Path(tempfile.mkdtemp(prefix="veritas-delivery-artifacts-")) if owns_artifact_root else artifact_root
    artifact_root.mkdir(parents=True, exist_ok=True)
    if any(artifact_root.iterdir()):
        raise ValueError("delivery staging directory must be empty")
    included: set[str] = set()
    try:
        for relative in artifacts:
            source = _safe_path(root, relative)
            canonical_relative = source.relative_to(root).as_posix()
            if not source.exists():
                raise ValueError(f"declared delivery artifact is missing: {relative}")
            if source.is_dir():
                shutil.copytree(source, artifact_root / canonical_relative, symlinks=True)
                included.update(path.relative_to(root).as_posix() for path in source.rglob("*"))
                included.add(canonical_relative)
            elif source.is_file():
                target = artifact_root / canonical_relative
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(source, target)
                included.add(canonical_relative)
            else:
                raise ValueError(f"delivery artifact is not a regular file or directory: {relative}")
        if lock not in included:
            raise ValueError("dependency_lock_path must be included in artifact_paths")
        for source_path in sources:
            source = _safe_path(root, source_path)
            if not source.exists():
                raise ValueError(f"declared source path is missing: {source_path}")
            source_rel = source.relative_to(root).as_posix()
            if source_rel in included or any(item.startswith(source_rel.rstrip("/") + "/") for item in included):
                raise ValueError(f"source path appears in delivered artifact set: {source_path}")
        digest = hashlib.sha256()
        total_bytes = 0
        for path in sorted(artifact_root.rglob("*")):
            if path.is_symlink():
                payload = b"symlink:" + str(path.readlink()).encode("utf-8")
            elif path.is_file():
                payload = path.read_bytes()
                total_bytes += len(payload)
            else:
                continue
            relative = path.relative_to(artifact_root).as_posix().encode("utf-8")
            digest.update(relative + b"\0" + hashlib.sha256(payload).digest())
        return artifact_root, {"command": command, "metadata": {"delivery_manifest": "delivery.json", "artifact_paths": artifacts, "source_paths": sources, "dependency_lock_path": lock, "artifact_sha256": digest.hexdigest(), "artifact_bytes": total_bytes}}
    except BaseException:
        if owns_artifact_root:
            shutil.rmtree(artifact_root, ignore_errors=True)
        raise


def _chown_workspace_entries(root: Path, uid: int, gid: int) -> None:
    """Give candidate data to the dropped-privilege child but keep the temp root owned by the runner."""
    for directory, child_dirs, files in os.walk(root, followlinks=False):
        for name in child_dirs + files:
            os.chown(Path(directory) / name, uid, gid, follow_symlinks=False)
    os.chmod(root, 0o1777)


def _run_candidate_process(
    argv: list[str],
    *,
    cwd: Path,
    env: dict[str, str],
    timeout_seconds: float,
    identity: dict[str, Any],
) -> subprocess.CompletedProcess[str]:
    """Run a candidate and clean up its process group before the next case step."""
    if not identity:
        return subprocess.run(
            argv,
            cwd=cwd,
            env=env,
            text=True,
            capture_output=True,
            timeout=timeout_seconds,
            check=False,
        )

    process = subprocess.Popen(
        argv,
        cwd=cwd,
        env=env,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        start_new_session=True,
        **identity,
    )
    try:
        stdout, stderr = process.communicate(timeout=timeout_seconds)
    except subprocess.TimeoutExpired as exc:
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        stdout, stderr = process.communicate()
        raise subprocess.TimeoutExpired(
            argv, timeout_seconds, output=stdout or exc.output, stderr=stderr or exc.stderr
        ) from exc
    finally:
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
    return subprocess.CompletedProcess(argv, process.returncode, stdout, stderr)


def _json_pointer(value: Any, pointer: str) -> Any:
    if pointer == "":
        return value
    if not isinstance(pointer, str) or not pointer.startswith("/"):
        raise ValueError("JSON path must be an RFC 6901 pointer beginning with '/'")
    current = value
    for raw_part in pointer[1:].split("/"):
        part = raw_part.replace("~1", "/").replace("~0", "~")
        if isinstance(current, list):
            current = current[int(part)]
        elif isinstance(current, dict):
            current = current[part]
        else:
            raise KeyError(f"cannot traverse JSON value at {part!r}")
    return current


def _json_values_equal(actual: Any, expected: Any) -> bool:
    """Compare JSON values without Python's bool/int or int/float coercions."""
    if type(actual) is not type(expected):
        return False
    if isinstance(actual, dict):
        return actual.keys() == expected.keys() and all(
            _json_values_equal(actual[key], expected[key]) for key in actual
        )
    if isinstance(actual, list):
        return len(actual) == len(expected) and all(
            _json_values_equal(left, right) for left, right in zip(actual, expected)
        )
    return actual == expected


@contextmanager
def _http_fixtures(fixtures: Any):
    """Start declarative loopback-only HTTP fixtures for one case."""
    if not isinstance(fixtures, dict):
        raise ValueError("http_fixtures must be an object")
    servers: dict[str, ThreadingHTTPServer] = {}
    threads: list[threading.Thread] = []
    state: dict[str, dict[str, Any]] = {}
    substitutions: dict[str, str] = {}
    try:
        for name, routes in fixtures.items():
            if not isinstance(name, str) or not name or not isinstance(routes, dict):
                raise ValueError("each HTTP fixture must map a name to a routes object")
            state[name] = {"requests": [], "route_counts": {}, "in_flight": 0, "max_in_flight": 0, "lock": threading.Lock()}

            def handler_factory(fixture_name: str, fixture_routes: dict[str, Any]):
                class FixtureHandler(BaseHTTPRequestHandler):
                    protocol_version = "HTTP/1.1"

                    def do_GET(self):
                        self._respond()

                    def do_POST(self):
                        self._respond()

                    def do_PUT(self):
                        self._respond()

                    def do_DELETE(self):
                        self._respond()

                    def _respond(self):
                        fixture_state = state[fixture_name]
                        route_key = self.path.split("?", 1)[0]
                        route = fixture_routes.get(route_key)
                        with fixture_state["lock"]:
                            fixture_state["in_flight"] += 1
                            fixture_state["max_in_flight"] = max(fixture_state["max_in_flight"], fixture_state["in_flight"])
                            fixture_state["requests"].append({"method": self.command, "path": self.path})
                            request_index = fixture_state["route_counts"].get(route_key, 0)
                            fixture_state["route_counts"][route_key] = request_index + 1
                        try:
                            if route is None:
                                response = {"status": 404, "body": "not found"}
                            elif isinstance(route, list):
                                # A response list supports retry scenarios; repeat the final response afterward.
                                response = route[min(request_index, len(route) - 1)] if route else {"status": 500, "body": "empty response sequence"}
                            else:
                                response = route
                            if not isinstance(response, dict):
                                response = {"status": 500, "body": "invalid fixture response"}
                            delay_ms = response.get("delay_ms", 0)
                            if isinstance(delay_ms, (int, float)) and delay_ms > 0:
                                time.sleep(min(delay_ms / 1000, 10))
                            body = response.get("body", "")
                            if isinstance(body, (dict, list)):
                                body = json.dumps(body, ensure_ascii=False)
                            if not isinstance(body, str):
                                body = str(body)
                            encoded = body.encode("utf-8")
                            chunks = response.get("chunks")
                            if isinstance(chunks, list) and all(isinstance(chunk, str) for chunk in chunks):
                                encoded_chunks = [chunk.encode("utf-8") for chunk in chunks]
                                encoded = b"".join(encoded_chunks)
                            else:
                                encoded_chunks = None
                            self.send_response(int(response.get("status", 200)))
                            headers = response.get("headers", {})
                            if not isinstance(headers, dict):
                                headers = {}
                            for header, value in headers.items():
                                self.send_header(str(header), str(value))
                            if not any(str(header).lower() == "content-length" for header in headers):
                                self.send_header("Content-Length", str(len(encoded)))
                            self.send_header("Connection", "close")
                            self.end_headers()
                            if encoded_chunks is None:
                                self.wfile.write(encoded)
                            else:
                                chunk_delay_ms = response.get("chunk_delay_ms", 0)
                                for chunk in encoded_chunks:
                                    if isinstance(chunk_delay_ms, (int, float)) and chunk_delay_ms > 0:
                                        time.sleep(min(chunk_delay_ms / 1000, 10))
                                    self.wfile.write(chunk)
                                    self.wfile.flush()
                        except (BrokenPipeError, ConnectionResetError):
                            pass
                        finally:
                            with fixture_state["lock"]:
                                fixture_state["in_flight"] -= 1

                    def log_message(self, _format, *_args):
                        return

                return FixtureHandler

            server = ThreadingHTTPServer(("127.0.0.1", 0), handler_factory(name, routes))
            server.daemon_threads = True
            servers[name] = server
            substitutions[f"${{http:{name}}}"] = f"http://127.0.0.1:{server.server_port}"
            thread = threading.Thread(target=server.serve_forever, name=f"case-http-{name}", daemon=True)
            thread.start()
            threads.append(thread)
        yield substitutions, state
    finally:
        for server in servers.values():
            server.shutdown()
            server.server_close()
        for thread in threads:
            thread.join(timeout=2)


def _check(check: Any, workspace: Path, stdout: str, stderr: str, http_state: dict[str, Any]) -> tuple[bool, str]:
    if not isinstance(check, dict):
        return False, "check must be an object"
    kind = check.get("kind")
    try:
        if kind == "stdout_contains":
            ok = check["value"] in stdout
        elif kind == "stderr_contains":
            ok = check["value"] in stderr
        elif kind == "stderr_not_contains":
            ok = check["value"] not in stderr
        elif kind == "file_exists":
            ok = _safe_path(workspace, check["file"]).is_file()
        elif kind == "file_absent":
            ok = not _safe_path(workspace, check["file"]).exists()
        elif kind == "file_equals":
            ok = _safe_path(workspace, check["file"]).read_text(encoding="utf-8") == check["value"]
        elif kind == "file_contains":
            ok = check["value"] in _safe_path(workspace, check["file"]).read_text(encoding="utf-8")
        elif kind == "file_bytes_not_contains":
            needle = check["value"]
            if not isinstance(needle, str) or not needle:
                raise ValueError("value must be a non-empty string")
            ok = needle.encode("utf-8") not in _safe_path(workspace, check["file"]).read_bytes()
        elif kind == "toml_equals":
            data = tomllib.loads(_safe_path(workspace, check["file"]).read_text(encoding="utf-8"))
            ok = data == check["value"]
        elif kind == "json_path_equals":
            data = json.loads(_safe_path(workspace, check["file"]).read_text(encoding="utf-8"))
            ok = _json_values_equal(_json_pointer(data, check["path"]), check["value"])
        elif kind == "json_equals":
            data = json.loads(_safe_path(workspace, check["file"]).read_text(encoding="utf-8"))
            ok = _json_values_equal(data, check["value"])
        elif kind == "json_path_absent":
            data = json.loads(_safe_path(workspace, check["file"]).read_text(encoding="utf-8"))
            try:
                _json_pointer(data, check["path"])
                ok = False
            except (KeyError, IndexError):
                ok = True
        elif kind == "http_request_count_equals":
            fixture = http_state[check["fixture"]]
            route = check.get("path")
            count = sum(1 for item in fixture["requests"] if route is None or item["path"].split("?", 1)[0] == route)
            ok = count == check["value"]
        elif kind == "http_max_in_flight_at_most":
            ok = http_state[check["fixture"]]["max_in_flight"] <= check["value"]
        elif kind == "http_max_in_flight_at_least":
            ok = http_state[check["fixture"]]["max_in_flight"] >= check["value"]
        else:
            return False, f"unsupported check kind: {kind!r}"
    except (KeyError, IndexError, OSError, UnicodeError, ValueError, TypeError, json.JSONDecodeError) as exc:
        return False, f"{kind or 'check'} could not be evaluated: {exc}"
    if not ok:
        return False, f"{kind} did not match its expected value"
    return True, ""


def run_case(
    case: Any,
    candidate: Any,
    candidate_root: Path,
    *,
    timeout_seconds: float = 60,
    candidate_uid: int | None = None,
) -> dict[str, Any]:
    """Execute one case, returning a machine-readable result without raising."""
    case_id = case.get("id") if isinstance(case, dict) else None
    failures: list[str] = []
    if not isinstance(case, dict):
        return {"case_id": None, "passed": False, "failures": ["case must be an object"]}
    if not isinstance(candidate, dict):
        return {"case_id": case_id, "passed": False, "failures": ["candidate configuration must be an object"]}
    command = candidate.get("command")
    workspace_root_project = candidate.get("workspace_root_project", False)
    if not isinstance(workspace_root_project, bool):
        return {"case_id": case_id, "passed": False, "failures": ["workspace_root_project must be a Boolean"]}
    if not isinstance(command, list) or not command or not all(isinstance(item, str) and item for item in command):
        return {"case_id": case_id, "passed": False, "failures": ["candidate command must be a non-empty array of strings"]}
    if not candidate_root.is_dir():
        return {"case_id": case_id, "passed": False, "failures": [f"candidate directory does not exist: {candidate_root}"]}
    if candidate_uid is not None:
        if isinstance(candidate_uid, bool) or not isinstance(candidate_uid, int) or candidate_uid <= 0:
            return {"case_id": case_id, "passed": False, "failures": ["candidate_uid must be a positive integer"]}
        if os.name != "posix" or os.geteuid() != 0:
            return {"case_id": case_id, "passed": False, "failures": ["candidate_uid requires a POSIX runner with root privileges"]}

    files = case.get("files", {})
    binary_files = case.get("binary_files", {})
    checks = case.get("checks", [])
    command_args = case.get("command_args", [])
    configured_steps = case.get("steps")
    setup = case.get("setup", [])
    mutations = case.get("mutations", {})
    binary_mutations = case.get("binary_mutations", {})
    delete_files = case.get("delete_files", [])
    symlinks = case.get("symlinks", {})
    redactions = case.get("redact", [])
    http_fixtures = case.get("http_fixtures", {})
    started_at = time.monotonic()
    if not isinstance(files, dict) or not isinstance(binary_files, dict) or not isinstance(checks, list) or not isinstance(command_args, list):
        return {"case_id": case_id, "passed": False, "failures": ["files, binary_files, checks, and command_args have invalid types"]}
    if not isinstance(redactions, list) or not all(isinstance(item, str) and item for item in redactions):
        return {"case_id": case_id, "passed": False, "failures": ["redact must be an array of non-empty strings"]}
    if not isinstance(setup, list) or not isinstance(mutations, dict) or not isinstance(binary_mutations, dict) or not isinstance(delete_files, list) or not isinstance(symlinks, dict):
        return {"case_id": case_id, "passed": False, "failures": ["setup, mutations, binary_mutations, or delete_files have invalid types"]}
    if set(files) & set(binary_files) or set(mutations) & set(binary_mutations):
        return {"case_id": case_id, "passed": False, "failures": ["text and binary fixtures cannot target the same path"]}
    if configured_steps is None:
        if not checks:
            return {"case_id": case_id, "passed": False, "failures": ["case must define at least one executable check"]}
        steps = [{"command_args": command_args, "expected_exit_code": case.get("expected_exit_code", 0), "checks": checks}]
    elif isinstance(configured_steps, list) and configured_steps:
        steps = configured_steps
    else:
        return {"case_id": case_id, "passed": False, "failures": ["steps must be a non-empty array"]}
    for index, step in enumerate(steps):
        if not isinstance(step, dict) or not isinstance(step.get("command_args", []), list) or not all(isinstance(item, str) for item in step.get("command_args", [])):
            return {"case_id": case_id, "passed": False, "failures": [f"step {index + 1} command_args must be an array of strings"]}
        if not isinstance(step.get("checks"), list) or not step["checks"]:
            return {"case_id": case_id, "passed": False, "failures": [f"step {index + 1} must define executable checks"]}
        expected_exit = step.get("expected_exit_code", 0)
        if isinstance(expected_exit, bool) or not isinstance(expected_exit, int):
            return {"case_id": case_id, "passed": False, "failures": [f"step {index + 1} expected_exit_code must be an integer"]}

    stdout = ""
    stderr = ""
    exit_code: int | None = None
    checks_passed = 0
    checks_total = 0
    step_results: list[dict[str, Any]] = []
    http_state: dict[str, Any] = {}
    with tempfile.TemporaryDirectory(
        prefix="veritas-independent-case-", ignore_cleanup_errors=candidate_uid is not None
    ) as temp, _http_fixtures(http_fixtures) as (http_substitutions, http_state):
        workspace = Path(temp).resolve()
        candidate_copy = workspace if workspace_root_project else workspace / "candidate"
        try:
            shutil.copytree(candidate_root, candidate_copy, symlinks=True, dirs_exist_ok=workspace_root_project)
            fixture_paths = set(files) | set(binary_files)
            collisions = [relative for relative in fixture_paths if _safe_path(workspace, relative).exists()]
            if collisions:
                raise ValueError(f"candidate project files collide with fixtures: {', '.join(sorted(collisions))}")
            for relative, content in files.items():
                path = _safe_path(workspace, relative)
                if not isinstance(content, str):
                    raise ValueError(f"fixture content for {relative!r} must be text")
                path.parent.mkdir(parents=True, exist_ok=True)
                for token, replacement in {**{"${workspace}": str(workspace), "${candidate}": str(candidate_copy)}, **http_substitutions}.items():
                    content = content.replace(token, replacement)
                path.write_text(content, encoding="utf-8")
            for relative, encoded in binary_files.items():
                path = _safe_path(workspace, relative)
                if not isinstance(encoded, str):
                    raise ValueError(f"binary fixture for {relative!r} must be base64 text")
                raw = base64.b64decode(encoded, validate=True)
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(raw)
            for relative, target in symlinks.items():
                if not isinstance(relative, str) or not relative or not isinstance(target, str) or not target:
                    raise ValueError("symlink paths and targets must be non-empty strings")
                link_path = workspace / relative
                if workspace not in link_path.parent.resolve().parents and link_path.parent.resolve() != workspace:
                    raise ValueError(f"symlink path escapes the case workspace: {relative}")
                link_path.parent.mkdir(parents=True, exist_ok=True)
                target_path = (link_path.parent / target).resolve()
                if target_path != workspace and workspace not in target_path.parents:
                    raise ValueError(f"symlink target escapes the case workspace: {target}")
                if link_path.exists() or link_path.is_symlink():
                    raise ValueError(f"symlink path already exists: {relative}")
                link_path.symlink_to(target)
        except (OSError, ValueError, TypeError) as exc:
            return {"case_id": case_id, "passed": False, "failures": [str(exc)]}
        if candidate_uid is not None:
            try:
                (workspace / ".candidate-home").mkdir()
                (workspace / ".tmp").mkdir()
                # The temp root remains runner-owned for reliable cleanup. Its
                # contents are candidate-owned, and the sticky bit guards entries.
                _chown_workspace_entries(workspace, candidate_uid, candidate_uid)
            except OSError as exc:
                return {"case_id": case_id, "passed": False, "failures": [f"candidate workspace ownership could not be restricted: {exc}"]}

        substitutions = {
            "${workspace}": str(workspace),
            "${candidate}": str(candidate_copy),
            **http_substitutions,
        }
        binary = candidate.get("binary")
        if binary is not None:
            if not isinstance(binary, str) or not binary:
                return {"case_id": case_id, "passed": False, "failures": ["candidate binary must be a non-empty string"]}
            substitutions["${binary}"] = binary

        def expand(value: str) -> str:
            for token, replacement in substitutions.items():
                value = value.replace(token, replacement)
            return value

        environment = (
            {
                "PATH": "/opt/veritas/bin:/usr/local/bin:/usr/bin:/bin",
                "HOME": str(workspace / ".candidate-home"),
                "TMPDIR": str(workspace / ".tmp"),
                "LANG": "C.UTF-8",
            }
            if candidate_uid is not None
            else os.environ.copy()
        )
        identity = {"user": candidate_uid, "group": candidate_uid, "extra_groups": ()} if candidate_uid is not None else {}
        configured_env = candidate.get("env", {})
        case_env = case.get("env", {})
        if not isinstance(configured_env, dict) or not isinstance(case_env, dict):
            return {"case_id": case_id, "passed": False, "failures": ["candidate and case env values must be objects"]}
        for key, value in {**configured_env, **case_env}.items():
            if not isinstance(key, str) or not isinstance(value, str):
                return {"case_id": case_id, "passed": False, "failures": ["environment names and values must be strings"]}
            environment[key] = expand(value)
        for setup_index, setup_step in enumerate(setup):
            if not isinstance(setup_step, dict) or not isinstance(setup_step.get("command_args", []), list) or not all(isinstance(item, str) for item in setup_step.get("command_args", [])):
                failures.append(f"setup step {setup_index + 1} command_args must be an array of strings")
                break
            setup_argv = [expand(item) for item in command] + [expand(item) for item in setup_step.get("command_args", [])]
            try:
                setup_result = _run_candidate_process(
                    setup_argv,
                    cwd=workspace,
                    env=environment,
                    timeout_seconds=timeout_seconds,
                    identity=identity,
                )
                setup_expected = setup_step.get("expected_exit_code", 0)
                if setup_result.returncode != setup_expected:
                    failures.append(f"setup step {setup_index + 1}: expected exit code {setup_expected}, received {setup_result.returncode}")
            except (OSError, subprocess.TimeoutExpired) as exc:
                failures.append(f"setup step {setup_index + 1} failed: {exc}")
                break
        if not failures:
            try:
                for relative in delete_files:
                    path = _safe_path(workspace, relative)
                    if path.is_dir():
                        raise ValueError(f"fixture mutation cannot delete a directory: {relative}")
                    path.unlink(missing_ok=True)
                for relative, content in mutations.items():
                    path = _safe_path(workspace, relative)
                    if not isinstance(content, str):
                        raise ValueError(f"mutation content for {relative!r} must be text")
                    path.parent.mkdir(parents=True, exist_ok=True)
                    for token, replacement in substitutions.items():
                        content = content.replace(token, replacement)
                    path.write_text(content, encoding="utf-8")
                for relative, encoded in binary_mutations.items():
                    path = _safe_path(workspace, relative)
                    if not isinstance(encoded, str):
                        raise ValueError(f"binary mutation for {relative!r} must be base64 text")
                    path.parent.mkdir(parents=True, exist_ok=True)
                    path.write_bytes(base64.b64decode(encoded, validate=True))
            except (OSError, ValueError, TypeError, binascii.Error) as exc:
                failures.append(str(exc))
        for step_index, step in enumerate(steps if not failures else []):
            step_args = step.get("command_args", [])
            argv = [expand(item) for item in command] + [expand(item) for item in step_args]
            expected_exit = step.get("expected_exit_code", 0)
            step_stdout = ""
            step_stderr = ""
            step_failures: list[str] = []
            try:
                step_started = time.monotonic()
                completed = _run_candidate_process(
                    argv,
                    cwd=workspace,
                    env=environment,
                    timeout_seconds=timeout_seconds,
                    identity=identity,
                )
                exit_code = completed.returncode
                step_stdout = completed.stdout
                step_stderr = completed.stderr
            except subprocess.TimeoutExpired as exc:
                step_stdout = exc.stdout.decode(errors="replace") if isinstance(exc.stdout, bytes) else (exc.stdout or "")
                step_stderr = exc.stderr.decode(errors="replace") if isinstance(exc.stderr, bytes) else (exc.stderr or "")
                step_failures.append(f"candidate exceeded {timeout_seconds} second timeout")
            except OSError as exc:
                step_failures.append(f"candidate could not be launched: {exc}")

            if exit_code is not None and exit_code != expected_exit:
                step_failures.append(f"expected exit code {expected_exit}, received {exit_code}")
            step_check_count = 0
            for check_index, check in enumerate(step["checks"]):
                passed, message = _check(check, workspace, step_stdout, step_stderr, http_state)
                step_check_count += 1
                checks_total += 1
                if passed:
                    checks_passed += 1
                else:
                    step_failures.append(f"check {check_index + 1}: {message}")
            failures.extend(f"step {step_index + 1}: {item}" for item in step_failures)
            step_results.append({"exit_code": exit_code, "checks_passed": step_check_count - sum(1 for item in step_failures if item.startswith("check ")), "checks_total": step_check_count, "failures": step_failures})
            step_results[-1]["elapsed_seconds"] = round(time.monotonic() - step_started, 6)
            stdout = step_stdout
            stderr = step_stderr

    safe_stdout = stdout
    safe_stderr = stderr
    for secret in redactions:
        safe_stdout = safe_stdout.replace(secret, "[REDACTED]")
        safe_stderr = safe_stderr.replace(secret, "[REDACTED]")
    safe_http_fixtures = {name: {"requests": list(item["requests"]), "max_in_flight": item["max_in_flight"]} for name, item in http_state.items()}
    for fixture in safe_http_fixtures.values():
        for request in fixture["requests"]:
            for secret in redactions:
                request["path"] = request["path"].replace(secret, "[REDACTED]")
    return {
        "case_id": case_id,
        "candidate": candidate.get("metadata", {}),
        "passed": not failures and checks_total > 0 and checks_passed == checks_total,
        "exit_code": exit_code,
        "checks_passed": checks_passed,
        "checks_total": checks_total,
        "steps": step_results,
        "elapsed_seconds": round(time.monotonic() - started_at, 6),
        "http_fixtures": safe_http_fixtures,
        "stdout": safe_stdout,
        "stderr": safe_stderr,
        "failures": failures,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("manifest", type=Path)
    parser.add_argument("task_id")
    parser.add_argument("stage", choices=("initial", "follow_up"))
    parser.add_argument("candidate", type=Path, help="JSON file with a command argv array")
    parser.add_argument("candidate_root", type=Path)
    parser.add_argument("--timeout", type=float, default=60)
    parser.add_argument(
        "--source-only",
        action="store_true",
        help="run this candidate tree only; the restricted launcher uses this for separate source and delivery containers",
    )
    parser.add_argument(
        "--candidate-uid",
        type=int,
        help="run candidate processes as this unprivileged UID (Linux container runner only)",
    )
    args = parser.parse_args()

    try:
        manifest = json.loads(args.manifest.read_text(encoding="utf-8"))
        candidate = json.loads(args.candidate.read_text(encoding="utf-8"))
        task = next(item for item in manifest["tasks"] if item["id"] == args.task_id)
        cases = task[args.stage].get("cases", [])
        if not cases:
            raise ValueError(f"{args.task_id}/{args.stage} has no executable cases")
        results = [
            run_case(
                case,
                candidate,
                args.candidate_root,
                timeout_seconds=args.timeout,
                candidate_uid=args.candidate_uid,
            )
            for case in cases
        ]
        delivery_root = None
        delivery_results = []
        delivery_candidate = None
        if not args.source_only:
            try:
                delivery_root, delivery_candidate = _prepare_delivery(args.candidate_root.resolve())
                delivery_candidate["env"] = candidate.get("env", {})
                delivery_candidate["workspace_root_project"] = candidate.get("workspace_root_project", False)
                delivery_results = [
                    run_case(case, delivery_candidate, delivery_root, timeout_seconds=args.timeout, candidate_uid=args.candidate_uid)
                    for case in cases
                ]
            finally:
                if delivery_root is not None:
                    shutil.rmtree(delivery_root, ignore_errors=True)
    except (OSError, json.JSONDecodeError, KeyError, StopIteration, TypeError, ValueError) as exc:
        print(json.dumps({"passed": False, "error": str(exc)}, sort_keys=True))
        return 2
    report = {"task_id": args.task_id, "stage": args.stage, "passed": all(item["passed"] for item in results), "cases": results}
    if not args.source_only:
        report["passed"] = report["passed"] and all(item["passed"] for item in delivery_results)
        report["delivery"] = {"passed": all(item["passed"] for item in delivery_results), "artifact_sha256": delivery_candidate["metadata"]["artifact_sha256"], "artifact_bytes": delivery_candidate["metadata"]["artifact_bytes"], "source_paths_excluded": delivery_candidate["metadata"]["source_paths"], "cases": delivery_results}
    print(json.dumps(report, ensure_ascii=False, sort_keys=True))
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
