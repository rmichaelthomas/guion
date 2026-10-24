#!/bin/sh
set -eu

if [ "$#" -ne 1 ]; then
    printf 'Usage: %s PATH_TO_VERITAS_LINUX_ARM64_ARCHIVE\n' "$0" >&2
    exit 2
fi

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd -P)
archive=$(CDPATH= cd -- "$(dirname -- "$1")" && pwd -P)/$(basename -- "$1")
campaign_lock="$repo_root/evaluation/sandbox/campaign-lock.json"
dependencies="$repo_root/evaluation/sandbox/dependencies/campaign-dependencies.tar.gz"
expected_sha=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["veritas_archive"]["sha256"])' "$campaign_lock")
expected_dependencies_sha=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["dependency_bundle"]["sha256"])' "$campaign_lock")
base_reference=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["base_image"]["reference"])' "$campaign_lock")
base_digest=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["base_image"]["sha256"])' "$campaign_lock")
campaign_python=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["python_version"])' "$campaign_lock")
campaign_uv=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["uv_version"])' "$campaign_lock")
case "$expected_sha" in
    *[!0-9a-f]*|'') printf 'Expected SHA-256 must be 64 lowercase hexadecimal characters\n' >&2; exit 2 ;;
esac
if [ "${#expected_sha}" -ne 64 ]; then
    printf 'Expected SHA-256 must be 64 lowercase hexadecimal characters\n' >&2
    exit 2
fi
actual_sha=$(shasum -a 256 "$archive" | awk '{print $1}')
if [ "$actual_sha" != "$expected_sha" ]; then
    printf 'Unexpected Veritas archive SHA-256: %s\nExpected: %s\n' "$actual_sha" "$expected_sha" >&2
    exit 1
fi
actual_dependencies_sha=$(shasum -a 256 "$dependencies" | awk '{print $1}')
if [ "$actual_dependencies_sha" != "$expected_dependencies_sha" ]; then
    printf 'Unexpected shared dependency archive SHA-256: %s\nExpected: %s\n' "$actual_dependencies_sha" "$expected_dependencies_sha" >&2
    exit 1
fi
if ! grep -Fq "ARG PYTHON_BASE=$base_reference@sha256:$base_digest" "$repo_root/evaluation/sandbox/Dockerfile"; then
    printf 'Dockerfile base image does not match campaign-lock.json\n' >&2
    exit 1
fi

context=$(mktemp -d "${TMPDIR:-/tmp}/veritas-sandbox-image.XXXXXX")
trap 'rm -rf "$context"' EXIT HUP INT TERM
cp "$repo_root/evaluation/sandbox/Dockerfile" "$context/Dockerfile"
cp "$repo_root/evaluation/protocol/case_runner.py" "$context/case_runner.py"
cp "$repo_root/evaluation/sandbox/candidate-python.json" "$context/candidate-python.json"
cp "$repo_root/evaluation/sandbox/candidate-veritas.json" "$context/candidate-veritas.json"
cp "$repo_root/evaluation/fixtures/independent-v1/manifest.json" "$context/manifest.json"
cp "$archive" "$context/veritas-linux-aarch64.tar.gz"
cp "$dependencies" "$context/campaign-dependencies.tar.gz"
uv_path=veritas-0.1.0-linux-aarch64/examples/python_interop_bundle/toolchains/uv
tar -xOzf "$archive" "$uv_path" > "$context/uv"
chmod 0555 "$context/uv"
docker buildx build --load --platform linux/arm64 --target python-runner \
    --tag "veritas-campaign-python:$campaign_python" \
    --label org.opencontainers.image.source=veritas-independent-workloads-v1 \
    --label "org.veritas.package.sha256=$actual_sha" \
    --label "org.veritas.campaign.dependencies.sha256=$actual_dependencies_sha" \
    --label "org.veritas.campaign.uv.version=$campaign_uv" \
    --label "org.veritas.python.base.digest=sha256:$base_digest" \
    --label "org.veritas.campaign.python.version=$campaign_python" \
    "$context"
docker buildx build --load --platform linux/arm64 --target veritas-runner \
    --tag veritas-campaign-veritas:0.1.0 \
    --label org.opencontainers.image.source=veritas-independent-workloads-v1 \
    --label "org.veritas.package.sha256=$actual_sha" \
    --label "org.veritas.campaign.dependencies.sha256=$actual_dependencies_sha" \
    --label "org.veritas.campaign.uv.version=$campaign_uv" \
    --label "org.veritas.python.base.digest=sha256:$base_digest" \
    --label "org.veritas.campaign.python.version=$campaign_python" \
    "$context"
for image in "veritas-campaign-python:$campaign_python" veritas-campaign-veritas:0.1.0; do
    docker run --rm --entrypoint sh "$image" -ec '
        test ! -e /opt/veritas/evaluation
        test ! -e /opt/veritas/examples
        test ! -e /opt/veritas/language
        test ! -e /opt/veritas/docs
    '
done
printf 'Built veritas-campaign-python:%s and veritas-campaign-veritas:0.1.0\n' "$campaign_python"
