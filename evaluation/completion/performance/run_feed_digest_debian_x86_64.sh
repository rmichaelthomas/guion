#!/usr/bin/env bash
set -euo pipefail

if [[ "${GUION_FEED_DIGEST_CONTAINER:-}" != "1" ]]; then
  REPO_ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
  SCRATCH_ROOT="$REPO_ROOT/target/perf-session/guion-feed-digest-debian-x86_64"
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
    || ! grep -Eq '^vmType:[[:space:]]*vz$' "$COLIMA_CONFIG" \
    || ! grep -Eq '^rosetta:[[:space:]]*true$' "$COLIMA_CONFIG"; then
    echo "Expected the existing default Colima VM to use vz with Rosetta enabled" >&2
    exit 2
  fi
  DOCKER_ENDPOINT="$(docker context inspect "$(docker context show)" --format '{{(index .Endpoints "docker").Host}}')"
  if [[ "$DOCKER_ENDPOINT" != *"/.colima/default/docker.sock" ]]; then
    echo "Expected Docker context to target the existing Colima default VM, found $DOCKER_ENDPOINT" >&2
    exit 2
  fi
  if [[ -n "$(docker ps -q)" ]]; then
    echo "Stop existing containers before this target run; none are stopped by the runner" >&2
    exit 2
  fi

  mkdir -p "$SCRATCH_ROOT/managed/cache" "$SCRATCH_ROOT/managed/python"
  LOG_PATH="$SCRATCH_ROOT/run.log"
  docker run --rm \
    --platform linux/amd64 \
    --cpus 1 \
    --memory 1g \
    rust:1.97-bookworm \
    bash -c 'test "$(uname -m)" = "x86_64"; . /etc/os-release; test "$ID" = debian; test "$VERSION_ID" = 12; case "$(rustc --version)" in "rustc 1.97."*) ;; *) exit 2 ;; esac; echo "amd64 preflight: Debian $VERSION_ID x86_64; $(rustc --version)"' \
    2>&1 | tee "$LOG_PATH"
  docker run --rm \
    --platform linux/amd64 \
    --cpus 2 \
    --memory 6g \
    --user "$(id -u):$(id -g)" \
    --mount "type=bind,source=$REPO_ROOT,target=/workspace" \
    --mount "type=bind,source=$CARGO_REGISTRY,target=/cargo-registry,readonly" \
    --env GUION_FEED_DIGEST_CONTAINER=1 \
    --workdir /workspace \
    rust:1.97-bookworm \
    bash /workspace/evaluation/completion/performance/run_feed_digest_debian_x86_64.sh \
    2>&1 | tee -a "$LOG_PATH"
  exit
fi

REPO_ROOT=/workspace
SCRATCH_ROOT="$REPO_ROOT/target/perf-session/guion-feed-digest-debian-x86_64"
WARMED="$SCRATCH_ROOT/managed"
CARGO_HOME_DIR="$SCRATCH_ROOT/cargo-home"
CARGO_TARGET_DIR="$SCRATCH_ROOT/cargo-target"
WARM_PROJECT="$SCRATCH_ROOT/warm-project"
RUN_SCRATCH="$SCRATCH_ROOT/package-run"
OUTPUT_STEM="$REPO_ROOT/evaluation/completion/results/real-task-sprint/2026-10-28-feed-digest-managed-package-debian-x86_64"

cleanup() {
  local status=$?
  trap - EXIT
  rm -rf "$WARM_PROJECT" "$RUN_SCRATCH" "$CARGO_HOME_DIR" "$CARGO_TARGET_DIR"
  if [[ "$status" -ne 0 ]]; then
    echo "Temporary build and package scratch removed; log and reusable managed cache remain under $SCRATCH_ROOT" >&2
  fi
  exit "$status"
}
trap cleanup EXIT

if [[ "$(uname -m)" != "x86_64" ]]; then
  echo "Expected the Rosetta-backed Debian x86_64 container, found $(uname -m)" >&2
  exit 2
fi
if ! grep -q '^ID=debian$' /etc/os-release || ! grep -q '^VERSION_ID="12"$' /etc/os-release; then
  echo "Expected Debian 12 in the x86_64 package container" >&2
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

mkdir -p "$WARMED/toolchain" "$WARMED/cache" "$WARMED/python" "$WARM_PROJECT" "$CARGO_HOME_DIR"
ln -sfn /cargo-registry "$CARGO_HOME_DIR/registry"
export CARGO_HOME="$CARGO_HOME_DIR"
export CARGO_TARGET_DIR
export RUSTUP_HOME=/usr/local/rustup
export PATH=/usr/local/cargo/bin:"$PATH"
export UV_CACHE_DIR="$WARMED/cache"
export UV_PYTHON_INSTALL_DIR="$WARMED/python"
export UV_NO_PROGRESS=1
export PYTHONDONTWRITEBYTECODE=1

curl -LsSf https://astral.sh/uv/0.13.4/install.sh \
  | env UV_INSTALL_DIR="$WARMED/toolchain" UV_NO_MODIFY_PATH=1 sh
UV="$WARMED/toolchain/uv"
if [[ "$("$UV" --version | awk '{print $2}')" != "0.13.4" ]]; then
  echo "Expected pinned uv 0.13.4" >&2
  exit 2
fi

"$UV" python install 3.14.8 --install-dir "$WARMED/python"
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
  python3 -B -c 'import feedparser, sgmllib; assert feedparser.__version__ == "6.0.14"; print("feedparser", feedparser.__version__, "preloaded")'

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
  --expected-arch x86_64 \
  --expected-distribution debian \
  --expected-distribution-version 12 \
  --colima-vm-used \
  --rosetta-used \
  --builder-image rust:1.97-bookworm \
  --builder-rust-version "$RUST_VERSION"

echo "Kept the pinned x86_64 managed Python cache at $WARMED for the next Linux package check."
