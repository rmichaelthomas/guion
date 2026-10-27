#!/usr/bin/env python3
"""Scale calendar parsing and conflict detection with non-overlapping events."""

from __future__ import annotations

import argparse
import copy
import hashlib
import json
import os
import platform
import random
import statistics
import subprocess
import sys
import tempfile
import tomllib
from datetime import datetime, timedelta, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT))
sys.path.insert(0, str(ROOT / "evaluation/completion"))
from compare_real_tasks import APPS, MANIFEST, PYTHON_REFERENCE  # noqa: E402
from evaluation.protocol.case_runner import run_case  # noqa: E402


def interval(values: list[float], rng: random.Random) -> list[float]:
    medians = sorted(statistics.median(rng.choices(values, k=len(values))) for _ in range(5000))
    return [medians[124], medians[4874]]


def calendar_body(name: str, base: datetime, count: int, spacing_seconds: int, duration_seconds: int) -> str:
    lines = ["BEGIN:VCALENDAR", "VERSION:2.0"]
    for index in range(count):
        start = base + timedelta(seconds=index * spacing_seconds)
        end = start + timedelta(seconds=duration_seconds)
        lines.extend(
            [
                "BEGIN:VEVENT",
                f"UID:{name}-{index:05d}",
                f"DTSTART:{start:%Y%m%dT%H%M%SZ}",
                f"DTEND:{end:%Y%m%dT%H%M%SZ}",
                "STATUS:CONFIRMED",
                "END:VEVENT",
            ]
        )
    lines.append("END:VCALENDAR")
    return "\n".join(lines) + "\n"


