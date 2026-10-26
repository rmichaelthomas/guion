#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
GUION="$ROOT/target/release/guion"
WORKLOAD="$ROOT/evaluation/development/famous_benchmarks/richards.vr"
SCRATCH="$ROOT/target/perf-session/guion-richards-host-profile"
RESULTS="$ROOT/evaluation/completion/results/runtime-efficiency"
PROFILE="$RESULTS/2026-10-26-richards-spindump.txt"
METADATA="$RESULTS/2026-10-26-richards-host-cpu-attribution.json"

if [[ ! -x "$GUION" ]]; then
  echo "Missing $GUION. Build it first with: cargo build --locked --offline -p guion-cli --release" >&2
  exit 2
fi
if [[ ! -f "$WORKLOAD" ]]; then
  echo "Missing Richards workload: $WORKLOAD" >&2
  exit 2
fi
if [[ -e "$SCRATCH" ]]; then
  echo "Refusing to overwrite existing scratch directory: $SCRATCH" >&2
  exit 2
fi
mkdir -p "$(dirname "$SCRATCH")" "$RESULTS"
mkdir "$SCRATCH"
PID=""
cleanup() {
  if [[ -n "$PID" ]] && kill -0 "$PID" 2>/dev/null; then
    kill "$PID" 2>/dev/null || true
    wait "$PID" 2>/dev/null || true
  fi
  rm -rf "$SCRATCH"
}
trap cleanup EXIT INT TERM

python3 - "$WORKLOAD" "$SCRATCH" <<'PY'
from pathlib import Path
import sys
source_path, scratch_path = map(Path, sys.argv[1:])
text = source_path.read_text(encoding="utf-8")
signature = "fn main() -> Int:"
if text.count(signature) != 1:
    raise SystemExit("Expected one Richards main function")
text = text.replace(signature, "fn benchmark_once() -> Int:", 1)
text += "\nfn main() -> Int:\n    var pass = 0\n    while pass < 3000:\n        let result = benchmark_once()\n        if result != 0:\n            return result\n        set pass = pass + 1\n    return 0\n"
(scratch_path / "main.gu").write_text(text, encoding="utf-8")
(scratch_path / "guion.toml").write_text(
    '[project]\nname = "richards-host-profile"\nversion = "0.1.0"\nentry = "main.gu"\n\n'
    '[runtime]\ninstruction_limit = 90000000000\nallocation_limit_bytes = 300000000000\n',
    encoding="utf-8",
)
PY

"$GUION" build --json "$SCRATCH/main.gu" > "$SCRATCH/build-report.json"
"$GUION" run --json "$SCRATCH/main.gub" > "$SCRATCH/run.json" 2> "$SCRATCH/run.stderr" &
PID=$!
sleep 0.1
set +e
sudo /usr/sbin/spindump "$PID" 4 10 -heavy -o "$SCRATCH/spindump.txt"
SPINDUMP_STATUS=$?
wait "$PID"
RUN_STATUS=$?
PID=""
set -e

python3 - "$SCRATCH" "$RESULTS" "$PROFILE" "$METADATA" "$SPINDUMP_STATUS" "$RUN_STATUS" <<'PY'
from datetime import datetime, timezone
import hashlib, json, shutil, sys
from pathlib import Path
scratch, results, profile, metadata = map(Path, sys.argv[1:5])
spindump_status, run_status = map(int, sys.argv[5:7])
envelope = json.loads((scratch / "run.json").read_text(encoding="utf-8"))
lines = envelope.get("program_stdout", "").splitlines()
output_ok = envelope.get("status") == "passed" and run_status == 0 and lines == ["9297", "23246"] * 3000
raw = scratch / "spindump.txt"
profile_bytes = raw.stat().st_size if raw.is_file() else 0
profile_ok = spindump_status == 0 and profile_bytes > 0
if profile_ok:
    shutil.copy2(raw, profile)
profile_sha = hashlib.sha256(profile.read_bytes()).hexdigest() if profile_ok else None
build = json.loads((scratch / "build-report.json").read_text(encoding="utf-8"))
record = {
    "schema_version": 1,
    "status": "passed" if output_ok and profile_ok else "incomplete",
    "captured_at_utc": datetime.now(timezone.utc).isoformat(timespec="seconds").replace("+00:00", "Z"),
    "workload": "Richards, 3000 repetitions in one artifact process",
    "guion_version": "0.1.0 (artifact format 39)",
    "artifact_sha256": build["artifact_sha256"],
    "spindump_exit_code": spindump_status,
    "program_exit_code": run_status,
    "output_lines": len(lines),
    "output_matches_expected": output_ok,
    "profile_path": profile.name if profile_ok else None,
    "profile_bytes": profile_bytes,
    "profile_sha256": profile_sha,
    "interpretation": "A CPU profile is valid only when status is passed; output correctness and host samples are both required.",
}
metadata.write_text(json.dumps(record, indent=2) + "\n", encoding="utf-8")
print(json.dumps(record, indent=2))
if not output_ok or not profile_ok:
    raise SystemExit(1)
PY
