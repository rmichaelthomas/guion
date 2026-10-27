#!/usr/bin/env bash
set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET="${GUION_TARGET:-}"
case "$TARGET" in
    macos-aarch64)
        [[ "$(uname -s)" == "Darwin" && "$(uname -m)" == "arm64" ]] || { echo "macos-aarch64 requires Apple Silicon macOS." >&2; exit 2; }
        ;;
    debian-aarch64)
        [[ "$(uname -s)" == "Linux" && "$(uname -m)" == "aarch64" && -r /etc/debian_version && "$(cut -d. -f1 /etc/debian_version)" == "12" ]] || { echo "debian-aarch64 requires Debian 12 on native ARM64 Linux." >&2; exit 2; }
        ;;
    debian-x86_64)
        [[ "$(uname -s)" == "Linux" && "$(uname -m)" == "x86_64" && -r /etc/debian_version && "$(cut -d. -f1 /etc/debian_version)" == "12" ]] || { echo "debian-x86_64 requires Debian 12 on native x86_64 Linux." >&2; exit 2; }
        ;;
    *) echo "GUION_TARGET must be macos-aarch64, debian-aarch64, or debian-x86_64." >&2; exit 2 ;;
esac

EVIDENCE_ROOT="$REPO_ROOT/target/perf-session/$TARGET-evidence"
TARGET_RUNS_ROOT="$REPO_ROOT/evaluation/completion/results/target-runs"
PRIOR_REPORTS="${GUION_PRIOR_TARGET_REPORTS:-}"
mkdir -p "$REPO_ROOT/target/perf-session"
rm -rf "$EVIDENCE_ROOT"
mkdir -p "$EVIDENCE_ROOT/reports" "$EVIDENCE_ROOT/logs" "$EVIDENCE_ROOT/applications" "$EVIDENCE_ROOT/target-runs"
if [[ -z "${GUION_SOURCE_REVISION:-}" ]]; then
    GUION_SOURCE_REVISION="$(git -C "$REPO_ROOT" rev-parse HEAD)"
fi
export GUION_SOURCE_REVISION
export CARGO_TARGET_DIR="$REPO_ROOT/target/perf-session/cargo"
export CARGO_INCREMENTAL=0
export CARGO_NET_OFFLINE=true
export CARGO_NET_RETRY=0
export TMPDIR="$REPO_ROOT/target/perf-session/tmp"
export GUION_APPLICATION_RESULTS="$EVIDENCE_ROOT/applications"
export GUION_RUNTIME_MEMORY_RESULTS="$EVIDENCE_ROOT/reports"
mkdir -p "$TMPDIR"

report_names=(conformance-inventory bootstrap conformance stdlib-data stdlib-services runtime product-dependencies product-lifecycle product-applications release)
targets=(macos-aarch64 debian-aarch64 debian-x86_64)
for report in "${report_names[@]}"; do
    for target in "${targets[@]}"; do
        rm -f "$TARGET_RUNS_ROOT/$report-$target.json"
    done
