#!/usr/bin/env bash
set -euo pipefail

if [[ "${GUION_FEED_DIGEST_CONTAINER:-}" != "1" ]]; then
  REPO_ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
  TARGET_ARCH="${GUION_FEED_DIGEST_ARCH:-x86_64}"
  case "$TARGET_ARCH" in
    x86_64)
      DOCKER_PLATFORM=linux/amd64
      EXPECTED_UNAME=x86_64
      ROSETTA_USED=1
      ;;
    aarch64)
      DOCKER_PLATFORM=linux/arm64
      EXPECTED_UNAME=aarch64
      ROSETTA_USED=0
      ;;
    *)
      echo "Unsupported Debian Feed Digest architecture: $TARGET_ARCH" >&2
      exit 2
      ;;
  esac
  SCRATCH_ROOT="$REPO_ROOT/target/perf-session/guion-feed-digest-debian-$TARGET_ARCH"
  BUILDER_IMAGE="guion-feed-digest-debian-$TARGET_ARCH:rust-1.97-cmake"
  IMAGE_CONTEXT="$SCRATCH_ROOT/image-context"
  BUILDX_CONFIG="$SCRATCH_ROOT/buildx-config"
  CARGO_CACHE_ROOT="${CARGO_HOME:-${HOME}/.cargo}"
  CARGO_REGISTRY="$CARGO_CACHE_ROOT/registry"
  if [[ ! -d "$CARGO_REGISTRY" ]]; then
    echo "Warmed Cargo registry cache missing: $CARGO_REGISTRY" >&2
    exit 2
  fi
  if ! colima status >/dev/null 2>&1; then
    echo "The existing Colima VM must already be running; this runner does not start one" >&2
    exit 2
  fi
  COLIMA_CONFIG="${HOME}/.colima/default/colima.yaml"
  if [[ ! -f "$COLIMA_CONFIG" ]] \
    || ! grep -Eq '^vmType:[[:space:]]*vz$' "$COLIMA_CONFIG"; then
    echo "Expected the existing default Colima VM to use macOS Virtualization.Framework" >&2
    exit 2
  fi
  if [[ "$ROSETTA_USED" == 1 ]] \
    && ! grep -Eq '^rosetta:[[:space:]]*true$' "$COLIMA_CONFIG"; then
    echo "The x86_64 target requires Rosetta enabled in the existing Colima VM" >&2
    exit 2
  fi
  DOCKER_ENDPOINT="$(docker context inspect "$(docker context show)" --format '{{(index .Endpoints "docker").Host}}')"
  if [[ "$DOCKER_ENDPOINT" != *"/.colima/default/docker.sock" ]]; then
    echo "Expected Docker context to target the existing Colima default VM, found $DOCKER_ENDPOINT" >&2
    exit 2
  fi
  if [[ "$TARGET_ARCH" == "aarch64" ]] \
    && [[ "$(docker info --format '{{.Architecture}}')" != "aarch64" ]]; then
    echo "The existing Colima Docker server is not native aarch64" >&2
    exit 2
  fi
  if [[ -n "$(docker ps -q)" ]]; then
    echo "Stop existing containers before this target run; none are stopped by the runner" >&2
    exit 2
  fi

  mkdir -p "$SCRATCH_ROOT/managed/python"
  if [[ "$TARGET_ARCH" == "aarch64" ]]; then
    SHARED_PYTHON_CACHE="$REPO_ROOT/target/perf-session/guion-feed-digest-debian-x86_64/managed/cache"
    if [[ ! -d "$SHARED_PYTHON_CACHE" ]]; then
      echo "Shared pure-Python dependency cache missing: $SHARED_PYTHON_CACHE" >&2
      exit 2
    fi
    mkdir -p "$SCRATCH_ROOT/managed"
    if [[ -e "$SCRATCH_ROOT/managed/cache" && ! -L "$SCRATCH_ROOT/managed/cache" ]]; then
      rmdir "$SCRATCH_ROOT/managed/cache"
    fi
    if [[ ! -L "$SCRATCH_ROOT/managed/cache" ]]; then
      ln -s ../../guion-feed-digest-debian-x86_64/managed/cache \
        "$SCRATCH_ROOT/managed/cache"
    fi
  else
    mkdir -p "$SCRATCH_ROOT/managed/cache"
  fi
  mkdir -p "$BUILDX_CONFIG"
  export BUILDX_CONFIG
  LOG_PATH="$SCRATCH_ROOT/run.log"
  mkdir -p "$IMAGE_CONTEXT"
  cp "$REPO_ROOT/evaluation/completion/performance/Dockerfile.feed-digest-debian" \
    "$IMAGE_CONTEXT/Dockerfile"
  docker build \
    --platform "$DOCKER_PLATFORM" \
    --tag "$BUILDER_IMAGE" \
    --file "$IMAGE_CONTEXT/Dockerfile" \
    "$IMAGE_CONTEXT" \
    2>&1 | tee "$LOG_PATH"
  docker run --rm \
    --platform "$DOCKER_PLATFORM" \
    --cpus 1 \
    --memory 1g \
    --env "GUION_FEED_DIGEST_EXPECTED_UNAME=$EXPECTED_UNAME" \
    "$BUILDER_IMAGE" \
    bash -c '
      set -e
      test "$(uname -m)" = "$GUION_FEED_DIGEST_EXPECTED_UNAME"
      . /etc/os-release
      test "$ID" = debian
      test "$VERSION_ID" = 12
      case "$(rustc --version)" in "rustc 1.97."*) ;; *) exit 2 ;; esac
      cmake --version
      echo "${GUION_FEED_DIGEST_EXPECTED_UNAME} preflight: Debian $VERSION_ID; $(rustc --version)"
    ' \
    2>&1 | tee "$LOG_PATH"
  docker run --rm \
    --platform "$DOCKER_PLATFORM" \
    --cpus 4 \
    --memory 6g \
    --user "$(id -u):$(id -g)" \
    --mount "type=bind,source=$REPO_ROOT,target=/workspace" \
    --mount "type=bind,source=$CARGO_REGISTRY,target=/cargo-registry,readonly" \
    --env GUION_FEED_DIGEST_CONTAINER=1 \
    --env "GUION_FEED_DIGEST_ARCH=$TARGET_ARCH" \
    --env "GUION_FEED_DIGEST_BUILDER_IMAGE=$BUILDER_IMAGE" \
    --workdir /workspace \
    "$BUILDER_IMAGE" \
    bash /workspace/evaluation/completion/performance/run_feed_digest_debian.sh \
    2>&1 | tee -a "$LOG_PATH"
  rm -rf "$IMAGE_CONTEXT" "$BUILDX_CONFIG"
  exit
