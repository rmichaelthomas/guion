"""Focused checks for the installed performance harness."""

import sys
import os
import json
import shutil
import subprocess
import tempfile
import unittest
from unittest import mock
from pathlib import Path

import runner

try:
    import session
except ModuleNotFoundError:
    session = None


class PeakMemoryTests(unittest.TestCase):
    def test_records_peak_memory_for_a_successful_candidate(self) -> None:
        observed = runner.peak_memory([sys.executable, "-c", "pass"], Path.cwd())

        self.assertIsInstance(observed, int)
        self.assertGreater(observed, 0)

    def test_ru_maxrss_conversion_records_platform_units(self) -> None:
        self.assertEqual(runner.ru_maxrss_to_bytes(4096, system="Darwin"), 4096)
        self.assertEqual(runner.ru_maxrss_to_bytes(4, system="Linux"), 4096)
        self.assertEqual(runner.MEMORY_MEASUREMENT["single_process_scope"], "direct child process")

    def test_uncached_source_command_explicitly_bypasses_guion_cache(self) -> None:
        command = ["/opt/guion", "run", "--json", "main.gu"]

        self.assertEqual(
            runner.uncached_source_command(command),
            ["/opt/guion", "run", "--json", "--no-cache", "main.gu"],
        )

    def test_phase_profile_is_separate_and_checks_program_output(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            project = Path(directory)
            binary = project / "guion"
            binary.write_text(
                "#!/bin/sh\nprintf '%s' '{\"status\":\"passed\",\"program_stdout\":\"9297\\n23246\\n\",\"phase_timings_ns\":{\"compile\":123,\"run\":456}}'\n",
                encoding="utf-8",
            )
            binary.chmod(0o755)

            profile = runner.profile_guion_run(
                [str(binary), "run", "--json", "--no-cache", "main.gu"],
                project,
                "richards-scheduler",
                ["9297", "23246"],
            )

        self.assertEqual(profile["reported_phase_timings_ns"], {"compile": 123, "run": 456})
        self.assertEqual(profile["cache_status"], None)
        self.assertGreater(profile["elapsed_seconds_including_process_startup"], 0)

    def test_parallel_worker_memory_report_is_an_untimed_upper_bound(self) -> None:
        script = Path(__file__).resolve().parents[2] / "development/famous_benchmarks/parallel_cpu.py"
        result = subprocess.run(
            [sys.executable, str(script), "processes", "1000", "--report-memory"],
            capture_output=True,
            text=True,
            check=False,
        )

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.splitlines(), [str(sum(i % 97 for i in range(1000)))] * 4)
        memory_line = result.stderr.strip().splitlines()[-1]
        self.assertTrue(memory_line.startswith("GUION_WORKER_MEMORY="))
        report = json.loads(memory_line.removeprefix("GUION_WORKER_MEMORY="))
        self.assertEqual(report["scope"], "sum of each observed worker's peak (upper bound)")
        self.assertGreater(report["sum_peak_rss_bytes_upper_bound"], 0)
        self.assertTrue(report["workers"])

    def test_runner_reads_the_worker_memory_report(self) -> None:
        script = Path(__file__).resolve().parents[2] / "development/famous_benchmarks/parallel_cpu.py"
        report = runner.parallel_worker_memory_report(
            [sys.executable, str(script), "processes", "1000"],
            Path.cwd(),
        )

        self.assertEqual(report["scope"], "sum of each observed worker's peak (upper bound)")
        self.assertEqual(report["worker_count"], len(report["workers"]))
        self.assertGreater(report["sum_peak_rss_bytes_upper_bound"], 0)

    def test_jsonl_scale_lanes_are_single_run_and_keep_cache_modes_separate(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            project = Path(directory)
            commands = runner.jsonl_scale_commands(Path("/opt/guion"), project)

        self.assertEqual(
            set(commands),
            {"guion_source", "guion_source_uncached", "guion_artifact", "python_data", "python_sqlite_native"},
        )
        self.assertIn("--no-cache", commands["guion_source_uncached"])
        self.assertNotIn("--no-cache", commands["guion_source"])

    def test_resume_report_requires_all_completed_pre_scale_workloads(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            report = Path(directory) / "partial.json"
            completed = [
                "richards-scheduler",
                "nbody-numeric",
                "mandelbrot-image",
                "parallel-cpu-20000",
                "parallel-cpu-200000",
                "jsonl-summary-data",
            ]
            report.write_text(
                json.dumps({"completed_workloads": completed, "workloads": {name: {} for name in completed}}),
                encoding="utf-8",
            )

            resumed = runner.load_resumable_suite_report(report)

            self.assertEqual(set(resumed["workloads"]), set(completed))
            report.write_text(json.dumps({"completed_workloads": completed[:-1], "workloads": {name: {} for name in completed[:-1]}}), encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "incomplete"):
                runner.load_resumable_suite_report(report)


class GuionApplicationPreparationTests(unittest.TestCase):
    def test_converts_only_the_temporary_copy_of_the_frozen_application(self) -> None:
        frozen_source = runner.APP / "main.vr"
        original_source = frozen_source.read_bytes()
        with tempfile.TemporaryDirectory() as scratch:
            project = Path(scratch) / "jsonl-summary"
            shutil.copytree(runner.APP, project)

            runner.prepare_guion_application(project)

            self.assertEqual((project / "main.gu").read_bytes(), original_source)
            self.assertIn('entry = "main.gu"', (project / "guion.toml").read_text())
            self.assertFalse((project / "main.vr").exists())
            self.assertFalse((project / "veritas.toml").exists())
            self.assertFalse((project / "veritas.lock").exists())
        self.assertEqual(frozen_source.read_bytes(), original_source)


class PerformanceSessionTests(unittest.TestCase):
    def require_session(self):
        if session is None:
            self.fail("the repository-local performance session harness is missing")

    @staticmethod
    def make_repo(root: Path) -> None:
        (root / "Cargo.toml").write_text("[workspace]\n", encoding="utf-8")
        (root / "Cargo.lock").write_text("# fixture lockfile\n", encoding="utf-8")

    def test_unpinned_python_fails_before_build_or_benchmark(self) -> None:
        self.require_session()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.make_repo(root)
            scratch = root / "target" / "perf-session"
            with mock.patch.object(session, "build_release_cli") as build, mock.patch.object(session, "run_benchmarks") as benchmark:
                with self.assertRaisesRegex(session.PreflightError, "3.14.7"):
                    session.baseline(
                        root=root,
                        cwd=root,
                        scratch=scratch,
                        python_version="3.13.0",
                        free_bytes=10_000_000_000,
                        offline_ok=True,
                    )
            build.assert_not_called()
            benchmark.assert_not_called()

    def test_preflight_rejects_wrong_root_bad_scratch_missing_dependencies_and_low_disk(self) -> None:
        self.require_session()
        with tempfile.TemporaryDirectory() as directory, tempfile.TemporaryDirectory() as elsewhere:
            root = Path(directory)
            self.make_repo(root)
            outside = Path(elsewhere)
            scratch = root / "target" / "perf-session"
            with self.subTest("wrong working root"), self.assertRaisesRegex(session.PreflightError, "repository root"):
                session.preflight(root=root, cwd=outside, scratch=scratch, python_version="3.14.7", free_bytes=10_000_000_000, offline_ok=True)
            blocker = root / "not-a-directory"
            blocker.write_text("file blocks scratch creation")
            with self.subTest("inaccessible scratch"), self.assertRaises(session.PreflightError):
                session.preflight(root=root, cwd=root, scratch=blocker / "perf-session", python_version="3.14.7", free_bytes=10_000_000_000, offline_ok=True)
            with self.subTest("missing offline dependency"), self.assertRaisesRegex(session.PreflightError, "offline"):
                session.preflight(root=root, cwd=root, scratch=scratch, python_version="3.14.7", free_bytes=10_000_000_000, offline_ok=False)
            with self.subTest("insufficient disk"), self.assertRaisesRegex(session.PreflightError, "disk"):
                session.preflight(root=root, cwd=root, scratch=scratch, python_version="3.14.7", free_bytes=1, offline_ok=True)

    def test_all_preflight_failures_happen_before_build_or_benchmark(self) -> None:
        self.require_session()
        with tempfile.TemporaryDirectory() as directory, tempfile.TemporaryDirectory() as elsewhere:
            root = Path(directory)
            self.make_repo(root)
            outside = Path(elsewhere)
            blocker = root / "scratch-blocker"
            blocker.write_text("not a directory")
            failures = [
                {"cwd": outside, "python_version": "3.14.7", "free_bytes": 10_000_000_000, "offline_ok": True},
                {"cwd": root, "python_version": "3.14.7", "free_bytes": 10_000_000_000, "offline_ok": False},
                {"cwd": root, "python_version": "3.14.7", "free_bytes": 1, "offline_ok": True},
                {"cwd": root, "scratch": blocker / "cache", "python_version": "3.14.7", "free_bytes": 10_000_000_000, "offline_ok": True},
            ]
            with mock.patch.object(session, "build_release_cli") as build, mock.patch.object(session, "run_benchmarks") as benchmark:
                for arguments in failures:
                    with self.subTest(arguments=arguments), self.assertRaises(session.PreflightError):
                        session.baseline(root=root, **arguments)
            build.assert_not_called()
            benchmark.assert_not_called()

    def test_external_scratch_is_rejected_without_creating_it(self) -> None:
        self.require_session()
        with tempfile.TemporaryDirectory() as directory, tempfile.TemporaryDirectory() as outside:
            root = Path(directory)
            self.make_repo(root)
            external = Path(outside) / "perf-session"
            with self.assertRaisesRegex(session.PreflightError, "inside the repository"):
                session.preflight(root=root, cwd=root, scratch=external, python_version="3.14.7", free_bytes=10_000_000_000, offline_ok=True)
            self.assertFalse(external.exists())

    def test_release_binary_is_reused_only_for_the_same_source_revision(self) -> None:
        self.require_session()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            scratch = root / "target" / "perf-session"
            calls = []

            def build(_root: Path, destination: Path) -> Path:
                calls.append(destination)
                binary = destination / "cargo" / "release" / "guion"
                binary.parent.mkdir(parents=True, exist_ok=True)
                binary.write_bytes(f"build-{len(calls)}".encode())
                return binary

            first, first_reused = session.ensure_cli(root, scratch, "revision-a", build)
            second, second_reused = session.ensure_cli(root, scratch, "revision-a", build)
            third, third_reused = session.ensure_cli(root, scratch, "revision-b", build)

            self.assertEqual(first, second)
            self.assertFalse(first_reused)
            self.assertTrue(second_reused)
            self.assertFalse(third_reused)
            self.assertEqual(len(calls), 2)
            self.assertEqual(third.read_bytes(), b"build-2")

    def test_source_fingerprint_in_a_target_nested_worktree_includes_source_files(self) -> None:
        self.require_session()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "repo" / "target" / "worktrees" / "checkout"
            source = root / "crates" / "cli" / "src" / "main.rs"
            source.parent.mkdir(parents=True)
            (root / "Cargo.toml").write_text("[workspace]\n", encoding="utf-8")
            source.write_text("fn main() {}\n", encoding="utf-8")
            first = session._source_fingerprint(root)
            source.write_text("fn main() { println!(\"changed\"); }\n", encoding="utf-8")
            second = session._source_fingerprint(root)
            self.assertNotEqual(first, second)

    def test_application_benchmarks_report_memory_and_source_cache_size(self) -> None:
        self.require_session()
        with tempfile.TemporaryDirectory() as directory:
            scratch = Path(directory)
            cache_bytes = b"bounded-cache"

            def sample(_name, _commands, project, _expected, **_kwargs):
                cache = project / ".guion" / "cache"
                cache.mkdir(parents=True)
                (cache / "source-run.cache").write_bytes(cache_bytes)
                return {"guion_source": {"median_seconds": 0.01}}, {"source_cache_states": {"guion_source": ["miss", "hit"]}}

            with mock.patch.object(runner, "require_ok", return_value=(0.0, "")), mock.patch.object(
                runner, "sample_candidates", side_effect=sample
            ), mock.patch.object(runner, "peak_memory", return_value=1234):
                results = session._measure_agent_examples(Path("/unused/guion"), scratch)

            self.assertEqual(results["file-data"]["peak_memory_bytes"]["guion_source"], 1234)
            self.assertEqual(results["sqlite-toml"]["peak_memory_bytes"]["python_reference"], 1234)
            self.assertEqual(results["file-data"]["source_cache_bytes"], len(cache_bytes))

    def test_report_writer_preserves_existing_report_and_allocates_another_name(self) -> None:
        self.require_session()
        with tempfile.TemporaryDirectory() as directory:
            reports = Path(directory)
            frozen = reports / "frozen-report.json"
            frozen.write_text("frozen evidence")
            with mock.patch.object(session, "FROZEN_REPORT", frozen):
                first = session.save_report(reports, {"value": 1}, run_id="frozen-report")
                second = session.save_report(reports, {"value": 2}, run_id="frozen-report")
            self.assertNotEqual(first, frozen)
            self.assertNotEqual(first, second)
            self.assertEqual(frozen.read_text(), "frozen evidence")
            self.assertEqual(session.json_load(first)["value"], 1)
            self.assertEqual(session.json_load(second)["value"], 2)

    def test_second_baseline_reuses_build_and_keeps_both_reports(self) -> None:
        self.require_session()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.make_repo(root)
            scratch = root / "target" / "perf-session"
            reports = root / "evaluation" / "completion" / "results" / "source-loop"
            builds = []

            def build(_root: Path, destination: Path) -> Path:
                builds.append(destination)
                binary = destination / "cargo" / "release" / "guion"
                binary.parent.mkdir(parents=True, exist_ok=True)
                binary.write_bytes(b"stable test executable")
                return binary

            with mock.patch.object(session, "build_release_cli", side_effect=build), mock.patch.object(
                session, "run_benchmarks", return_value={"tiny_cli_launch": {"median_seconds": 0.01}}
            ) as benchmark, mock.patch.object(session, "source_revision", return_value="same-revision"):
                first = session.baseline(root=root, cwd=root, scratch=scratch, python_version="3.14.7", free_bytes=10_000_000_000, offline_ok=True, report_dir=reports, run_id="first")
                second = session.baseline(root=root, cwd=root, scratch=scratch, python_version="3.14.7", free_bytes=10_000_000_000, offline_ok=True, report_dir=reports, run_id="second")

            self.assertEqual(len(builds), 1)
            self.assertNotEqual(benchmark.call_args_list[0].args[1], benchmark.call_args_list[1].args[1])
            self.assertNotEqual(first, second)
            self.assertFalse(session.json_load(first)["manifest"]["build_reused"])
            self.assertTrue(session.json_load(second)["manifest"]["build_reused"])

    def test_baseline_resumes_the_existing_run_workspace(self) -> None:
        self.require_session()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.make_repo(root)
            scratch = root / "target" / "perf-session"
            run_scratch = scratch / "runs" / "resume-run"
            partial = run_scratch / "existing-suite.partial.json"
            partial.parent.mkdir(parents=True)
            partial.write_text("{}", encoding="utf-8")

            def build(_root: Path, destination: Path) -> Path:
                binary = destination / "release" / "guion"
                binary.parent.mkdir(parents=True, exist_ok=True)
                binary.write_bytes(b"baseline binary")
                return binary

            with mock.patch.object(session, "build_release_cli", side_effect=build), mock.patch.object(
                session, "source_revision", return_value="revision-resume"
            ), mock.patch.object(session, "run_benchmarks", return_value={"tiny_cli_launch": {"median_seconds": 0.01}}) as benchmark:
                session.baseline(
                    root=root,
                    cwd=root,
                    scratch=scratch,
                    python_version="3.14.7",
                    free_bytes=10_000_000_000,
                    offline_ok=True,
                    report_dir=root / "evaluation" / "results",
                    run_id="resume-run",
                    resume_from=partial,
                )

            benchmark.assert_called_once()
            self.assertEqual(benchmark.call_args.args[1], run_scratch.resolve())
            self.assertEqual(benchmark.call_args.kwargs["resume_from"], partial.resolve())
            self.assertEqual(benchmark.call_args.args[0].read_bytes(), b"baseline binary")

    def test_run_benchmarks_reuses_existing_session_directories(self) -> None:
        self.require_session()
        with tempfile.TemporaryDirectory() as directory:
            scratch = Path(directory)
            (scratch / "source-loop" / "tiny-launch").mkdir(parents=True)
            envelope = {"source_cache": "bypassed", "phase_timings_ns": {"compile": 1}}
            output = json.dumps({"status": "passed", "program_stdout": "tiny\n"})
            with mock.patch.object(session, "_measure_source_lanes", return_value={}), mock.patch.object(
                session, "_measure_agent_examples", return_value={}
            ), mock.patch.object(runner, "write_manifest"), mock.patch.object(
                runner, "guion_result", return_value=(0.01, output, envelope)
            ), mock.patch.object(runner, "peak_memory", return_value=1234), mock.patch.object(
                runner, "run_suite", return_value={"workloads": {}}
            ), mock.patch.object(session, "_measure_launch", return_value={"median_seconds": 0.01}):
                report = session.run_benchmarks(Path("/unused/guion"), scratch)

        self.assertEqual(report["tiny_source_phase_profile"]["peak_memory_bytes"], 1234)

    def test_clean_removes_only_the_owned_session_tree(self) -> None:
        self.require_session()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            scratch = root / "target" / "perf-session"
            other = root / "target" / "other-cache"
            report = root / "evaluation" / "completion" / "results" / "kept.json"
            scratch.mkdir(parents=True)
            other.mkdir(parents=True)
            report.parent.mkdir(parents=True)
            (scratch / "owned.bin").write_bytes(b"owned")
            (other / "keep.bin").write_bytes(b"keep")
            report.write_text("report")

            session.clean(root, scratch)

            self.assertFalse(scratch.exists())
            self.assertEqual((other / "keep.bin").read_bytes(), b"keep")
            self.assertEqual(report.read_text(), "report")

    def test_compare_reports_returns_lane_deltas_without_overwriting_inputs(self) -> None:
        self.require_session()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            before = root / "before.json"
            after = root / "after.json"
            before.write_text(json.dumps({"tiny_cli_launch": {"median_seconds": 0.5}, "source_run_lanes": {"richards": {"cold_source_first_run": {"seconds": 1.0}, "repeat_and_artifact_lanes": {"guion_source": {"median_seconds": 0.9}}}}}), encoding="utf-8")
            after.write_text(json.dumps({"tiny_cli_launch": {"median_seconds": 0.3}, "source_run_lanes": {"richards": {"cold_source_first_run": {"seconds": 0.7}, "repeat_and_artifact_lanes": {"guion_source": {"median_seconds": 0.6}}}}}), encoding="utf-8")
            before_text, after_text = before.read_text(), after.read_text()

            result = session.compare(before, after)

            self.assertEqual(result["comparisons"]["tiny_cli_launch/median_seconds"]["delta_seconds"], -0.2)
            self.assertEqual(result["comparisons"]["source/richards/cold_source_first_run/seconds"]["ratio"], 0.7)
            self.assertEqual(result["comparisons"]["source/richards/guion_source/median_seconds"]["ratio"], 2 / 3)
            self.assertEqual(before.read_text(), before_text)
            self.assertEqual(after.read_text(), after_text)

    def test_baseline_preserves_another_live_session_lock(self) -> None:
        self.require_session()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.make_repo(root)
            scratch = root / "target" / "perf-session"
            scratch.mkdir(parents=True)
            lock = scratch / session.ACTIVE_LOCK
            lock.write_text(str(os.getpid()), encoding="utf-8")
            with mock.patch.object(session, "build_release_cli") as build, mock.patch.object(session, "run_benchmarks") as benchmark:
                with self.assertRaises(FileExistsError):
                    session.baseline(root=root, cwd=root, scratch=scratch, python_version="3.14.7", free_bytes=10_000_000_000, offline_ok=True)
            self.assertEqual(lock.read_text(), str(os.getpid()))
            build.assert_not_called()
            benchmark.assert_not_called()


class PerformanceRunnerPathTests(unittest.TestCase):
    def test_progress_report_uses_the_explicit_output_path(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "cohort.json"
            runner.save_progress({"workloads": {}}, output)
            self.assertTrue(output.with_name("cohort.partial.json").is_file())
            self.assertFalse((runner.ROOT / "evaluation/completion/results/performance-guion.partial.json").exists())

    def test_cli_passes_explicit_scratch_and_output_paths_to_the_suite(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            binary = root / "guion"
            binary.write_bytes(b"test binary")
            scratch = root / "session-scratch"
            output = root / "cohort.json"
            report = {"python": {"matches_pinned": True}, "workloads": {}}
            with mock.patch.object(runner, "run_suite", return_value=report) as suite, mock.patch.object(
                runner, "required_speed_wins", return_value={"all_required_wins_observed": True}
            ):
                result = runner.main(["--installed", "--binary", str(binary), "--scratch-dir", str(scratch), "--output", str(output)])
            self.assertEqual(result, 0)
            suite.assert_called_once_with(binary.resolve(), scratch.resolve(), output)
            self.assertTrue(output.is_file())


if __name__ == "__main__":
    unittest.main()