done
if [[ -n "$PRIOR_REPORTS" && -d "$PRIOR_REPORTS" ]]; then
    mkdir -p "$TARGET_RUNS_ROOT"
    cp -a "$PRIOR_REPORTS"/*.json "$TARGET_RUNS_ROOT/" 2>/dev/null || true
fi

run_gate() {
    local gate_id="$1"
    local report_name="$2"
    local target_required="$3"
    shift 3
    local log_path="$EVIDENCE_ROOT/logs/$gate_id.log"
    local report_path="$REPO_ROOT/evaluation/completion/results/$report_name.json"
    local copied_report="$EVIDENCE_ROOT/reports/$report_name.json"
    local command_status=0
    if [[ "$target_required" == "yes" ]]; then
        echo "Running $gate_id on $TARGET: cargo xtask $* --target both"
        if cargo xtask "$@" --target both >"$log_path" 2>&1; then
            command_status=0
        else
            command_status=$?
        fi
    else
        echo "Running $gate_id on $TARGET: cargo xtask $*"
        if cargo xtask "$@" >"$log_path" 2>&1; then
            command_status=0
        else
            command_status=$?
        fi
    fi

    tail -n 60 "$log_path"
    if [[ -f "$report_path" ]]; then
        cp "$report_path" "$copied_report"
    fi

    local result
    result=$(python3 - "$copied_report" "$command_status" "$target_required" "$TARGET" <<'PY'
import json
import sys
from pathlib import Path

path = Path(sys.argv[1])
try:
    report = json.loads(path.read_text(encoding="utf-8"))
except (OSError, json.JSONDecodeError):
    print("FAIL")
    raise SystemExit(0)
target = sys.argv[4]
checks = report.get("checks", [])
target_checks = [item for item in checks if item.get("target") == target]
if not target_checks and report.get("target") == target:
    target_checks = checks
passed = report.get("status") != "failed" and sys.argv[2] == "0" and bool(target_checks)
passed = passed and all(item.get("status") == "passed" for item in target_checks)
if sys.argv[3] == "yes":
    passed = passed and target in report.get("completed_targets", [])
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

for report in "${report_names[@]}"; do
    for target in "${targets[@]}"; do
        source="$TARGET_RUNS_ROOT/$report-$target.json"
        if [[ -f "$source" ]]; then
            cp "$source" "$EVIDENCE_ROOT/target-runs/"
        fi
    done
done

python3 - "$REPO_ROOT" "$EVIDENCE_ROOT" "$TARGET" "${GUION_REQUIRE_ALL_TARGETS:-0}" "$GUION_SOURCE_REVISION" <<'PY'
import json
import os
import platform
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

evidence = Path(sys.argv[2])
target = sys.argv[3]
require_all_targets = sys.argv[4] == "1"
revision = sys.argv[5]
distribution = (
    "Debian " + Path("/etc/debian_version").read_text(encoding="utf-8").strip()
    if sys.platform.startswith("linux")
    else "macOS " + (platform.mac_ver()[0] or "unknown")
)
libc_name, libc_version = platform.libc_ver()
environment = {
    "captured_at_utc": datetime.now(timezone.utc).isoformat(),
    "source_revision": revision,
    "operating_system": platform.platform(),
    "distribution": distribution,
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
app_passed = len(application_reports) == 18 and application_passed == 18
required_targets = ["macos-aarch64", "debian-aarch64", "debian-x86_64"]
aggregate_names = [
    "bootstrap", "conformance", "stdlib-data", "stdlib-services", "runtime",
    "product-dependencies", "product-lifecycle", "product-applications", "release",
]
aggregate_passed = True
for name in aggregate_names:
    try:
        report = json.loads((evidence / "reports" / f"{name}.json").read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        aggregate_passed = False
        continue
    aggregate_passed &= report.get("status") == "passed" and report.get("target") == "both"
    aggregate_passed &= report.get("required_targets") == required_targets
    aggregate_passed &= report.get("completed_targets") == required_targets and report.get("skipped_targets") == []
    aggregate_passed &= report.get("source_revision") == revision
gates_passed = all(status == "PASS" for _, status, _, _ in gates)
app_status = "PASS" if app_passed else "FAIL"
lines = [
    "# Guion release-target verification",
    "",
    f"Source revision: `{revision}`",
    f"Target: `{target}`",
    f"Host: {distribution}, {environment['machine']}, {libc_name} {libc_version}",
    "",
    "| Gate | Result | Exit | Target |",
    "| --- | --- | ---: | --- |",
]
for name, status, code, target in gates:
    lines.append(f"| `{name}` | {status} | {code} | {target or 'inventory'} |")
lines.append(f"| `application reports` | {app_status} ({application_passed}/18) | — | {target} |")
if require_all_targets:
    lines.append(f"| `three-target aggregate` | {'PASS' if aggregate_passed else 'FAIL'} | — | all targets |")
    all_passed = gates_passed and app_passed and aggregate_passed
    lines.extend(["", f"**Overall: {'PASS' if all_passed else 'FAIL'}.**", ""])
else:
    all_passed = gates_passed and app_passed
    lines.extend(["", f"Target result: **{'PASS' if all_passed else 'FAIL'}**."])
    lines.extend(["Three-target aggregates remain pending the remaining target runners.", ""])
(evidence / "summary.md").write_text("\n".join(lines), encoding="utf-8")
print((evidence / "summary.md").read_text(encoding="utf-8"))
if os.environ.get("GITHUB_STEP_SUMMARY"):
    with open(os.environ["GITHUB_STEP_SUMMARY"], "a", encoding="utf-8") as summary:
        summary.write((evidence / "summary.md").read_text(encoding="utf-8"))
raise SystemExit(0 if all_passed else 1)
PY
