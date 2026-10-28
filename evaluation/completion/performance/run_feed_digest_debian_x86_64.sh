#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
export GUION_FEED_DIGEST_ARCH=x86_64
exec bash "$SCRIPT_DIR/run_feed_digest_debian.sh" "$@"
