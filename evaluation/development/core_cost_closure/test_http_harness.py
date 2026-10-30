"""Regression tests for controlled HTTP benchmark arms."""

from __future__ import annotations

import sys
import unittest
from pathlib import Path

REPO = Path(__file__).resolve().parents[3]
HERE = REPO / "evaluation/development/core_north_star_sprint"
sys.path.insert(0, str(HERE))
import benchmark_core_workloads as bench


class HttpHarnessTests(unittest.TestCase):
    def test_jsonl_gate_measures_both_required_sizes(self) -> None:
        self.assertEqual(bench.JSONL_ROWS, (20_000, 200_000))

    def test_legacy_and_typed_fixtures_use_the_same_explicit_timeout(self) -> None:
        legacy = (HERE / "http_request_legacy.gu").read_text(encoding="utf-8")
        typed = (HERE / "http_response_json.gu").read_text(encoding="utf-8")

        self.assertIn('http_request_with_timeout("GET", "__URL__", {}, "", 1000)', legacy)
        self.assertIn('http_request_bytes("GET", "__URL__", {}, bytes_from_text(""), 1000)', typed)

    def test_http_templates_separate_pinned_compatibility_from_lean_candidate(self) -> None:
        candidate = (HERE / "http_response_json.gu").read_text(encoding="utf-8")
        baseline = (HERE / "http_response_json_legacy_types.gu").read_text(encoding="utf-8")
        hashes = bench.benchmark_input_hashes()

        self.assertIn('import "std:http"', candidate)
        self.assertNotIn('import "std:types"', candidate)
        self.assertIn('import "std:types"', baseline)
        self.assertEqual(
            hashes["http_typed_baseline_template"],
            bench.digest(HERE / "http_response_json_legacy_types.gu"),
        )

    def test_same_binary_arms_keep_executable_constant(self) -> None:
        commands = bench.http_commands_for_binary(
            Path("/release/guion"),
            Path("legacy.gu"),
            Path("typed.gu"),
            "candidate",
            Path("/stdlib/candidate"),
        )

        self.assertEqual(commands["guion_legacy_candidate"][:3], commands["guion_typed_candidate"][:3])
        self.assertEqual(commands["guion_legacy_candidate"][2], "/release/guion")
        self.assertEqual(commands["guion_legacy_candidate"][1], "GUION_STDLIB_ROOT=/stdlib/candidate")
        self.assertIn("--no-cache", commands["guion_legacy_candidate"])
        self.assertIn("--no-cache", commands["guion_typed_candidate"])
        self.assertEqual(commands["guion_legacy_candidate"][-1], "legacy.gu")
        self.assertEqual(commands["guion_typed_candidate"][-1], "typed.gu")

    def test_http_artifact_arms_skip_source_compilation_and_keep_host_grant(self) -> None:
        commands = bench.http_artifact_commands_for_binary(
            Path("/release/guion"),
            Path("legacy.gub"),
            Path("typed.gub"),
            "candidate",
            Path("/stdlib/candidate"),
        )

        self.assertEqual(
            commands["guion_legacy_candidate_artifact"][:3],
            commands["guion_typed_candidate_artifact"][:3],
        )
        self.assertEqual(
            commands["guion_legacy_candidate_artifact"][1], "GUION_STDLIB_ROOT=/stdlib/candidate"
        )
        self.assertNotIn("--no-cache", commands["guion_legacy_candidate_artifact"])
        self.assertNotIn("--no-cache", commands["guion_typed_candidate_artifact"])
        self.assertIn("--allow-http-host", commands["guion_legacy_candidate_artifact"])
        self.assertEqual(commands["guion_legacy_candidate_artifact"][-1], "legacy.gub")
        self.assertEqual(commands["guion_typed_candidate_artifact"][-1], "typed.gub")

    def test_cached_source_arms_use_the_normal_source_cache(self) -> None:
        commands = bench.http_cached_source_commands_for_binary(
            Path("/release/guion"),
            Path("legacy.gu"),
            Path("typed.gu"),
            "candidate",
            Path("/stdlib/candidate"),
        )

        self.assertEqual(
            commands["guion_legacy_candidate_cached_source"][:3],
            commands["guion_typed_candidate_cached_source"][:3],
        )
        self.assertEqual(
            commands["guion_legacy_candidate_cached_source"][1], "GUION_STDLIB_ROOT=/stdlib/candidate"
        )
        self.assertNotIn("--no-cache", commands["guion_legacy_candidate_cached_source"])
        self.assertNotIn("--no-cache", commands["guion_typed_candidate_cached_source"])
        self.assertIn("--allow-http-host", commands["guion_legacy_candidate_cached_source"])
        self.assertEqual(commands["guion_legacy_candidate_cached_source"][-1], "legacy.gu")
        self.assertEqual(commands["guion_typed_candidate_cached_source"][-1], "typed.gu")

    def test_http_programs_get_separate_project_cache_roots(self) -> None:
        import tempfile

        with tempfile.TemporaryDirectory() as directory:
            parent = Path(directory)
            legacy = bench.write_http_source_project(parent, "legacy", "fn main() -> Int:\n    return 0\n")
            typed = bench.write_http_source_project(parent, "typed", "fn main() -> Int:\n    return 0\n")

            self.assertNotEqual(legacy.parent, typed.parent)
            self.assertTrue((legacy.parent / "guion.toml").is_file())
            self.assertTrue((typed.parent / "guion.toml").is_file())

    def test_cached_http_sources_are_isolated_per_executable_cohort(self) -> None:
        import tempfile

        with tempfile.TemporaryDirectory() as directory:
            parent = Path(directory)
            baseline = bench.write_http_cached_source_projects(
                parent, "baseline", "legacy", "typed"
            )
            candidate = bench.write_http_cached_source_projects(
                parent, "candidate", "legacy", "typed"
            )

            self.assertNotEqual(baseline[0].parent, candidate[0].parent)
            self.assertNotEqual(baseline[1].parent, candidate[1].parent)
            self.assertEqual(baseline[0].read_text(encoding="utf-8"), "legacy")
            self.assertEqual(candidate[1].read_text(encoding="utf-8"), "typed")

    def test_baseline_http_stdlib_snapshot_pins_the_before_module(self) -> None:
        import tempfile

        with tempfile.TemporaryDirectory() as directory:
            root = bench.prepare_http_baseline_stdlib(Path(directory))
            self.assertEqual(
                (root / "http.gu").read_text(encoding="utf-8"),
                bench.BASELINE_HTTP_STDLIB.read_text(encoding="utf-8"),
            )
            self.assertNotEqual(
                (root / "http.gu").read_text(encoding="utf-8"),
                (bench.ROOT / "language/stdlib/http.gu").read_text(encoding="utf-8"),
            )

    def test_baseline_http_stdlib_snapshot_uses_baseline_compatible_modules(self) -> None:
        import tempfile

        with tempfile.TemporaryDirectory() as directory:
            parent = Path(directory)
            root = bench.prepare_http_baseline_stdlib(parent)
            expected = bench.prepare_frozen_stdlib(
                parent / "expected", bench.BASELINE_SOURCE_REVISION
            )
            self.assertEqual(
                (root / "time.gu").read_text(encoding="utf-8"),
                (expected / "time.gu").read_text(encoding="utf-8"),
            )

    def test_candidate_http_stdlib_snapshot_pins_the_current_module(self) -> None:
        import tempfile

        with tempfile.TemporaryDirectory() as directory:
            root = bench.prepare_http_candidate_stdlib(Path(directory))
            self.assertEqual(
                (root / "http.gu").read_text(encoding="utf-8"),
                (bench.ROOT / "language/stdlib/http.gu").read_text(encoding="utf-8"),
            )

    def test_jsonl_baseline_stdlib_snapshot_is_pinned_to_the_baseline_revision(self) -> None:
        import tempfile

        with tempfile.TemporaryDirectory() as directory:
            root = bench.prepare_frozen_stdlib(
                Path(directory), "41d3b48551ea735135ee8dc759bff513213a4d72"
            )
            time_module = (root / "time.gu").read_text(encoding="utf-8")
            self.assertTrue((root / "http.gu").is_file())

        self.assertNotIn("format_utc_date", time_module)

    def test_payload_matrix_covers_empty_small_target_and_large_bodies(self) -> None:
        self.assertEqual(bench.HTTP_PAYLOAD_SIZES, (0, 1024, 65536, 1048576))
        for size in bench.HTTP_PAYLOAD_SIZES:
            body = bench.json.loads(bench.make_payload(size))
            self.assertEqual(len(body["payload"]), size)

    def test_corrupt_program_output_fails_before_it_can_be_timed_as_valid(self) -> None:
        import tempfile

        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaisesRegex(RuntimeError, "output mismatch"):
                bench.runner.sample_candidates(
                    "sprint-tiny-source",
                    {"python_reference": [sys.executable, "-c", "print('corrupt')"]},
                    Path(directory),
                    ["ready"],
                    trials=1,
                )


if __name__ == "__main__":
    unittest.main()