fi

REPO_ROOT=/workspace
TARGET_ARCH="${GUION_FEED_DIGEST_ARCH:-x86_64}"
ROSETTA_ARGS=()
case "$TARGET_ARCH" in
  x86_64)
    DOCKER_PLATFORM=linux/amd64
    EXPECTED_UNAME=x86_64
    ROSETTA_USED=1
    ROSETTA_ARGS+=(--rosetta-used)
    ;;
  aarch64)
    DOCKER_PLATFORM=linux/arm64
    EXPECTED_UNAME=aarch64
    ROSETTA_USED=0
    ;;
  *)
    echo "Unsupported Debian Feed Digest architecture: $TARGET_ARCH" >&2
    exit 2
    ;;
esac
SCRATCH_ROOT="$REPO_ROOT/target/perf-session/guion-feed-digest-debian-$TARGET_ARCH"
BUILDER_IMAGE="${GUION_FEED_DIGEST_BUILDER_IMAGE:-guion-feed-digest-debian-$TARGET_ARCH:rust-1.97-cmake}"
WARMED="$SCRATCH_ROOT/managed"
CARGO_HOME_DIR="$SCRATCH_ROOT/cargo-home"
CARGO_TARGET_DIR="$SCRATCH_ROOT/cargo-target"
WARM_PROJECT="$SCRATCH_ROOT/warm-project"
RUN_SCRATCH="$SCRATCH_ROOT/package-run"
OUTPUT_STEM="$REPO_ROOT/evaluation/completion/results/real-task-sprint/2026-10-28-feed-digest-managed-package-debian-$TARGET_ARCH"

cleanup() {
  local status=$?
  trap - EXIT
  rm -rf "$WARM_PROJECT" "$RUN_SCRATCH" "$CARGO_HOME_DIR" "$SCRATCH_ROOT/xdg-config"
  if [[ "$status" -eq 0 ]]; then
    rm -rf "$CARGO_TARGET_DIR"
  else
    echo "Temporary project/package scratch removed." >&2
    echo "Cargo incremental output, log, and managed cache retained under $SCRATCH_ROOT" >&2
  fi
  exit "$status"
}
trap cleanup EXIT

if [[ "$(uname -m)" != "$EXPECTED_UNAME" ]]; then
  echo "Expected Debian $EXPECTED_UNAME container, found $(uname -m)" >&2
  exit 2
