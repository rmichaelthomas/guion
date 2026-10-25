#!/bin/sh
set -eu
cd /private/tmp/veritas-fresh-builders/field-kit-log-veritas
report_dir=/Users/rmichaelthomas/Documents/Codex/2026-10-22/referenced-chatgpt-conversation-this-is-an/work/veritas-foundation/evaluation/completion/builder/reports
evidence_dir="$report_dir/evidence/field-kit-log-veritas"
mkdir -p "$evidence_dir"
cp -R app data evidence package package-v1 package-final 'Field Kit – café' acceptance-initial.sh acceptance-followup.sh acceptance-relocation.sh preserve-report.sh "$evidence_dir/"
finish_utc=$(date -u '+%Y-%m-%dT%H:%M:%SZ')
finish_epoch=$(date -u '+%s')
start_epoch=$(date -j -u -f '%Y-%m-%dT%H:%M:%SZ' '2026-10-25T10:51:47Z' '+%s')
elapsed_seconds=$((finish_epoch - start_epoch))
test "$elapsed_seconds" -le 900
report_path="$report_dir/field-kit-log-veritas.json"
cat > "$report_path" <<JSON
{
  "schema_version": 1,
  "assignment_id": "field-kit-log-veritas",
  "status": "complete",
  "language": "veritas",
  "brief_id": "field-kit-log",
  "target_class": "maintainable-local-app",
  "start_utc": "2026-10-25T10:51:47Z",
  "finish_utc": "$finish_utc",
  "budget_minutes": 15,
  "elapsed_seconds": $elapsed_seconds,
  "initial_correct": true,
  "followup_correct": true,
  "relocation_passed": true,
  "setup_seconds": 25,
  "build_seconds": 105,
  "followup_seconds": 11,
  "installed_launch_seconds": 0.39,
  "artifact_bytes": 12880529,
  "artifact_bytes_basis": "Final package regular-file bytes (12872255) plus source, manifest, sample config and operator guide (8274); excludes database, duplicate package snapshots and acceptance evidence.",
  "timing_notes": "Start is the first captured UTC task-log timestamp, as required by protocol. Required contract opening and skill read immediately preceded it and were within the 15-minute dispatch budget. Setup/build phases are self-reported wall-clock estimates totaling 130 seconds through initial acceptance at 10:53:57Z. Follow-up timestamps are 10:54:11Z and 10:54:22Z. Remaining elapsed includes installation, relocation, timezone checks, guide, evidence and report production. Launch is one time -p observation, not a benchmark.",
  "interventions": ["No human repair or author rescue. Parent confirmed use of the first captured UTC task-log timestamp and disclosure of immediately preceding contract setup."],
  "failures": [
    "Evidence validator mismatch: macOS plutil rejected JSON as a property list; installed Veritas JSON parse and report field validation passed.",
    "Resolved CLI discovery probe: --help is not supported and returned usage with exit 1.",
    "Resolved generated entry lookup: app/main.vr did not exist; generated src/main.vr was used.",
    "Resolved syntax probe: Python-style from/import rejected; import quoted std module accepted.",
    "Resolved conversion probes: str, text and to_text unavailable; documented JSON integer conversion used.",
    "Resolved documentation/runtime inconsistency: absolute SQLite paths required by host-api.md are rejected with E_SQLITE_PATH; relative database filename with explicit external file root works. Initial absolute-path package attempts created no records."
  ],
  "acceptance": {
    "item_creation": true,
    "pass_and_fail": true,
    "automatic_local_date": true,
    "explicit_clock_grant": true,
    "native_sqlite_and_toml": true,
    "restart_persistence": true,
    "stable_sequence_order": true,
    "duplicate_unknown_invalid_rejected": true,
    "rejected_operations_byte_identical_database": true,
    "original_list_preserved_after_edit": true,
    "failed_only_filter": true,
    "unknown_filter_rejected_without_database_change": true,
    "installed_package_verified": true,
    "relocated_installed_package_verified": true,
    "relocation_spaces_and_unicode": true,
    "separate_preexisting_caller_selected_data_directory": true,
    "relocated_write_and_restart": true,
    "timezone_specific_date": true
  },
  "recommendation": {
    "preferred_language": "veritas",
    "reason": "For this bounded local application, I would choose the delivered Veritas implementation: native transactional SQLite and TOML, an explicitly granted local clock, and a relocatable installed application all passed. This judgment uses only my own application and maintenance experience; I did not inspect or compare another implementation.",
    "observed_user_benefit": "Operators receive a standalone current-host installation with no Python or third-party setup, automatic local dates, persistent ordered inspection history, and database state preserved across invalid operations. The requested filter required a small source edit and retained original output.",
    "material_regressions": [
      "Installed documentation requires absolute SQLite paths but runtime requires relative paths. A new builder following the documented contract fails until discovering the relative-path workaround.",
      "The allowed documentation lacks a compact language syntax and conversion reference; import and integer-formatting discovery required failed probes."
    ]
  },
  "evidence_paths": [
JSON
printf '    "%s",\n' "$report_path" >> "$report_path"
printf '    "%s",\n' "$evidence_dir/report.json" >> "$report_path"
find "$evidence_dir" -type f | LC_ALL=C sort | awk '{gsub(/\\/, "\\\\"); gsub(/"/, "\\\""); if (NR > 1) printf ",\n"; printf "    \"%s\"", $0} END {printf "\n"}' >> "$report_path"
printf '  ]\n}\n' >> "$report_path"
cp "$report_path" "$evidence_dir/report.json"
printf '%s\n' "$report_path" "$finish_utc" "$elapsed_seconds"
