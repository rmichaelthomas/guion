"""Execute the frozen application regression cases against staged Guion packages."""

from __future__ import annotations

import argparse
import hashlib
import hmac
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import threading
import tempfile
import time

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "evaluation" / "protocol"))
import case_runner  # noqa: E402
import generated_applications  # noqa: E402


APP_IDS = {
    "sla-report": "01-support-sla-report",
    "jsonl-summary": "02-jsonl-retention",
    "release-notes": "03-release-notes",
    "calendar-conflicts": "04-calendar-conflict-report",
    "webhook-replay": "05-webhook-replay",
    "health-digest": "06-service-health-digest",
    "config-migrator": "07-config-migrator",
    "doc-index": "08-documentation-link-index",
    "lockfile-auditor": "09-lockfile-auditor",
}


def run_concurrent_webhook_check(launch: list[str], timeout: float, parent: Path) -> dict:
    """Race independent processes against one SQLite event id and verify one winner."""
    root = parent / "concurrent-webhook"
    root.mkdir()
    secret = b"guion-concurrent-test-secret-20261024"
    (root / "secret.txt").write_bytes(secret)
    environment = os.environ.copy()
    environment["GUION_FILE_ROOT"] = str(root)
    initialized = subprocess.run(
        [*launch, "init", "--db", "state.sqlite", "--secret-file", "secret.txt"],
        cwd=root,
        env=environment,
        capture_output=True,
        text=True,
        timeout=timeout,
    )
    if initialized.returncode != 0:
        return {
            "passed": False,
            "error": f"init exited {initialized.returncode}: {initialized.stderr.strip()}",
            "rounds": [],
        }

    rounds = []
    for round_index in range(3):
        event_id = f"concurrent-{round_index:02d}"
        body = json.dumps(
            {"id": event_id, "payload": {"round": round_index}},
            separators=(",", ":"),
        ).encode("utf-8") + b"\n"
        (root / "event.json").write_bytes(body)
        signature = hmac.new(secret, body, hashlib.sha256).hexdigest()
        gate = threading.Barrier(3)
        results: list[dict | None] = [None, None]

        def ingest(worker_index: int) -> None:
            gate.wait()
            completed = subprocess.run(
                [
                    *launch,
                    "ingest",
                    "--db",
                    "state.sqlite",
                    "--secret-file",
                    "secret.txt",
                    "--body",
                    "event.json",
                    "--signature",
                    signature,
                    "--result",
                    f"result-{round_index}-{worker_index}.json",
                ],
                cwd=root,
                env=environment,
                capture_output=True,
                text=True,
                timeout=timeout,
            )
            result_path = root / f"result-{round_index}-{worker_index}.json"
            try:
                result_data = json.loads(result_path.read_text(encoding="utf-8"))
            except (OSError, json.JSONDecodeError):
                result_data = None
            results[worker_index] = {
                "exit_code": completed.returncode,
                "result": result_data,
                "stderr": completed.stderr.strip(),
            }

        workers = [
            threading.Thread(target=ingest, args=(worker_index,))
            for worker_index in range(2)
        ]
        for worker in workers:
            worker.start()
        gate.wait()
        for worker in workers:
            worker.join(timeout + 1)
        outcomes = [item for item in results if item is not None]
        accepted = [
            item
            for item in outcomes
            if item["exit_code"] == 0
            and item["result"] == {"accepted": True, "id": event_id}
        ]
        duplicates = [
            item
            for item in outcomes
            if item["exit_code"] == 1
            and item["result"] == {"accepted": False, "reason": "duplicate_id"}
        ]
        rounds.append(
            {
                "event_id": event_id,
                "passed": len(outcomes) == 2 and len(accepted) == 1 and len(duplicates) == 1,
                "outcomes": outcomes,
            }
        )

    status = subprocess.run(
        [*launch, "status", "--db", "state.sqlite", "--output", "status.json"],
        cwd=root,
        env=environment,
        capture_output=True,
        text=True,
        timeout=timeout,
    )
    try:
        status_data = json.loads((root / "status.json").read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        status_data = None
    expected_pending = [f"concurrent-{round_index:02d}" for round_index in range(3)]
    status_passed = (
        status.returncode == 0
        and status_data == {"pending": expected_pending, "delivered": []}
    )
    return {
        "passed": status_passed and all(item["passed"] for item in rounds),
        "round_count": len(rounds),
        "status": status_data,
        "status_exit_code": status.returncode,
        "rounds": rounds,
    }


def run_application(
    name: str, task_id: str, manifest: dict, timeout: float, applications_root: Path
) -> dict:
    app_root = applications_root / name
    delivery_path = app_root / "delivery.json"
    delivery_spec = json.loads(delivery_path.read_text(encoding="utf-8"))
    with tempfile.TemporaryDirectory(prefix="guion-completion-移動-") as scratch:
        scratch_root = Path(scratch)
        relocated_root = scratch_root / "Moved package – Guion"
        relocated_root.mkdir()
        artifact_root, delivery = case_runner._prepare_delivery(
            app_root, artifact_root=relocated_root
        )
        launch = [
            item.replace("${candidate}", str(artifact_root))
            for item in delivery_spec["launch_command"]
        ]
        candidate = {
            "command": launch,
            "env": {"GUION_FILE_ROOT": "${workspace}"},
        }
        empty_candidate = scratch_root / "empty-candidate"
        empty_candidate.mkdir()
        stage_reports = {}
        for stage in ("initial", "follow_up"):
            cases = next(item for item in manifest["tasks"] if item["id"] == task_id)[stage].get("cases", [])
            started = time.perf_counter()
            results = [
                case_runner.run_case(
                    case,
                    candidate,
                    empty_candidate,
                    timeout_seconds=timeout,
                )
                for case in cases
            ]
            stage_reports[stage] = {
                "passed": bool(results) and all(item["passed"] for item in results),
                "elapsed_seconds": time.perf_counter() - started,
                "case_count": len(results),
                "cases": results,
            }
        generated_cases = generated_applications.generated_cases(name)
        generated_results = [
            case_runner.run_case(
                case,
                candidate,
                empty_candidate,
                timeout_seconds=timeout,
            )
            for case in generated_cases
        ]
        concurrency = None
        if name == "webhook-replay":
            concurrency = run_concurrent_webhook_check(launch, timeout, scratch_root)
        generated_report = {
            "schema_version": 1,
            "application": name,
            "product": "Guion",
            "mode": "seeded independent-oracle cases against relocated package",
            "seed": generated_applications.SEED,
            "sizes": list(generated_applications.SIZES),
            "case_count": len(generated_results),
            "concurrency": concurrency,
            "passed": bool(generated_results)
            and all(item["passed"] for item in generated_results)
            and (concurrency is None or concurrency["passed"]),
            "cases": generated_results,
        }
        return {
            "schema_version": 1,
            "task_id": task_id,
            "application": name,
            "mode": "source-free relocated package",
            "relocation_path": "Moved package – Guion",
            "product": "Guion",
            "artifact_sha256": delivery["metadata"]["artifact_sha256"],
            "artifact_bytes": delivery["metadata"]["artifact_bytes"],
            "source_paths_excluded": delivery["metadata"]["source_paths"],
            "stages": stage_reports,
            "passed": all(report["passed"] for report in stage_reports.values()),
            "generated": generated_report,
        }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--application", choices=sorted(APP_IDS))
    parser.add_argument("--timeout", type=float, default=60.0)
    parser.add_argument(
        "--applications-root",
        type=Path,
        default=ROOT / "evaluation" / "completion" / "applications",
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=ROOT / "evaluation" / "completion" / "results" / "guion-applications",
    )
    args = parser.parse_args()
    manifest_path = ROOT / "evaluation" / "fixtures" / "independent-v1" / "manifest.json"
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    selected = [args.application] if args.application else list(APP_IDS)
    args.output.mkdir(parents=True, exist_ok=True)
    all_passed = True
    for name in selected:
        task_id = APP_IDS[name]
        report = run_application(name, task_id, manifest, args.timeout, args.applications_root)
        target = args.output / f"{name}.json"
        target.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        generated_target = args.output / f"{name}-generated.json"
        generated_target.write_text(
            json.dumps(report["generated"], ensure_ascii=False, indent=2) + "\n",
            encoding="utf-8",
        )
        print(
            json.dumps(
                {
                    "application": name,
                    "passed": report["passed"],
                    "initial": report["stages"]["initial"]["case_count"],
                    "follow_up": report["stages"]["follow_up"]["case_count"],
                    "report": str(target),
                },
                ensure_ascii=False,
                sort_keys=True,
            ),
            flush=True,
        )
        all_passed = all_passed and report["passed"]
    return 0 if all_passed else 1


if __name__ == "__main__":
    raise SystemExit(main())