fi
if ! grep -q '^ID=debian$' /etc/os-release || ! grep -q '^VERSION_ID="12"$' /etc/os-release; then
  echo "Expected Debian 12 in the $EXPECTED_UNAME package container" >&2
  exit 2
fi
if ! command -v curl >/dev/null 2>&1; then
  echo "The pinned Rust builder image must provide curl for the pinned uv installer" >&2
  exit 2
fi
RUST_VERSION="$(rustc --version | awk '{print $2}')"
if [[ "$RUST_VERSION" != 1.97.* ]]; then
  echo "Expected Rust 1.97.x, found $RUST_VERSION" >&2
  exit 2
fi
CMAKE_VERSION="$(cmake --version | awk 'NR == 1 {print $3}')"

mkdir -p \
  "$WARMED/toolchain" "$WARMED/python" \
  "$WARM_PROJECT" "$CARGO_HOME_DIR" "$SCRATCH_ROOT/xdg-config"
if [[ ! -d "$WARMED/cache" ]]; then
  echo "Warmed dependency cache missing: $WARMED/cache" >&2
  exit 2
fi
ln -sfn /cargo-registry "$CARGO_HOME_DIR/registry"
export CARGO_HOME="$CARGO_HOME_DIR"
export CARGO_TARGET_DIR
export RUSTUP_HOME=/usr/local/rustup
export PATH=/usr/local/cargo/bin:"$PATH"
export UV_CACHE_DIR="$WARMED/cache"
export UV_PYTHON_INSTALL_DIR="$WARMED/python"
export XDG_CONFIG_HOME="$SCRATCH_ROOT/xdg-config"
export UV_NO_PROGRESS=1
export PYTHONDONTWRITEBYTECODE=1

UV="$WARMED/toolchain/uv"
UV_VERSION=""
if [[ -x "$UV" ]]; then
  UV_VERSION="$("$UV" --version | awk '{print $2}')"
fi
if [[ "$UV_VERSION" != "0.13.4" ]]; then
  curl -LsSf https://astral.sh/uv/0.13.4/install.sh \
    | env UV_INSTALL_DIR="$WARMED/toolchain" UV_NO_MODIFY_PATH=1 sh
fi
if [[ "$("$UV" --version | awk '{print $2}')" != "0.13.4" ]]; then
  echo "Expected pinned uv 0.13.4" >&2
  exit 2
fi

"$UV" python install 3.14.8 --install-dir "$WARMED/python" --no-bin
cat > "$WARM_PROJECT/pyproject.toml" <<'TOML'
[project]
name = "guion-feed-digest-warmup"
version = "0.1.0"
requires-python = "==3.14.8"
dependencies = ["feedparser==6.0.14"]
TOML
"$UV" lock --project "$WARM_PROJECT" --python 3.14.8 --managed-python
"$UV" sync --project "$WARM_PROJECT" --locked --python 3.14.8 --managed-python
"$UV" sync --project "$WARM_PROJECT" --locked --python 3.14.8 --managed-python --offline
"$UV" run --project "$WARM_PROJECT" --python 3.14.8 --managed-python --offline --no-sync \
  python3 -B - <<'PY'
import feedparser
from importlib.metadata import version

assert feedparser.__version__ == "6.0.14"
assert version("feedparser-sgmllib") == "2.1.0"
print(
    "feedparser",
    version("feedparser"),
    "feedparser-sgmllib",
    version("feedparser-sgmllib"),
    "preloaded",
)
PY

cargo build --locked --offline --jobs 2 --release -p guion-cli --features python-interop
GUION="$CARGO_TARGET_DIR/release/guion"
"$GUION" doctor --json

PYTHON="$($UV python find 3.14.8 --managed-python)"
"$PYTHON" -B \
  /workspace/evaluation/completion/performance/verify_managed_feed_digest_package.py \
  --binary "$GUION" \
  --uv "$UV" \
  --warmed-dir "$WARMED" \
  --scratch-dir "$RUN_SCRATCH" \
  --output-stem "$OUTPUT_STEM" \
  --expected-os linux \
  --expected-arch "$EXPECTED_UNAME" \
  --expected-distribution debian \
  --expected-distribution-version 12 \
  --colima-vm-used \
  "${ROSETTA_ARGS[@]}" \
  --builder-image "$BUILDER_IMAGE" \
  --builder-rust-version "$RUST_VERSION" \
  --builder-cmake-version "$CMAKE_VERSION"

echo "Kept the pinned $EXPECTED_UNAME managed Python cache at $WARMED for the next Linux package check."