def make_case(event_count: int, layout: str) -> dict:
    manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))
    task = next(item for item in manifest["tasks"] if item["id"] == "04-calendar-conflict-report")
    case = copy.deepcopy(task["initial"]["cases"][0])
    case["id"] = f"calendar-scale-{layout}-{event_count}-per-source"
    first_start = datetime(2025, 1, 1, 0, tzinfo=timezone.utc)
    window_end = first_start + timedelta(days=3)
    window_end_index = case["command_args"].index("--window-end") + 1
    case["command_args"][window_end_index] = window_end.strftime("%Y-%m-%dT%H:%M:%SZ")
    if layout == "disjoint":
        spacing_seconds = 11
        duration_seconds = 10
        calendar_span_seconds = event_count * spacing_seconds + 1
        bases = {
            "team": first_start,
            "personal": first_start + timedelta(seconds=calendar_span_seconds),
            "other": first_start + timedelta(seconds=calendar_span_seconds * 2),
        }
    else:
        spacing_seconds = 3
        duration_seconds = 1
        bases = {
            "team": first_start,
            "personal": first_start + timedelta(seconds=1),
            "other": first_start + timedelta(seconds=2),
        }
    fixtures = case["http_fixtures"]["calendar"]
    for name in bases:
        fixture = fixtures[f"/{name}.ics"]
        fixture["body"] = calendar_body(name, bases[name], event_count, spacing_seconds, duration_seconds)
        fixture["delay_ms"] = 50
    expected = {
        "conflicts": [],
        "source_errors": [
            {"calendar": "broken", "category": "invalid_calendar"},
            {"calendar": "unavailable", "category": "http"},
        ],
    }
    for check in case["checks"]:
        if check["kind"] == "json_path_equals" and check["path"] == "/conflicts":
            check["value"] = []
        elif check["kind"] == "json_equals":
            check["value"] = expected
    return case


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--sizes", nargs="+", type=int, default=[50, 100, 250])
    parser.add_argument("--trials", type=int, default=7)
    parser.add_argument("--layout", choices=["disjoint", "interleaved"], default="disjoint")
    args = parser.parse_args()
    if args.trials < 3 or any(size < 1 for size in args.sizes):
        parser.error("trials must be at least three and sizes must be positive")
    longest_stream = (lambda size: size * 33 + 1) if args.layout == "disjoint" else (lambda size: size * 3 + 2)
    if any(longest_stream(size) > 3 * 24 * 60 * 60 for size in args.sizes):
        parser.error("three non-overlapping event streams must fit inside the three-day test window")

    binary = args.binary.resolve(strict=True)
    calendar_project = APPS / "calendar-conflicts"
    project_config_path = calendar_project / "guion.toml"
    project_config = tomllib.loads(project_config_path.read_text(encoding="utf-8"))
    instruction_limit = project_config["runtime"]["instruction_limit"]
    scratch = ROOT / "target/perf-session/real-tasks-tmp"
    scratch.mkdir(parents=True, exist_ok=True)
    os.environ["TMPDIR"] = str(scratch)
    tempfile.tempdir = str(scratch)
    seed = 20261027
    rng = random.Random(seed)
    report = {
        "schema_version": 1,
        "kind": "calendar-event-scaling-diagnostic",
        "status": "passed",
        "commit": subprocess.run(
            ["git", "rev-parse", "--short", "HEAD"], cwd=ROOT, text=True, capture_output=True, check=True
        ).stdout.strip(),
        "guion_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
        "calendar_source_sha256": hashlib.sha256((APPS / "calendar-conflicts/main.vr").read_bytes()).hexdigest(),
        "calendar_project_config_sha256": hashlib.sha256(project_config_path.read_bytes()).hexdigest(),
        "runtime_instruction_limit": instruction_limit,
        "python": sys.version,
        "platform": platform.platform(),
        "trials": args.trials,
        "layout": args.layout,
        "measurement": (
            "One unmeasured randomized warmup pair plus randomized paired trials. Fresh-process source execution, "
            f"Guion cache bypassed with the project instruction limit set to {instruction_limit}, "
            "output and concurrency checks included, fixture setup excluded, OS cache uncontrolled. "
            f"Three calendars use the {args.layout} event-time layout inside a three-day window; "
            "the other two endpoints retain fixed error cases."
        ),
        "workloads": {},
    }

    for event_count in args.sizes:
        case = make_case(event_count, args.layout)
        candidates = {
            "guion": (
                {
                    "command": [str(binary), "run", "--no-cache", "${candidate}"],
                    "workspace_root_project": True,
                    "env": {"GUION_FILE_ROOT": "${workspace}"},
                },
                calendar_project,
            ),
            "python": ({"command": [sys.executable, "${candidate}/main.py"]}, PYTHON_REFERENCE),
        }

        def observe(name: str) -> dict:
            config, candidate_root = candidates[name]
            outcome = run_case(case, config, candidate_root, timeout_seconds=90)
            return {
                "passed": outcome["passed"],
                "elapsed_seconds": sum(step["elapsed_seconds"] for step in outcome["steps"]),
                "failures": outcome["failures"],
                "stderr": outcome["stderr"][:500],
            }

        preflight = {name: observe(name) for name in candidates}
        if not all(result["passed"] for result in preflight.values()):
            report["status"] = "failed"
            report["workloads"][str(event_count)] = {
                "events_per_calendar": event_count,
                "parsed_events": event_count * 3,
                "preflight": preflight,
                "status": "failed",
            }
            args.output.parent.mkdir(parents=True, exist_ok=True)
            args.output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
            return 1

        samples = {name: [] for name in candidates}
        orders = []
        for pair in range(args.trials + 1):
            order = list(candidates)
            rng.shuffle(order)
            observed = {}
            for name in order:
                result = observe(name)
                if not result["passed"]:
                    report["status"] = "failed"
                    report["workloads"][str(event_count)] = {
                        "events_per_calendar": event_count,
                        "parsed_events": event_count * 3,
                        "outputs_verified": False,
                        "preflight": preflight,
                        "samples_seconds": samples,
                        "paired_order": orders,
                        "failure": {"candidate": name, **result},
                        "status": "failed",
                    }
                    args.output.parent.mkdir(parents=True, exist_ok=True)
                    args.output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
                    print(
                        f"{event_count} events/source: {name} failed during timed run: "
                        f"{result['failures']}; {result['stderr']}",
                        file=sys.stderr,
                    )
                    return 1
                observed[name] = result["elapsed_seconds"]
            if pair:
                orders.append(order)
                for name in candidates:
                    samples[name].append(observed[name])

        ratios = [g / p for g, p in zip(samples["guion"], samples["python"], strict=True)]
        workload = {
            "events_per_calendar": event_count,
            "parsed_events": event_count * 3,
            "outputs_verified": True,
            "preflight": preflight,
            "samples_seconds": samples,
            "paired_order": orders,
            "median_seconds": {name: statistics.median(values) for name, values in samples.items()},
            "guion_over_python_median": statistics.median(ratios),
            "bootstrap_95_interval": interval(ratios, rng),
        }
        report["workloads"][str(event_count)] = workload
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
        print(
            json.dumps(
                {
                    "events_per_calendar": event_count,
                    "median_seconds": workload["median_seconds"],
                    "ratio": workload["guion_over_python_median"],
                }
            ),
            flush=True,
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
