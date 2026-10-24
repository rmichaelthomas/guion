"""Keep the candidate independent evaluation kit complete and visibly unapproved."""

import json
import hashlib
from datetime import datetime
from pathlib import Path
import tempfile
import unittest

from evaluation.protocol.case_runner import run_case
from evaluation.protocol.recommendations import EXPECTED_CLASSES


ROOT = Path(__file__).resolve().parents[2]
KIT = ROOT / "evaluation/fixtures/independent-v1"


class IndependentKitTests(unittest.TestCase):
    def test_manifest_covers_all_nine_briefs_and_both_stages(self):
        manifest = json.loads((KIT / "manifest.json").read_text(encoding="utf-8"))
        self.assertEqual(manifest["schema_version"], 1)
        self.assertEqual(manifest["status"], "frozen")
        self.assertEqual(manifest["task_count"], 9)
        self.assertEqual(manifest["case_count"], 127)
        self.assertEqual(manifest["stages"], ["initial", "follow_up"])
        self.assertEqual(len(manifest["tasks"]), 9)
        self.assertEqual(len({task["id"] for task in manifest["tasks"]}), 9)

        expected_classes = {
            "small_automation_cli": 3,
            "concurrent_network_integration": 3,
            "maintainable_application": 3,
        }
        actual_classes = {}
        executable_cases = 0
        for task in manifest["tasks"]:
            actual_classes[task["class"]] = actual_classes.get(task["class"], 0) + 1
            brief = ROOT / task["brief"]
            self.assertTrue(brief.is_file(), f"missing brief: {brief}")
            self.assertTrue(task["initial"]["assertions"])
            self.assertTrue(task["follow_up"]["assertions"])
            has_initial_fixture = any(
                task["initial"].get(key)
                for key in ("files", "events", "services", "commands", "server_script", "lock_entries")
            )
            self.assertTrue(has_initial_fixture, f"no initial fixture for {task['id']}")
            for stage in ("initial", "follow_up"):
                cases = task[stage].get("cases", [])
                self.assertTrue(cases, f"no executable cases for {task['id']}/{stage}")
                for case in cases:
                    self.assertTrue(case.get("id"), f"case without id in {task['id']}/{stage}")
                    if "steps" in case:
                        self.assertIsInstance(case["steps"], list)
                        self.assertTrue(case["steps"], f"case without steps: {case.get('id')}")
                        for step in case["steps"]:
                            self.assertIsInstance(step.get("command_args"), list)
                            self.assertTrue(step.get("checks"), f"step without checks: {case.get('id')}")
                    else:
                        self.assertIsInstance(case.get("command_args"), list)
                        self.assertIsInstance(case.get("checks"), list)
                        self.assertTrue(case["checks"], f"case without executable checks: {case.get('id')}")
                    executable_cases += 1
        self.assertEqual(actual_classes, expected_classes)
        self.assertEqual(set(actual_classes), EXPECTED_CLASSES)
        self.assertGreaterEqual(executable_cases, 18)

        case_ids = {
            case["id"]
            for task in manifest["tasks"]
            for stage in ("initial", "follow_up")
            for case in task[stage]["cases"]
        }
        self.assertEqual(executable_cases, sum(len(task[stage]["cases"]) for task in manifest["tasks"] for stage in ("initial", "follow_up")))
        self.assertEqual(executable_cases, 127)
        self.assertEqual(manifest["case_count"], executable_cases)
        self.assertTrue({
            "doc-index-indexes-nested-folders",
            "doc-index-ignores-symlinked-directory",
            "doc-index-normalized-path-tie-break",
            "health-maintenance-invalid-rfc3339-preserves-output",
            "calendar-rejects-space-separator-preserves-output",
            "doc-index-duplicate-alias-field-preserves-index",
            "doc-index-rebuild-removes-stale-backlinks",
            "lockfile-auditor-missing-as-of-preserves-output",
            "lockfile-auditor-invalid-as-of-preserves-output",
            "sla-policy-boolean-preserves-output",
            "sla-policy-negative-preserves-output",
            "sla-policy-unknown-key-preserves-output",
            "release-notes-escapes-all-ascii-punctuation",
            "release-notes-reject-category-cr-preserves-both-outputs",
            "release-notes-reject-category-lf-preserves-both-outputs",
            "release-notes-reject-text-cr-preserves-both-outputs",
            "release-notes-reject-text-lf-preserves-both-outputs",
            "config-migrator-check-schema1-valid",
            "config-migrator-check-schema2-valid",
            "config-migrator-check-schema2-missing-timeout-safe",
            "config-migrator-migrate-schema2-missing-timeout-safe",
            "config-migrator-migrate-schema1-preserves-inline-version-comment",
            "config-migrator-check-schema2-missing-parallelism-read-only",
            "config-migrator-invalid-schema2-boolean-parallelism-check-and-migrate-safe",
            "lockfile-auditor-repeated-diagnostics-per-entry",
            "lockfile-auditor-duplicate-exception-preserves-output",
            "lockfile-auditor-exception-expires-on-as-of",
        }.issubset(case_ids))

    def test_task06_brief_states_the_output_preservation_oracle(self):
        manifest = json.loads((KIT / "manifest.json").read_text(encoding="utf-8"))
        task = next(item for item in manifest["tasks"] if item["id"] == "06-service-health-digest")
        case = next(item for item in task["follow_up"]["cases"] if item["id"] == "health-maintenance-invalid-rfc3339-preserves-output")
        brief = (ROOT / task["brief"]).read_text(encoding="utf-8")
        self.assertIn("Invalid maintenance timestamps must fail without replacing either existing output file.", brief)
        checks = {check["file"]: check["value"] for check in case["checks"] if check["kind"] == "file_equals"}
        self.assertEqual(checks["report.json"], "previous report\n")
        self.assertEqual(checks["digest.txt"], "previous digest\n")

    def test_veritas_bridge_uses_project_managed_python_without_global_overrides(self):
        candidate = json.loads((ROOT / "evaluation/sandbox/candidate-veritas.json").read_text(encoding="utf-8"))
        self.assertNotIn("VERITAS_PYTHON", candidate.get("env", {}))
        self.assertNotIn("PYTHONPATH", candidate.get("env", {}))
        self.assertEqual(candidate["workspace_root_project"], True)

    def test_veritas_campaign_image_removes_unmatched_development_material(self):
        dockerfile = (ROOT / "evaluation/sandbox/Dockerfile").read_text(encoding="utf-8")
        build_script = (ROOT / "evaluation/sandbox/build-images.sh").read_text(encoding="utf-8")
        for private_path in ("/opt/veritas/evaluation", "/opt/veritas/examples", "/opt/veritas/language", "/opt/veritas/docs"):
            self.assertIn(private_path, dockerfile)
            self.assertIn(f"test ! -e {private_path}", build_script)

    def test_v16_review_findings_have_executable_coverage_and_package_alignment(self):
        manifest = json.loads((KIT / "manifest.json").read_text(encoding="utf-8"))
        cases = {
            case["id"]: case
            for task in manifest["tasks"]
            for stage in ("initial", "follow_up")
            for case in task[stage]["cases"]
        }
        self.assertEqual(manifest["case_count"], 127)

        priority = cases["sla-unknown-priority-preserves-output"]
        self.assertIn("unknown_priority", [check.get("value") for check in priority["checks"]])
        timestamp = cases["sla-invalid-timestamp-preserves-output"]
        self.assertIn("2025-01-02 00:00:00Z", timestamp["files"]["tickets.csv"])
        self.assertTrue(any("timestamp" in check.get("value", "") for check in timestamp["checks"]))

        release_brief = (ROOT / "evaluation/briefs/independent/03-release-notes.md").read_text(encoding="utf-8")
        self.assertNotIn("record versions", release_brief)
        shipped_docs_test = (ROOT / "evaluation/tests/test_shipped_docs_repair.py").read_text(encoding="utf-8")
        self.assertIn('"127 cases total"', shipped_docs_test)

        calendar = cases["calendar-rejects-space-separator-preserves-output"]
        self.assertIn("2025-01-01 00:00:00Z", calendar["command_args"])
        self.assertTrue(any("previous report" in check.get("value", "") for check in calendar["checks"]))
        health = cases["health-maintenance-invalid-rfc3339-preserves-output"]
        self.assertIn("2025-01-02 00:00:00Z", health["files"]["services.json"])
        self.assertTrue(any(check.get("kind") == "file_equals" for check in health["checks"]))

        tie = cases["doc-index-normalized-path-tie-break"]
        self.assertTrue(any(
            "backlinks" in step["command_args"]
            and any(check.get("kind") == "json_equals" and check.get("value") == {"backlinks": ["Guide A.md", "guide   a.md"]} for check in step["checks"])
            for step in tie["steps"]
        ))

        hashes = json.loads((ROOT / "evaluation/review/independent-v1/artifact-hashes.json").read_text(encoding="utf-8"))
        aliases = {row["path"]: row.get("source", row["path"]) for row in hashes["artifacts"]}
        self.assertEqual(aliases.get("docs/evaluation-protocol.md"), "evaluation/protocol/README.md")
        self.assertEqual(aliases.get("docs/case-runner.md"), "evaluation/protocol/case-runner.md")
        self.assertTrue(any(row["path"].endswith("review-result-v16.md") for row in hashes["artifacts"]))
        self.assertTrue(any(row["path"].endswith("review-result-v17.md") for row in hashes["artifacts"]))
        self.assertTrue(any(row["path"].endswith("review-result-v18.md") for row in hashes["artifacts"]))
        self.assertTrue(any(row["path"].endswith("review-result-v19.md") for row in hashes["artifacts"]))

    def test_brief_specific_argv_has_no_unsupported_cross_task_options(self):
        manifest = json.loads((KIT / "manifest.json").read_text(encoding="utf-8"))
        by_id = {
            task["id"]: task
            for task in manifest["tasks"]
        }
        for stage in ("initial", "follow_up"):
            for case in by_id["02-jsonl-retention"][stage]["cases"]:
                for step in case.get("steps", []):
                    self.assertNotIn("--policy", step["command_args"])
                self.assertNotIn("--policy", case.get("command_args", []))
        for case in by_id["09-lockfile-auditor"]["initial"]["cases"]:
            self.assertNotIn("--as-of", case.get("command_args", []))

    def test_python_candidate_uses_its_locked_offline_project(self):
        candidate = json.loads((ROOT / "evaluation/sandbox/candidate-python.json").read_text(encoding="utf-8"))
        self.assertEqual(candidate["command"][:3], ["uv", "run", "--locked"])
        self.assertIn("--offline", candidate["command"])
        self.assertNotIn("PYTHONPATH", candidate.get("env", {}))

    def test_all_cases_are_accepted_by_the_candidate_runner(self):
        manifest = json.loads((KIT / "manifest.json").read_text(encoding="utf-8"))
        with tempfile.TemporaryDirectory(prefix="veritas-empty-candidate-") as temporary:
            candidate_root = Path(temporary)
            seen = 0
            for task in manifest["tasks"]:
                for stage in ("initial", "follow_up"):
                    for case in task[stage]["cases"]:
                        # A no-op candidate intentionally fails semantic checks. This verifies that every
                        # fixture is structurally executable and returns a result instead of crashing.
                        result = run_case(
                            case,
                            {"command": ["python3", "-c", "pass"], "metadata": {"probe": "schema-only"}},
                            candidate_root,
                            timeout_seconds=2,
                        )
                        self.assertEqual(result["case_id"], case["id"])
                        self.assertGreater(result["checks_total"], 0)
                        self.assertIn("passed", result)
                        seen += 1
            self.assertEqual(seen, sum(len(task[stage]["cases"]) for task in manifest["tasks"] for stage in ("initial", "follow_up")))

    def test_concurrency_briefs_require_observed_overlap(self):
        manifest = json.loads((KIT / "manifest.json").read_text(encoding="utf-8"))
        required_cases = {
            "calendar-overlap-and-boundary": 2,
            "calendar-tentative-follow-up": 2,
            "health-concurrency-bound": 2,
        }
        by_id = {
            case["id"]: case
            for task in manifest["tasks"]
            for stage in ("initial", "follow_up")
            for case in task[stage]["cases"]
        }
        self.assertTrue(set(required_cases).issubset(by_id))
        for case_id, expected_overlap in required_cases.items():
            check_values = {
                check.get("kind"): check.get("value")
                for check in by_id[case_id].get("checks", [])
            }
            self.assertGreaterEqual(
                check_values.get("http_max_in_flight_at_least", 0),
                expected_overlap,
                case_id,
            )

    def test_calendar_oracle_contains_every_cross_calendar_overlap(self):
        manifest = json.loads((KIT / "manifest.json").read_text(encoding="utf-8"))
        task = next(task for task in manifest["tasks"] if task["id"] == "04-calendar-conflict-report")
        case = next(case for case in task["initial"]["cases"] if case["id"] == "calendar-overlap-and-boundary")
        source_map = {
            source["url"].rsplit("/", 1)[-1]: source["name"]
            for source in json.loads(case["files"]["sources.json"])["calendars"]
        }
        events = []
        for route, response in case["http_fixtures"]["calendar"].items():
            calendar = source_map[route.rsplit("/", 1)[-1]]
            for component in response["body"].split("BEGIN:VEVENT\n")[1:]:
                fields = dict(line.split(":", 1) for line in component.splitlines() if ":" in line)
                if fields.get("STATUS") != "CONFIRMED":
                    continue
                events.append({
                    "id": fields["UID"],
                    "calendar": calendar,
                    "start": datetime.strptime(fields["DTSTART"], "%Y%m%dT%H%M%SZ"),
                    "end": datetime.strptime(fields["DTEND"], "%Y%m%dT%H%M%SZ"),
                })
        expected = []
        for index, left in enumerate(events):
            for right in events[index + 1:]:
                if left["calendar"] == right["calendar"]:
                    continue
                start = max(left["start"], right["start"])
                end = min(left["end"], right["end"])
                if start >= end:
                    continue
                expected.append({
                    "start": start.strftime("%Y-%m-%dT%H:%M:%SZ"),
                    "end": end.strftime("%Y-%m-%dT%H:%M:%SZ"),
                    "event_ids": sorted((left["id"], right["id"])),
                    "calendars": sorted((left["calendar"], right["calendar"])),
                })
        expected.sort(key=lambda item: (item["start"], item["end"], item["event_ids"]))
        oracle = next(check["value"] for check in case["checks"] if check.get("path") == "/conflicts")
        self.assertEqual(oracle, expected)

    def test_status_does_not_claim_independent_review_or_frozen_evidence(self):
        manifest = json.loads((KIT / "manifest.json").read_text(encoding="utf-8"))
        self.assertIn("Frozen after independent review v17", manifest["status_note"])
        hashes_path = ROOT / "evaluation/review/independent-v1/artifact-hashes.json"
        hashes = json.loads(hashes_path.read_text(encoding="utf-8"))
        self.assertEqual(hashes["status"], "frozen")
        hashed_sources = {artifact["source"] for artifact in hashes["artifacts"]}
        python_calibration_inputs = {
            "evaluation/calibration/python-reference/main.py",
            "evaluation/calibration/python-reference/requirements.txt",
            "evaluation/calibration/results/python-reference-calibration.json",
        }
        self.assertTrue(python_calibration_inputs.issubset(hashed_sources))
        sandbox_inputs = {
            "evaluation/sandbox/Dockerfile",
            "evaluation/sandbox/build-images.sh",
            "evaluation/sandbox/run-case.sh",
            "evaluation/sandbox/dependencies/requirements.txt",
            "evaluation/sandbox/dependencies/campaign-dependencies.tar.gz",
            "evaluation/tests/test_case_runner.py",
            "evaluation/tests/test_shipped_docs_repair.py",
            "distribution/package-current-host.sh",
        }
        campaign_inputs = {
            "evaluation/protocol/campaign-config.json",
            "evaluation/protocol/render_choice_prompt.py",
            "evaluation/tests/test_choice_prompt.py",
            "evaluation/tests/test_recommendations.py",
            "evaluation/review/independent-v1/review-result-v18.md",
            "evaluation/review/independent-v1/review-result-v19.md",
            "evaluation/review/independent-v1/review-result-final-pass.md",
            "evaluation/protocol/usage_evidence.py",
            "evaluation/tests/test_usage_evidence.py",
            "evaluation/review/independent-v1/product-information/python-baseline.md",
        }
        self.assertTrue(sandbox_inputs.issubset(hashed_sources))
        self.assertTrue(campaign_inputs.issubset(hashed_sources))
        reviewer_instructions = (ROOT / "evaluation/review/independent-v1/reviewer-instructions.md").read_text(encoding="utf-8")
        for relative_path in python_calibration_inputs:
            self.assertIn(relative_path, reviewer_instructions)
        for relative_path in sandbox_inputs:
            self.assertIn(relative_path, reviewer_instructions)
        for relative_path in campaign_inputs:
            self.assertIn(relative_path, reviewer_instructions)
        self.assertIn("preloaded", reviewer_instructions)
        self.assertIn("delivery.json", reviewer_instructions)
        self.assertIn("review-result-v13.md", reviewer_instructions)
        self.assertIn("review-result-v14.md", reviewer_instructions)
        self.assertIn("review-result-v15.md", reviewer_instructions)
        self.assertIn("review-result-v16.md", reviewer_instructions)
        self.assertIn("review-result-v17.md", reviewer_instructions)
        self.assertIn("review-result-v19.md", reviewer_instructions)
        self.assertIn("review-result-final-pass.md", reviewer_instructions)
        self.assertIn("review-result-v18.md", reviewer_instructions)
        for artifact in hashes["artifacts"]:
            path = ROOT / artifact["path"]
            if not path.is_file():
                path = ROOT / artifact["source"]
            self.assertTrue(path.is_file(), f"missing reviewer packet artifact: {path}")
            self.assertEqual(hashlib.sha256(path.read_bytes()).hexdigest(), artifact["sha256"], artifact["path"])
        bundle_path = ROOT / "evaluation/review/independent-v1/product-information/bundle-manifest.json"
        bundle = json.loads(bundle_path.read_text(encoding="utf-8"))
        canonical = json.dumps(bundle["artifacts"], sort_keys=True, separators=(",", ":")).encode("utf-8")
        self.assertEqual(hashlib.sha256(canonical).hexdigest(), bundle["bundle_sha256"])
        for artifact in bundle["artifacts"]:
            content = (bundle_path.parent / artifact["path"]).read_bytes()
            self.assertEqual(hashlib.sha256(content).hexdigest(), artifact["sha256"])
        protocol_path = ROOT / "evaluation/protocol/README.md"
        if not protocol_path.is_file():
            protocol_path = ROOT / "docs/evaluation-protocol.md"
        protocol = protocol_path.read_text(encoding="utf-8")
        self.assertIn("second evaluator must review", protocol)
        self.assertIn("exactly two independent matched implementation pairs", protocol)
        self.assertIn("campaign-pinned CPython 3.14.8", protocol)
        self.assertNotIn("current stable Python version available on the host", protocol)
        self.assertIn("at least five of six initial-plus-follow-up primary successes", protocol)
        self.assertIn("do not launch further Veritas runs in that class", protocol)
        calibration = json.loads((ROOT / "evaluation/calibration/results/python-reference-calibration.json").read_text(encoding="utf-8"))
        self.assertEqual(calibration["summary"], {"passed": 127, "total": 127})
        self.assertEqual(calibration["manifest_sha256"], hashlib.sha256((KIT / "manifest.json").read_bytes()).hexdigest())

    def test_last_independent_review_contracts_are_explicit_and_executable(self):
        manifest = json.loads((KIT / "manifest.json").read_text(encoding="utf-8"))
        briefs = {task["id"]: (ROOT / task["brief"]).read_text(encoding="utf-8") for task in manifest["tasks"]}
        cases = {
            case["id"]: case
            for task in manifest["tasks"]
            for stage in ("initial", "follow_up")
            for case in task[stage]["cases"]
        }
        self.assertIn("ascending ASCII bytewise order", briefs["03-release-notes"])
        self.assertIn("U+007B–U+007E", briefs["03-release-notes"])
        self.assertIn("invalid --since version", next(check["value"] for check in cases["release-notes-invalid-since-preserves-outputs"]["checks"] if check["kind"] == "stderr_contains"))
        self.assertIn("SHA-256 hexadecimal digest", briefs["05-webhook-replay"])
        self.assertIn("declared schema version", briefs["07-config-migrator"])
        release_output = next(check["value"] for check in cases["release-notes-initial"]["checks"] if check.get("file") == "notes.md")
        self.assertLess(release_output.index("## 1.2.0\n"), release_output.index("## 1.2.0+build.1\n"))
        self.assertIn("exactly two fields", briefs["09-lockfile-auditor"])
        self.assertIn("YYYY-MM-DD", briefs["09-lockfile-auditor"])
        self.assertIn("to `warnings`", briefs["09-lockfile-auditor"])
        self.assertIn("config-migrator-schema1-sync-not-table-safe", cases)
        self.assertIn("sync = true", cases["config-migrator-schema1-sync-not-table-safe"]["files"]["config.toml"])
        self.assertIn("doc-index-alias-json-grammar", cases)
        self.assertIn("original relative-path UTF-8 bytes as the tie-breaker", briefs["08-documentation-link-index"])
        self.assertNotIn("database relocation", briefs["08-documentation-link-index"])
        self.assertTrue(any(check.get("kind") == "json_equals" for check in cases["webhook-dead-letter-limit"]["steps"][1]["checks"]))
        self.assertTrue(any(check.get("kind") == "json_equals" and check.get("file") == "accepted.json" for check in cases["webhook-late-max-attempts-does-not-retry"]["steps"][1]["checks"]))
        self.assertTrue(any(check.get("kind") == "json_equals" and check.get("file") == "report.json" for step in cases["doc-index-alias-collision-is-ambiguous"]["steps"] for check in step["checks"]))
        self.assertIn(r'say \"hello\"', cases["doc-index-alias-json-grammar"]["files"]["docs/Target.md"])
        self.assertIn("lockfile-auditor-invalid-exception-preserves-output", cases)
        self.assertIn("invalid exception", cases["lockfile-auditor-invalid-exception-preserves-output"]["checks"][-1]["value"])
        privacy_checks = cases["webhook-durable-retry-and-privacy"]["steps"][0]["checks"]
        self.assertEqual(sum(check["kind"] == "file_bytes_not_contains" for check in privacy_checks), 2)
        audit = (KIT / "coverage-audit.md").read_text(encoding="utf-8")
        self.assertIn("all 127 cases", audit)
        self.assertNotIn("all 51 revised cases", audit)

    def test_relocated_container_does_not_mount_the_original_candidate(self):
        launcher = (ROOT / "evaluation/sandbox/run-case.sh").read_text(encoding="utf-8")
        self.assertIn("stage_delivery.py", launcher)
        self.assertIn("delivery_root", launcher)
        self.assertIn("dst=/candidate,readonly", launcher)
        self.assertIn("Source-free delivery", launcher)
        self.assertIn("pair_id", launcher)
        self.assertIn("manifest_sha256", launcher)
        self.assertIn("schedule_sha256", launcher)
        self.assertIn("delivery_manifest_sha256", launcher)
        self.assertNotIn('"$candidate_project,dst=/candidate,readonly"', launcher.split("delivery_root", 1)[-1])

    def test_campaign_docs_and_package_boundary_are_explicit(self):
        protocol = (ROOT / "evaluation/protocol/README.md").read_text(encoding="utf-8")
        self.assertIn("18 choice sessions and 36 implementation sessions (54 sessions total)", protocol)
        self.assertNotIn("All 36 choices and implementation sessions", protocol)
        self.assertIn("not a standalone campaign kit", protocol)
        self.assertIn("usage_snapshot_sha256", protocol)
        self.assertIn("delivery tree hash", protocol)
        packager = (ROOT / "distribution/package-current-host.sh").read_text(encoding="utf-8")
        self.assertIn("evaluation/protocol/usage_evidence.py", packager)


if __name__ == "__main__":
    unittest.main()
