import json
import platform
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
WORKLOAD = ROOT / "evaluation" / "development" / "sales_report"
EXPECTED = (WORKLOAD / "expected.txt").read_text(encoding="utf-8")


def run_case(name: str, command: list[str]) -> dict[str, object]:
    completed = subprocess.run(
        command,
        cwd=ROOT,
        text=True,
        encoding="utf-8",
        capture_output=True,
        check=False,
    )
    return {
        "name": name,
        "command": command,
        "exit_code": completed.returncode,
        "stdout": completed.stdout,
        "stderr": completed.stderr,
        "matches_expected": completed.returncode == 0 and completed.stdout == EXPECTED,
    }


def main() -> int:
    results = [
        run_case(
            "veritas_source",
            [
                "cargo",
                "run",
                "-q",
                "-p",
                "veritas-cli",
                "--offline",
                "--",
                "run",
                str(WORKLOAD),
                "sales.json",
            ],
        ),
        run_case(
            "python_standard_library",
            [
                "python3",
                str(WORKLOAD / "reference.py"),
                str(WORKLOAD / "sales.json"),
            ],
        ),
    ]
    report = {
        "schema_version": 1,
        "classification": "known_development_case_not_independent_evidence",
        "host": {"system": platform.system(), "machine": platform.machine()},
        "workload": "sales_report",
        "expected_stdout": EXPECTED,
        "results": results,
        "all_match": all(result["matches_expected"] for result in results),
        "performance_claim": False,
    }
    result_dir = ROOT / "evaluation" / "results"
    result_dir.mkdir(parents=True, exist_ok=True)
    destination = result_dir / "development-sales-report.json"
    temporary = destination.with_suffix(".json.tmp")
    temporary.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    temporary.replace(destination)
    print(json.dumps(report, indent=2))
    return 0 if report["all_match"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
