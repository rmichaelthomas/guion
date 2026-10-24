"""Acceptance checks for the independent candidate case runner."""

import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

from evaluation.protocol.case_runner import _check, _prepare_delivery, run_case
from evaluation.protocol.ledger import _validate_runner_evidence


class CaseRunnerTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="veritas-case-runner-")
        self.root = Path(self.temp.name)
        self.candidate = self.root / "candidate"
        self.candidate.mkdir()
        (self.candidate / "main.py").write_text(
            "import json, pathlib, sys\n"
            "payload = json.loads(pathlib.Path('fixture.json').read_text())\n"
            "pathlib.Path('report.json').write_text(json.dumps({'count': payload['count']}))\n"
            "print('completed', file=sys.stdout)\n",
            encoding="utf-8",
        )

    def tearDown(self):
        self.temp.cleanup()

    def test_binary_file_check_detects_secret_bytes_and_digest(self):
        state = self.root / "state.sqlite"
        state.write_bytes(b"header: fixture-only-secret; digest=abc123")
        check = {"kind": "file_bytes_not_contains", "file": "state.sqlite", "value": "fixture-only-secret"}
        workspace = self.root.resolve()
        self.assertEqual(_check(check, workspace, "", "", {}), (False, "file_bytes_not_contains did not match its expected value"))
        check["value"] = "missing-secret-digest"
        self.assertEqual(_check(check, workspace, "", "", {}), (True, ""))

    def test_json_equals_checks_the_entire_document_shape_and_values(self):
        report = self.root / "report.json"
        report.write_text('{"items":[{"id":"one"}],"count":1}', encoding="utf-8")
        workspace = self.root.resolve()
        exact = {"kind": "json_equals", "file": "report.json", "value": {"items": [{"id": "one"}], "count": 1}}
        self.assertEqual(_check(exact, workspace, "", "", {}), (True, ""))
        exact["value"]["extra"] = True
        self.assertEqual(_check(exact, workspace, "", "", {}), (False, "json_equals did not match its expected value"))

    def test_json_comparisons_preserve_boolean_integer_and_float_types(self):
        report = self.root / "report.json"
        workspace = self.root.resolve()
        report.write_text('{"flag":true,"integer":1,"float":1.0}', encoding="utf-8")
        for path, expected in (("/flag", 1), ("/integer", 1.0), ("/float", 1)):
            check = {"kind": "json_path_equals", "file": "report.json", "path": path, "value": expected}
            self.assertEqual(_check(check, workspace, "", "", {}), (False, "json_path_equals did not match its expected value"))
        exact = {"kind": "json_equals", "file": "report.json", "value": {"flag": True, "integer": 1, "float": 1.0}}
        self.assertEqual(_check(exact, workspace, "", "", {}), (True, ""))
        exact["value"]["flag"] = 1
        self.assertEqual(_check(exact, workspace, "", "", {}), (False, "json_equals did not match its expected value"))
        exact["value"]["flag"] = True
        exact["value"]["integer"] = 1.0
        self.assertEqual(_check(exact, workspace, "", "", {}), (False, "json_equals did not match its expected value"))

    def test_delivery_stages_locked_artifact_without_original_sources(self):
        package = self.candidate / "dist"
        package.mkdir()
        (self.candidate / "source.vr").write_text("source must stay out of delivery", encoding="utf-8")
        (package / "uv.lock").write_text("version = 1\n", encoding="utf-8")
        (package / "app.py").write_text(
            "import json, pathlib\n"
            "data=json.loads(pathlib.Path('input.json').read_text())\n"
            "pathlib.Path('output.json').write_text(json.dumps({'answer':data['answer']}))\n",
            encoding="utf-8",
        )
        (self.candidate / "delivery.json").write_text(json.dumps({
            "launch_command": ["python3", "${candidate}/dist/app.py"],
            "artifact_paths": ["dist"],
            "source_paths": ["source.vr"],
            "dependency_lock_path": "dist/uv.lock",
        }), encoding="utf-8")
        delivery_root, candidate = _prepare_delivery(self.candidate.resolve())
        try:
            self.assertFalse((delivery_root / "source.vr").exists())
            self.assertEqual(candidate["metadata"]["artifact_bytes"], sum(p.stat().st_size for p in (delivery_root / "dist").rglob("*") if p.is_file()))
            result = run_case({
                "id": "relocated-delivery",
                "files": {"input.json": '{"answer":42}'},
                "command_args": [],
                "expected_exit_code": 0,
                "checks": [{"kind": "json_path_equals", "file": "output.json", "path": "/answer", "value": 42}],
            }, candidate, delivery_root, timeout_seconds=5)
            self.assertTrue(result["passed"], result)
        finally:
            import shutil
            shutil.rmtree(delivery_root, ignore_errors=True)

    def test_documented_python_delivery_example_runs_after_relocation(self):
        (self.candidate / "main.py").write_text(
            "import json, pathlib\n"
            "request=json.loads(pathlib.Path('input.json').read_text())\n"
            "pathlib.Path('output.json').write_text(json.dumps({'answer': request['answer']}))\n",
            encoding="utf-8",
        )
        (self.candidate / "requirements.lock").write_text("", encoding="utf-8")
        source_tree = self.candidate / "src"
        source_tree.mkdir()
        (source_tree / "unused.py").write_text("raise RuntimeError('original source must not be staged')\n", encoding="utf-8")
        (self.candidate / "delivery.json").write_text(json.dumps({
            "launch_command": ["python3", "${candidate}/main.py"],
            "artifact_paths": ["main.py", "requirements.lock"],
            "source_paths": ["src/"],
            "dependency_lock_path": "requirements.lock",
        }), encoding="utf-8")
        delivery_root, candidate = _prepare_delivery(self.candidate.resolve())
        try:
            self.assertTrue((delivery_root / "main.py").is_file())
            self.assertTrue((delivery_root / "requirements.lock").is_file())
            self.assertFalse((delivery_root / "src").exists())
            result = run_case({
                "id": "documented-python-relocation",
                "files": {"input.json": '{"answer":42}'},
                "command_args": [],
                "expected_exit_code": 0,
                "checks": [{"kind": "json_path_equals", "file": "output.json", "path": "/answer", "value": 42}],
            }, candidate, delivery_root, timeout_seconds=5)
            self.assertTrue(result["passed"], result)
        finally:
            import shutil
            shutil.rmtree(delivery_root, ignore_errors=True)

    def test_delivery_rejects_source_in_artifact_paths(self):
        (self.candidate / "source.vr").write_text("source", encoding="utf-8")
        (self.candidate / "delivery.json").write_text(json.dumps({
            "launch_command": ["python3", "${candidate}/source.vr"],
            "artifact_paths": ["source.vr"],
            "source_paths": ["source.vr"],
            "dependency_lock_path": "source.vr",
        }), encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "source path appears"):
            _prepare_delivery(self.candidate.resolve())

    def test_delivery_rejects_parent_components_before_overlap_comparison(self):
        (self.candidate / "main.py").write_text("print('source')\n", encoding="utf-8")
        (self.candidate / "delivery.json").write_text(json.dumps({
            "launch_command": ["python3", "${candidate}/main.py"],
            "artifact_paths": ["main.py"],
            "source_paths": ["src/../main.py"],
            "dependency_lock_path": "main.py",
        }), encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "normalized relative path"):
            _prepare_delivery(self.candidate.resolve())

    def test_delivery_stager_can_write_a_persistent_artifact_tree_outside_source(self):
        package = self.candidate / "dist"
        package.mkdir()
        (package / "uv.lock").write_text("version = 1\n", encoding="utf-8")
        (package / "app.py").write_text("print('delivered')\n", encoding="utf-8")
        (self.candidate / "source.vr").write_text("source", encoding="utf-8")
        (self.candidate / "delivery.json").write_text(json.dumps({
            "launch_command": ["python3", "${candidate}/dist/app.py"],
            "artifact_paths": ["dist"],
            "source_paths": ["source.vr"],
            "dependency_lock_path": "dist/uv.lock",
        }), encoding="utf-8")
        staged = self.root / "delivery"
        base_config = self.root / "base.json"
        output_config = self.root / "delivery.json"
        base_config.write_text(json.dumps({"command": ["python3", "${candidate}/source.py"], "env": {"MODE": "offline"}}), encoding="utf-8")
        staged.mkdir()
        result = subprocess.run(
            [sys.executable, "evaluation/protocol/stage_delivery.py", str(self.candidate), str(base_config), str(staged), str(output_config)],
            text=True, capture_output=True, check=False,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        config = json.loads(output_config.read_text(encoding="utf-8"))
        self.assertEqual(config["command"], ["python3", "${candidate}/dist/app.py"])
        self.assertEqual(config["env"], {"MODE": "offline"})
        self.assertFalse((staged / "source.vr").exists())
        self.assertTrue((staged / "dist/uv.lock").is_file())

    def test_source_only_mode_does_not_read_or_stage_delivery_manifest(self):
        (self.candidate / "main.py").write_text("print('ok')\n", encoding="utf-8")
        (self.candidate / "delivery.json").write_text("not json", encoding="utf-8")
        manifest = self.root / "manifest.json"
        manifest.write_text(json.dumps({
            "tasks": [{"id": "task", "initial": {"cases": [
                {"id": "one", "files": {}, "checks": [{"kind": "stdout_contains", "value": "ok"}]}
            ]}}]
        }), encoding="utf-8")
        config = self.root / "candidate.json"
        config.write_text(json.dumps({"command": ["python3", "${candidate}/main.py"]}), encoding="utf-8")
        result = subprocess.run(
            [sys.executable, "evaluation/protocol/case_runner.py", str(manifest), "task", "initial", str(config), str(self.candidate), "--source-only"],
            text=True, capture_output=True, check=False,
        )
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        report = json.loads(result.stdout)
        self.assertTrue(report["passed"])
        self.assertNotIn("delivery", report)

    def test_campaign_launcher_uses_a_second_container_without_the_source_mount(self):
        import os

        (self.candidate / "main.py").write_text("print('source')\n", encoding="utf-8")
        artifact = self.candidate / "dist"
        artifact.mkdir()
        (artifact / "app.py").write_text("print('delivered')\n", encoding="utf-8")
        (artifact / "uv.lock").write_text("version = 1\n", encoding="utf-8")
        (self.candidate / "delivery.json").write_text(json.dumps({
            "launch_command": ["python3", "${candidate}/dist/app.py"],
            "artifact_paths": ["dist"],
            "source_paths": ["main.py"],
            "dependency_lock_path": "dist/uv.lock",
        }), encoding="utf-8")
        bin_dir = self.root / "bin"
        bin_dir.mkdir()
        docker = bin_dir / "docker"
        docker.write_text(
            "#!/bin/sh\n"
            "if [ \"$1\" = info ]; then printf 'fixture-engine\\n'; exit 0; fi\n"
            "if [ \"$1\" = image ]; then printf 'sha256:fixture\\n'; exit 0; fi\n"
            "if [ \"$1\" = run ]; then printf '%s\\n' \"$*\" >> \"$DOCKER_LOG\"; printf '%s\\n' '{\"passed\":true,\"cases\":[]}'; exit 0; fi\n"
            "exit 2\n",
            encoding="utf-8",
        )
        docker.chmod(0o755)
        report = self.root / "runs" / "candidate.json"
        log = self.root / "docker-calls.txt"
        env = os.environ.copy()
        env["PATH"] = f"{bin_dir}:{env['PATH']}"
        env["DOCKER_LOG"] = str(log)
        env["TMPDIR"] = str(self.root)
        result = subprocess.run(
            ["sh", "evaluation/sandbox/run-case.sh", "python", "01-support-sla-report", "initial", "pair-001", "session-001", "3.14.8 with uv", "a" * 64, "b" * 64, str(self.candidate), str(report)],
            text=True, capture_output=True, check=False, env=env,
        )
        detail = result.stdout + result.stderr
        if report.is_file():
            detail += "\n" + report.read_text(encoding="utf-8")
        self.assertEqual(result.returncode, 0, detail)
        calls = log.read_text(encoding="utf-8").splitlines()
        self.assertEqual(len(calls), 2)
        self.assertIn(f"src={self.candidate.resolve()},dst=/candidate,readonly", calls[0])
        expected_delivery = report.parent.resolve() / (report.name + ".delivery")
        self.assertIn(f"src={expected_delivery},dst=/candidate,readonly", calls[1])
        self.assertNotIn(f"src={self.candidate.resolve()},dst=/candidate,readonly", calls[1])
        final_report = json.loads(report.read_text(encoding="utf-8"))
        self.assertTrue(final_report["case_report"]["passed"])
        self.assertFalse(final_report["container"]["delivery_source_project_mounted"])
        self.assertEqual(final_report["container"]["docker_engine_id"], "fixture-engine")
        self.assertEqual(final_report["delivery_artifact_sha256"], final_report["case_report"]["delivery"]["artifact_sha256"])
        self.assertEqual(final_report["pair_id"], "pair-001")
        self.assertEqual(final_report["session_id"], "session-001")
        self.assertEqual(final_report["candidate_version"], "3.14.8 with uv")
        self.assertEqual(final_report["manifest_sha256"], "a" * 64)
        self.assertEqual(final_report["schedule_sha256"], "b" * 64)
        self.assertTrue((report.parent / final_report["delivery_manifest_path"]).is_file())
        binding_record = {
            "pair_id": "pair-001",
            "assignment": {"manifest_sha256": "a" * 64, "schedule_sha256": "b" * 64},
            "candidate": {"language": "python", "version": "3.14.8 with uv"},
            "model": {"session_id": "session-001"},
            "task": {"id": "01-support-sla-report"},
            "environment": {"host_id": "fixture-engine"},
            "outcome": {"initial": {"status": "passed"}},
        }
        errors = []
        _validate_runner_evidence(
            record=binding_record,
            context={"schedule": {"manifest_sha256": "a" * 64}},
            stage="initial",
            stage_evidence={
                "runner_report_path": report.relative_to(self.root).as_posix(),
                "runner_report_sha256": hashlib.sha256(report.read_bytes()).hexdigest(),
            },
            evidence_root=self.root,
            errors=errors,
        )
        self.assertEqual(errors, [])

    def test_checks_toml_semantics_without_requiring_serializer_whitespace(self):
        document = self.root / "config.toml"
        document.write_text("# owned\nversion = 2\n[sync]\ntimeout_seconds = 30\n", encoding="utf-8")
        workspace = self.root.resolve()
        passed, message = _check(
            {"kind": "toml_equals", "file": "config.toml", "value": {"version": 2, "sync": {"timeout_seconds": 30}}},
            workspace, "", "", {},
        )
        self.assertTrue(passed, message)
        passed, message = _check(
            {"kind": "file_contains", "file": "config.toml", "value": "# owned"},
            workspace, "", "", {},
        )
        self.assertTrue(passed, message)
        passed, _ = _check(
            {"kind": "toml_equals", "file": "config.toml", "value": {"version": 3}},
            workspace, "", "", {},
        )
        self.assertFalse(passed)

    def test_materializes_fixture_runs_command_and_checks_json_output(self):
        result = run_case(
            {"id": "count-check", "files": {"fixture.json": "{\"count\":3}"},
             "command_args": [], "expected_exit_code": 0,
             "checks": [
                 {"kind": "stdout_contains", "value": "completed"},
                 {"kind": "json_path_equals", "file": "report.json", "path": "/count", "value": 3},
             ]},
            {"command": ["python3", "${candidate}/main.py"]},
            self.candidate,
            timeout_seconds=5,
        )
        self.assertTrue(result["passed"], result)
        self.assertEqual(result["case_id"], "count-check")
        self.assertEqual(result["checks_passed"], 2)

    def test_candidate_uid_requires_a_root_posix_container_runner(self):
        with patch("evaluation.protocol.case_runner.os.geteuid", return_value=1000):
            result = run_case(
                {"id": "uid-guard", "files": {}, "checks": [{"kind": "stdout_contains", "value": "never"}]},
                {"command": ["python3", "-c", "pass"]},
                self.candidate,
                timeout_seconds=5,
                candidate_uid=10001,
            )
        self.assertFalse(result["passed"])
        self.assertEqual(
            result["failures"],
            ["candidate_uid requires a POSIX runner with root privileges"],
        )

    def test_candidate_uid_rejects_root_and_boolean_values(self):
        for uid in (0, True):
            result = run_case(
                {"id": "uid-value", "files": {}, "checks": [{"kind": "stdout_contains", "value": "never"}]},
                {"command": ["python3", "-c", "pass"]},
                self.candidate,
                timeout_seconds=5,
                candidate_uid=uid,
            )
            self.assertFalse(result["passed"])
            self.assertEqual(result["failures"], ["candidate_uid must be a positive integer"])

    def test_can_place_project_files_at_workspace_root_for_project_scoped_runtimes(self):
        result = run_case(
            {"id": "project-root", "files": {"fixture.json": "{\"count\":7}"},
             "command_args": [], "expected_exit_code": 0,
             "checks": [{"kind": "json_path_equals", "file": "report.json", "path": "/count", "value": 7}]},
            {"command": [
                "python3", "-c",
                "import pathlib,sys; root=pathlib.Path(sys.argv[1]).resolve(); assert root==pathlib.Path.cwd().resolve(); exec((root/'main.py').read_text())",
                "${candidate}",
            ], "workspace_root_project": True},
            self.candidate,
            timeout_seconds=5,
        )
        self.assertTrue(result["passed"], result)

    def test_preserves_failed_process_and_reports_each_failed_assertion(self):
        result = run_case(
            {"id": "bad-check", "files": {}, "expected_exit_code": 7,
             "checks": [{"kind": "stderr_contains", "value": "expected diagnostic"}]},
            {"command": ["python3", "-c", "raise SystemExit(2)"]},
            self.candidate,
            timeout_seconds=5,
        )
        self.assertFalse(result["passed"])
        self.assertEqual(result["exit_code"], 2)
        self.assertEqual(len(result["failures"]), 2)

    def test_rejects_fixture_path_escape(self):
        result = run_case(
            {"id": "escape", "files": {"../escape.txt": "bad"},
             "checks": [{"kind": "stdout_contains", "value": "never"}]},
            {"command": ["python3", "-c", "pass"]},
            self.candidate,
            timeout_seconds=5,
        )
        self.assertFalse(result["passed"])
        self.assertTrue(any("normalized relative path" in item for item in result["failures"]))

    def test_materializes_workspace_local_symlink_and_rejects_escape(self):
        result = run_case(
            {
                "id": "symlink-fixture",
                "files": {"outside.md": "outside root\n"},
                "symlinks": {"docs/escape.md": "../outside.md"},
                "checks": [{"kind": "stdout_contains", "value": "outside root"}],
            },
            {"command": ["python3", "-c", "from pathlib import Path; print(Path('docs/escape.md').read_text())"]},
            self.candidate,
            timeout_seconds=5,
        )
        self.assertTrue(result["passed"], result)

        escaped = run_case(
            {
                "id": "symlink-escape",
                "files": {},
                "symlinks": {"docs/escape.md": "../../../outside.md"},
                "checks": [{"kind": "stdout_contains", "value": "never"}],
            },
            {"command": ["python3", "-c", "pass"]},
            self.candidate,
            timeout_seconds=5,
        )
        self.assertFalse(escaped["passed"])
        self.assertTrue(any("symlink target escapes" in item for item in escaped["failures"]))

    def test_materializes_binary_files_and_binary_mutations(self):
        result = run_case(
            {
                "id": "binary-fixtures",
                "binary_files": {"input.bin": "/wA="},
                "binary_mutations": {"changed.bin": "//4="},
                "checks": [{"kind": "stdout_contains", "value": "ff00fffe"}],
            },
            {
                "command": [
                    "python3", "-c",
                    "from pathlib import Path; print(Path('input.bin').read_bytes().hex()+Path('changed.bin').read_bytes().hex())",
                ]
            },
            self.candidate,
            timeout_seconds=5,
        )
        self.assertTrue(result["passed"], result)

    def test_rejects_shell_strings_instead_of_argument_arrays(self):
        result = run_case(
            {"id": "shell", "files": {}, "checks": []},
            {"command": "python3 main.py"},
            self.candidate,
            timeout_seconds=5,
        )
        self.assertFalse(result["passed"])
        self.assertTrue(any("command must be a non-empty array" in item for item in result["failures"]))

    def test_checks_unredacted_output_but_redacts_secrets_in_the_report(self):
        result = run_case(
            {"id": "redaction", "files": {}, "expected_exit_code": 0,
             "checks": [{"kind": "stderr_not_contains", "value": "test-secret"}],
             "redact": ["test-secret"]},
            {"command": ["python3", "-c", "import sys; print('test-secret', file=sys.stderr)"]},
            self.candidate,
            timeout_seconds=5,
        )
        self.assertFalse(result["passed"])
        self.assertTrue(result["failures"])
        self.assertNotIn("test-secret", result["stderr"])
        self.assertNotIn("test-secret", " ".join(result["failures"]))

    def test_runs_multi_command_case_steps_in_one_workspace(self):
        (self.candidate / "state.py").write_text(
            "from pathlib import Path\nimport sys\np=Path('state.txt')\n"
            "p.write_text(p.read_text() + sys.argv[1] + '\\n' if p.exists() else sys.argv[1] + '\\n')\n"
            "print(sys.argv[1])\n",
            encoding="utf-8",
        )
        result = run_case(
            {"id": "stateful-steps", "files": {}, "steps": [
                {"command_args": ["build"], "expected_exit_code": 0,
                 "checks": [{"kind": "stdout_contains", "value": "build"}]},
                {"command_args": ["query"], "expected_exit_code": 0,
                 "checks": [{"kind": "file_equals", "file": "state.txt", "value": "build\nquery\n"}]},
            ]},
            {"command": ["python3", "${candidate}/state.py"]},
            self.candidate,
            timeout_seconds=5,
        )
        self.assertTrue(result["passed"], result)
        self.assertEqual(result["checks_passed"], 2)
        self.assertEqual(len(result["steps"]), 2)

    def test_starts_loopback_http_fixture_and_checks_request_metrics(self):
        (self.candidate / "client.py").write_text(
            "import json, pathlib, urllib.request\n"
            "cfg=json.loads(pathlib.Path('fixture.json').read_text())\n"
            "with urllib.request.urlopen(cfg['url'] + '/health') as r: body=r.read().decode()\n"
            "pathlib.Path('report.json').write_text(json.dumps({'body':body}))\n",
            encoding="utf-8",
        )
        result = run_case(
            {
                "id": "loopback-http",
                "files": {"fixture.json": '{"url":"${http:health}"}'},
                "http_fixtures": {"health": {"/health": {"status": 200, "body": "ok"}}},
                "command_args": [],
                "expected_exit_code": 0,
                "checks": [
                    {"kind": "json_path_equals", "file": "report.json", "path": "/body", "value": "ok"},
                    {"kind": "http_request_count_equals", "fixture": "health", "path": "/health", "value": 1},
                    {"kind": "http_max_in_flight_at_most", "fixture": "health", "value": 1},
                ],
            },
            {"command": ["python3", "${candidate}/client.py"]},
            self.candidate,
            timeout_seconds=5,
        )
        self.assertTrue(result["passed"], result)
        self.assertEqual(result["http_fixtures"]["health"]["requests"], [{"method": "GET", "path": "/health"}])

    def test_orders_retry_responses_and_measures_real_concurrency(self):
        (self.candidate / "client.py").write_text(
            "import concurrent.futures, json, pathlib, urllib.error, urllib.request\n"
            "base=json.loads(pathlib.Path('fixture.json').read_text())['url']\n"
            "def get(path):\n"
            "  try:\n"
            "    with urllib.request.urlopen(base+path) as response: return response.status\n"
            "  except urllib.error.HTTPError as error: return error.code\n"
            "with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:\n"
            "  list(pool.map(get, ['/slow']*4))\n"
            "retry=[get('/retry'),get('/retry')]\n"
            "pathlib.Path('report.json').write_text(json.dumps({'retry':retry}))\n",
            encoding="utf-8",
        )
        result = run_case(
            {
                "id": "retry-and-concurrency-fixture",
                "files": {"fixture.json": '{"url":"${http:downstream}"}'},
                "http_fixtures": {"downstream": {
                    "/slow": {"status": 200, "body": "ok", "delay_ms": 80},
                    "/retry": [{"status": 503, "body": "retry"}, {"status": 201, "body": "created"}],
                }},
                "command_args": [],
                "expected_exit_code": 0,
                "checks": [
                    {"kind": "json_path_equals", "file": "report.json", "path": "/retry", "value": [503, 201]},
                    {"kind": "http_request_count_equals", "fixture": "downstream", "path": "/retry", "value": 2},
                    {"kind": "http_request_count_equals", "fixture": "downstream", "value": 6},
                    {"kind": "http_max_in_flight_at_least", "fixture": "downstream", "value": 2},
                    {"kind": "http_max_in_flight_at_most", "fixture": "downstream", "value": 4},
                ],
            },
            {"command": ["python3", "${candidate}/client.py"]},
            self.candidate,
            timeout_seconds=5,
        )
        self.assertTrue(result["passed"], result)


if __name__ == "__main__":
    unittest.main()
