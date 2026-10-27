#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET_DIR="${CARGO_TARGET_DIR:-$REPO_ROOT/target}"
if [[ "$TARGET_DIR" != /* ]]; then
    TARGET_DIR="$REPO_ROOT/$TARGET_DIR"
fi
CLI="$TARGET_DIR/debug/guion"

(
    cd "$REPO_ROOT"
    CARGO_TARGET_DIR="$TARGET_DIR" CARGO_INCREMENTAL=0 cargo build --locked --offline -p guion-cli
)

mkdir -p "$REPO_ROOT/target/perf-session"
GUION_REPO_ROOT="$REPO_ROOT" \
GUION_CLI="$CLI" \
GUION_SCRATCH_PARENT="$REPO_ROOT/target/perf-session" \
python3 - <<'PY'
import json
import os
import shutil
import subprocess
import tempfile
from datetime import datetime, timezone
from pathlib import Path

repo = Path(os.environ["GUION_REPO_ROOT"])
cli = Path(os.environ["GUION_CLI"])
scratch_parent = Path(os.environ["GUION_SCRATCH_PARENT"])
index_path = repo / "docs/agents/index.json"
report_path = repo / "docs/agents/agent-examples-verification.md"
examples_dir = repo / "docs/agents/examples"
scratch = Path(tempfile.mkdtemp(prefix="agent-examples-", dir=scratch_parent))
rows = []


def record(label, passed, details):
    rows.append((label, passed, " ".join(str(details).split())))


def run(label, command, *, cwd=repo):
    result = subprocess.run(command, cwd=cwd, text=True, capture_output=True)
    details = (result.stdout + result.stderr).strip().replace(f"{repo}/", "")
    warning_output = "warning[" in details.lower()
    passed = result.returncode == 0 and not warning_output
    if warning_output and result.returncode == 0:
        details += " (lint warnings are treated as failures)"
    record(label, passed, f"exit {result.returncode}; {details or 'no output'}")
    return result


try:
    index = json.loads(index_path.read_text(encoding="utf-8"))
    examples = index["examples"]
    example_by_id = {example["id"]: example for example in examples}
    if len(example_by_id) != len(examples):
        raise ValueError("duplicate example id in docs/agents/index.json")

    expected_sources = {
        "native_temporal_resolution",
        "file_data_cli",
        "http_request",
        "sqlite_toml",
        "concurrent_task",
        "agent_failure_paths",
    }
    actual_sources = {example["id"] for example in examples}
    if actual_sources != expected_sources:
        raise ValueError(f"indexed examples differ from expected set: {sorted(actual_sources)}")

    for example in examples:
        source = repo / example["source"]
        for verb, extra in (("check", []), ("lint", []), ("fmt", ["--check"])):
            run(f"{example['id']} {verb}", [str(cli), verb, *extra, str(source)])

    cases = []
    seen_case_ids = set()
    for example in examples:
        for case in example.get("verification_cases", []):
            case_id = case["id"]
            if case_id in seen_case_ids:
                raise ValueError(f"duplicate verification case id: {case_id}")
            seen_case_ids.add(case_id)
            cases.append((example, case))

    if len(cases) != 8:
        raise ValueError(f"expected 8 indexed runtime cases, found {len(cases)}")

    for example, case in cases:
        source = Path(example["source"]).name
        args = [str(cli), "run", "--no-cache"]
        if case["id"] in {"sqlite_toml_run", "failure_paths_sqlite_escape"}:
            file_root = scratch / case["id"]
            file_root.mkdir(parents=True)
            args.extend(["--file-root", str(file_root)])
        args.extend([source, *case["arguments"]])
        result = subprocess.run(args, cwd=examples_dir, text=True, capture_output=True)
        stdout_ok = result.stdout == case["stdout"]
        exit_ok = result.returncode == case["exit_code"]
        details = (
            f"exit {result.returncode} (expected {case['exit_code']}); "
            f"stdout {result.stdout!r} (expected {case['stdout']!r})"
        )
        if result.stderr:
            details += f"; stderr {result.stderr!r}"
        record(case["id"], stdout_ok and exit_ok, details)

    passed_count = sum(1 for _, passed, _ in rows if passed)
    status = "PASS" if passed_count == len(rows) else "FAIL"
    timestamp = datetime.now(timezone.utc).strftime("%Y-%m-%d %H:%M UTC")
    lines = [
        "# Agent examples verification",
        "",
        f"Generated {timestamp} by `scripts/verify-agent-examples.sh`.",
        "",
        "The HTTP example was checked, linted, and formatted; it was not run and no network request was made.",
        "Runtime stdout and exit status are compared with `verification_cases` in `docs/agents/index.json`.",
        "",
        "| Check | Result | Details |",
        "| --- | --- | --- |",
    ]
    for label, passed, details in rows:
        safe_details = details.replace("|", "\\|")
        lines.append(f"| `{label}` | {'PASS' if passed else 'FAIL'} | {safe_details} |")
    lines.extend(["", f"**Overall: {status} ({passed_count}/{len(rows)} checks passed).**", ""])
    report_path.write_text("\n".join(lines), encoding="utf-8")
    print("| Check | Result | Details |")
    print("| --- | --- | --- |")
    for label, passed, details in rows:
        print(f"| {label} | {'PASS' if passed else 'FAIL'} | {details} |")
    print(f"\nOverall: {status} ({passed_count}/{len(rows)} checks passed).")
    print(f"Report: {report_path}")
    raise SystemExit(0 if status == "PASS" else 1)
except Exception as exc:
    record("verification script", False, f"{type(exc).__name__}: {exc}")
    report_path.parent.mkdir(parents=True, exist_ok=True)
    details = " ".join(rows[-1][2].split())
    report_path.write_text(
        "# Agent examples verification\n\n"
        f"**Overall: FAIL.** {details}\n",
        encoding="utf-8",
    )
    print(f"FAIL: {details}")
    raise SystemExit(1)
finally:
    shutil.rmtree(scratch, ignore_errors=True)
PY
