#!/usr/bin/env bash
set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
if [[ "$(uname -s)" != "Linux" || "$(uname -m)" != "x86_64" ]]; then
    echo "This release matrix must run on x86_64 Linux inside the Debian 12 job container." >&2
    exit 2
fi
if [[ ! -r /etc/debian_version || "$(cut -d. -f1 /etc/debian_version)" != "12" ]]; then
    echo "This release matrix requires Debian 12." >&2
    exit 2
fi

EVIDENCE_ROOT="$REPO_ROOT/target/perf-session/debian-x86_64-evidence"
mkdir -p "$REPO_ROOT/target/perf-session"
rm -rf "$EVIDENCE_ROOT"
mkdir -p "$EVIDENCE_ROOT/reports" "$EVIDENCE_ROOT/logs" "$EVIDENCE_ROOT/applications"
export CARGO_TARGET_DIR="$REPO_ROOT/target/perf-session/cargo"
export CARGO_INCREMENTAL=0
export CARGO_NET_OFFLINE=true
export CARGO_NET_RETRY=0
export TMPDIR="$REPO_ROOT/target/perf-session/tmp"
export GUION_APPLICATION_RESULTS="$EVIDENCE_ROOT/applications"
export GUION_RUNTIME_MEMORY_RESULTS="$EVIDENCE_ROOT/reports"
export GUION_SOURCE_REVISION="${GITHUB_SHA:-}"
mkdir -p "$TMPDIR"

run_gate() {
    local gate_id="$1"
    local report_name="$2"
    local target_required="$3"
    shift 3
    local log_path="$EVIDENCE_ROOT/logs/$gate_id.log"
    local report_path="$REPO_ROOT/evaluation/completion/results/$report_name.json"
    local copied_report="$EVIDENCE_ROOT/reports/$report_name.json"
    local command_status=0

    echo "Running $gate_id: cargo xtask $*"
    if cargo xtask "$@" >"$log_path" 2>&1; then
        command_status=0
    else
        command_status=$?
    fi
    tail -n 60 "$log_path"
    if [[ -f "$report_path" ]]; then
        cp "$report_path" "$copied_report"
    fi

    local result
    result=$(python3 - "$copied_report" "$command_status" "$target_required" <<'PY'
import json
import sys
from pathlib import Path

path = Path(sys.argv[1])
try:
    report = json.loads(path.read_text(encoding="utf-8"))
except (OSError, json.JSONDecodeError):
    print("FAIL")
    raise SystemExit(0)
passed = report.get("status") == "passed" and sys.argv[2] == "0"
if sys.argv[3] == "yes":
    passed = passed and report.get("target") == "debian-x86_64"
    passed = passed and "debian-x86_64" in report.get("completed_targets", [])
print("PASS" if passed else "FAIL")
PY
)
    printf '%s\t%s\t%s\n' "$gate_id" "$result" "$command_status" >> "$EVIDENCE_ROOT/gate-status.tsv"
}

run_gate conformance-inventory conformance-inventory no conformance --inventory-only
run_gate bootstrap bootstrap yes bootstrap --stages 3
run_gate conformance conformance yes conformance --compilers stage2,stage3
run_gate stdlib-data stdlib-data yes stdlib --group data
run_gate stdlib-services stdlib-services yes stdlib --group services
run_gate runtime runtime yes runtime
run_gate product-dependencies product-dependencies yes product --group dependencies
run_gate product-lifecycle product-lifecycle yes product --group lifecycle
run_gate product-applications product-applications yes product --group applications --stage initial,follow-up
run_gate release release yes release

python3 - "$REPO_ROOT" "$EVIDENCE_ROOT" <<'PY'
import json
import os
import platform
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

repo = Path(sys.argv[1])
evidence = Path(sys.argv[2])
revision = os.environ.get("GUION_SOURCE_REVISION")
if not revision:
    revision = subprocess.run(
        ["git", "rev-parse", "HEAD"], cwd=repo, text=True, capture_output=True, check=True
    ).stdout.strip()
libc_name, libc_version = platform.libc_ver()
environment = {
    "captured_at_utc": datetime.now(timezone.utc).isoformat(),
    "source_revision": revision,
    "operating_system": platform.platform(),
    "distribution": Path("/etc/debian_version").read_text(encoding="utf-8").strip(),
    "machine": platform.machine(),
    "libc": {"name": libc_name, "version": libc_version},
    "rustc": subprocess.run(["rustc", "--version"], text=True, capture_output=True, check=True).stdout.strip(),
    "cargo": subprocess.run(["cargo", "--version"], text=True, capture_output=True, check=True).stdout.strip(),
}
(evidence / "environment.json").write_text(
    json.dumps(environment, indent=2, sort_keys=True) + "\n", encoding="utf-8"
)

gates = []
for line in (evidence / "gate-status.tsv").read_text(encoding="utf-8").splitlines():
    name, status, code = line.split("\t")
    report_path = evidence / "reports" / f"{name}.json"
    report_data = {}
    if report_path.is_file():
        report_data = json.loads(report_path.read_text(encoding="utf-8"))
    gates.append((name, status, code, report_data.get("target", "")))

application_reports = sorted((evidence / "applications").glob("*.json"))
application_passed = 0
for report_path in application_reports:
    try:
        report = json.loads(report_path.read_text(encoding="utf-8"))
    except json.JSONDecodeError:
        continue
    if report.get("passed") is True:
        application_passed += 1
app_status = "PASS" if len(application_reports) == 18 and application_passed == 18 else "FAIL"
all_passed = all(status == "PASS" for _, status, _, _ in gates) and app_status == "PASS"
lines = [
    "# Debian 12 x86_64 release-target verification",
    "",
    f"Source revision: `{revision}`",
    f"Host: Debian {environment['distribution']}, {environment['machine']}, {libc_name} {libc_version}",
    "",
    "| Gate | Result | Exit | Target |",
    "| --- | --- | ---: | --- |",
]
for name, status, code, target in gates:
    lines.append(f"| `{name}` | {status} | {code} | {target or 'inventory'} |")
lines.append(f"| `application reports` | {app_status} ({application_passed}/18) | — | debian-x86_64 |")
lines.extend(["", f"**Overall: {'PASS' if all_passed else 'FAIL'}.**", ""])
(evidence / "summary.md").write_text("\n".join(lines), encoding="utf-8")
print((evidence / "summary.md").read_text(encoding="utf-8"))
if os.environ.get("GITHUB_STEP_SUMMARY"):
    with open(os.environ["GITHUB_STEP_SUMMARY"], "a", encoding="utf-8") as summary:
        summary.write((evidence / "summary.md").read_text(encoding="utf-8"))
raise SystemExit(0 if all_passed else 1)
PY
