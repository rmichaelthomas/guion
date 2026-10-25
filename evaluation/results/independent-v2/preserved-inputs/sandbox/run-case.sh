#!/bin/sh
set -eu

if [ "$#" -ne 10 ]; then
    printf 'Usage: %s {python|veritas} TASK_ID {initial|follow_up} PAIR_ID SESSION_ID CANDIDATE_VERSION MANIFEST_SHA256 SCHEDULE_SHA256 CANDIDATE_PROJECT HOST_REPORT_PATH\n' "$0" >&2
    exit 2
fi

language=$1
task_id=$2
stage=$3
pair_id=$4
session_id=$5
candidate_version=$6
manifest_sha256=$7
schedule_sha256=$8
candidate_project=$(CDPATH= cd -- "$9" && pwd -P)
report_path=${10}
script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd -P)
protocol_dir="$script_dir/../protocol"

case "$language" in
    python) image=veritas-campaign-python:3.14.8; candidate_config="$script_dir/candidate-python.json" ;;
    veritas) image=veritas-campaign-veritas:0.1.0; candidate_config="$script_dir/candidate-veritas.json" ;;
    *) printf 'Language must be python or veritas\n' >&2; exit 2 ;;
esac
case "$stage" in
    initial|follow_up) ;;
    *) printf 'Stage must be initial or follow_up\n' >&2; exit 2 ;;
esac
if [ ! -f "$candidate_config" ]; then
    printf 'candidate configuration is missing: %s\n' "$candidate_config" >&2
    exit 2
fi

mkdir -p "$(dirname -- "$report_path")"
report_dir=$(CDPATH= cd -- "$(dirname -- "$report_path")" && pwd -P)
report_path="$report_dir/$(basename -- "$report_path")"
delivery_root="$report_path.delivery"
if [ -e "$delivery_root" ]; then
    printf 'refusing to overwrite existing delivery evidence: %s\n' "$delivery_root" >&2
    exit 2
fi
mkdir -p "$delivery_root"
temp_root=$(mktemp -d "${TMPDIR:-/tmp}/veritas-run-report.XXXXXX")
delivery_config="$report_path.delivery-candidate.json"
source_raw="$temp_root/source-report.json"
source_error="$temp_root/source-error.txt"
delivery_raw="$temp_root/delivery-report.json"
delivery_error="$temp_root/delivery-error.txt"
stage_error="$temp_root/stage-error.txt"
delivery_stage_exit=0
cleanup() {
    rm -rf "$temp_root"
    rm -f "$delivery_config"
    if [ "$delivery_stage_exit" -ne 0 ]; then rm -rf "$delivery_root"; fi
}
trap cleanup EXIT HUP INT TERM

image_id=$(docker image inspect --format '{{.Id}}' "$image")
docker_engine_id=$(docker info --format '{{.ID}}')
if [ -z "$docker_engine_id" ]; then
    printf 'Docker Engine ID is unavailable; cannot bind this run to its host\n' >&2
    exit 2
fi
package_sha=$(docker image inspect --format '{{index .Config.Labels "org.veritas.package.sha256"}}' "$image")
base_digest=$(docker image inspect --format '{{index .Config.Labels "org.veritas.python.base.digest"}}' "$image")
dependencies_sha=$(docker image inspect --format '{{index .Config.Labels "org.veritas.campaign.dependencies.sha256"}}' "$image")
uv_version=$(docker image inspect --format '{{index .Config.Labels "org.veritas.campaign.uv.version"}}' "$image")
python_version=$(docker image inspect --format '{{index .Config.Labels "org.veritas.campaign.python.version"}}' "$image")

python3 "$protocol_dir/stage_delivery.py" "$candidate_project" "$candidate_config" "$delivery_root" "$delivery_config" > "$temp_root/delivery-metadata.json" 2> "$stage_error" || delivery_stage_exit=$?
manifest_capture="$report_path.delivery-manifest.json"
if [ -f "$candidate_project/delivery.json" ]; then
    cp "$candidate_project/delivery.json" "$manifest_capture"
fi

# Source-free delivery runs in a second container that mounts only the staged artifacts.
run_container() {
    mounted_tree=$1
    mounted_config=$2
    raw_output=$3
    error_output=$4
    set -- docker run --rm \
        --platform linux/arm64 \
        --network none \
        --read-only \
        --tmpfs /tmp:rw,nosuid,nodev,noexec,size=512m,mode=1777 \
        --cap-drop ALL \
        --cap-add CHOWN \
        --cap-add DAC_OVERRIDE \
        --cap-add SETUID \
        --cap-add SETGID \
        --security-opt no-new-privileges \
        --pids-limit 128 \
        --memory 2g \
        --memory-swap 2g \
        --cpus 2 \
        --mount "type=bind,src=$mounted_tree,dst=/candidate,readonly"
    if [ -n "$mounted_config" ]; then
        set -- "$@" --mount "type=bind,src=$mounted_config,dst=/opt/runner/candidate.json,readonly"
    fi
    set -- "$@" "$image" /opt/runner/manifest.json "$task_id" "$stage" \
        /opt/runner/candidate.json /candidate --candidate-uid 10001 --source-only
    if "$@" > "$raw_output" 2> "$error_output"; then
        return 0
    else
        return $?
    fi
}

source_exit=0
run_container "$candidate_project" "" "$source_raw" "$source_error" || source_exit=$?
delivery_exit=125
if [ "$delivery_stage_exit" -eq 0 ]; then
    delivery_exit=0
    run_container "$delivery_root" "$delivery_config" "$delivery_raw" "$delivery_error" || delivery_exit=$?
fi

python3 - "$source_raw" "$source_error" "$delivery_raw" "$delivery_error" "$stage_error" "$delivery_config" "$report_path" "$language" "$task_id" "$stage" "$pair_id" "$session_id" "$candidate_version" "$manifest_sha256" "$schedule_sha256" "$image" "$image_id" "$docker_engine_id" "$package_sha" "$base_digest" "$dependencies_sha" "$uv_version" "$python_version" "$source_exit" "$delivery_stage_exit" "$delivery_exit" "$candidate_project" <<'PY'
import hashlib
import json
import pathlib
import sys

source_raw, source_error, delivery_raw, delivery_error, stage_error = map(pathlib.Path, sys.argv[1:6])
delivery_config, report_path = map(pathlib.Path, sys.argv[6:8])
language, task_id, stage, pair_id, session_id, candidate_version, manifest_sha256, schedule_sha256, image, image_id, docker_engine_id, package_sha, base_digest, dependencies_sha, uv_version, python_version, source_exit, stage_exit, delivery_exit, candidate_path = sys.argv[8:]

def load_json(path):
    try:
        return json.loads(path.read_text(encoding="utf-8", errors="replace"))
    except (OSError, json.JSONDecodeError):
        return None

source_report = load_json(source_raw)
delivery_report = load_json(delivery_raw) if int(stage_exit) == 0 else None
staged_config = load_json(delivery_config)
delivery_metadata = staged_config.get("metadata", {}) if isinstance(staged_config, dict) else {}
candidate_root = pathlib.Path(candidate_path)
captured_manifest_path = report_path.with_name(report_path.name + ".delivery-manifest.json")
if captured_manifest_path.is_file():
    delivery_manifest_sha256 = hashlib.sha256(captured_manifest_path.read_bytes()).hexdigest()
    delivery_manifest_path = captured_manifest_path.name
else:
    delivery_manifest_sha256 = None
    delivery_manifest_path = None
digest = hashlib.sha256()
for path in sorted(candidate_root.rglob("*")):
    relative = path.relative_to(candidate_root).as_posix().encode("utf-8")
    if path.is_symlink():
        payload = b"symlink:" + str(path.readlink()).encode("utf-8")
    elif path.is_file():
        payload = path.read_bytes()
    else:
        continue
    digest.update(relative + b"\0" + hashlib.sha256(payload).digest())
source_passed = bool(source_report and source_report.get("passed") and int(source_exit) == 0)
delivery_passed = bool(delivery_report and delivery_report.get("passed") and int(delivery_exit) == 0 and int(stage_exit) == 0)
case_report = {
    "task_id": task_id,
    "stage": stage,
    "passed": source_passed and delivery_passed,
    "source": source_report,
    "delivery": {
        "passed": delivery_passed,
        "artifact_sha256": delivery_metadata.get("artifact_sha256"),
        "artifact_bytes": delivery_metadata.get("artifact_bytes"),
        "source_paths_excluded": delivery_metadata.get("source_paths"),
        "original_project_mounted": False,
        "cases": delivery_report.get("cases") if delivery_report else None,
        "stage_error": stage_error.read_text(encoding="utf-8", errors="replace"),
    },
}
report = {
    "schema_version": 1,
    "pair_id": pair_id,
    "session_id": session_id,
    "language": language,
    "candidate_version": candidate_version,
    "task_id": task_id,
    "stage": stage,
    "manifest_sha256": manifest_sha256,
    "schedule_sha256": schedule_sha256,
    "candidate_project_sha256": digest.hexdigest(),
    "delivery_artifact_path": report_path.name + ".delivery",
    "delivery_artifact_sha256": case_report["delivery"]["artifact_sha256"],
    "delivery_manifest_path": delivery_manifest_path,
    "delivery_manifest_sha256": delivery_manifest_sha256,
    "container": {
        "image": image,
        "image_id": image_id,
        "docker_engine_id": docker_engine_id,
        "package_archive_sha256": package_sha,
        "python_base_digest": base_digest,
        "shared_dependencies_sha256": dependencies_sha,
        "uv_version": uv_version,
        "candidate_python_version": python_version,
        "platform": "linux/arm64",
        "network": "none",
        "read_only_rootfs": True,
        "candidate_uid": 10001,
        "limits": {"cpus": 2, "memory_bytes": 2147483648, "pids": 128, "tmpfs_bytes": 536870912},
        "source_project_mounted": True,
        "delivery_source_project_mounted": False,
    },
    "container_exit_codes": {"source": int(source_exit), "delivery": int(delivery_exit), "delivery_stage": int(stage_exit)},
    "launcher_stderr": {
        "source": source_error.read_text(encoding="utf-8", errors="replace"),
        "delivery": delivery_error.read_text(encoding="utf-8", errors="replace"),
    },
    "case_report": case_report,
}
temporary = report_path.with_name(report_path.name + ".tmp")
temporary.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
temporary.replace(report_path)
summary = {"language": language, "task_id": task_id, "stage": stage, "passed": case_report["passed"], "report": str(report_path), "candidate_project_sha256": digest.hexdigest(), "delivery_artifact_sha256": case_report["delivery"]["artifact_sha256"], "container_image_id": image_id, "docker_engine_id": docker_engine_id}
print(json.dumps(summary, ensure_ascii=False, sort_keys=True))
if not summary["passed"]:
    raise SystemExit(1)
PY
