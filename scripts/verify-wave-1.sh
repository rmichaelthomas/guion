#!/usr/bin/env bash
# Verifies stdlib Wave 1 (stages B1 to B8) end to end and writes a pass/fail table to stdout and
# wave-1-verification.md. Runs entirely offline against loopback fixtures.
set -u
cd "$(dirname "$0")/.."
out=wave-1-verification.md
rows=()
failed=0

record() { # name status detail
  rows+=("| $1 | $2 | $3 |")
  [ "$2" = "PASS" ] || failed=1
}

step() { # name command...
  local name=$1; shift
  local log
  log=$(mktemp)
  if "$@" >"$log" 2>&1; then
    record "$name" PASS "$(grep -E '^test result' "$log" | tail -1 | cut -c1-80)"
  else
    record "$name" FAIL "see $log"
  fi
}

step "release binary builds" cargo build --release --locked --offline -p guion-cli
step "B1 math, conversions, random" cargo test --locked --offline -p guion-cli --test stdlib_math_random
step "B2 encodings and URLs" cargo test --locked --offline -p guion-cli --test stdlib_encoding_url
step "B3 paths, filesystem, temp files" cargo test --locked --offline -p guion-cli --test stdlib_fs_path
step "B3 filesystem grant denial (runtime)" cargo test --locked --offline -p guion-runtime --test execution filesystem_functions_are_denied_without_a_policy_grant_and_confined_with_one
step "B4 regex" cargo test --locked --offline -p guion-cli --test stdlib_regex
step "B5 processes and signals (default-deny, limits)" cargo test --locked --offline -p guion-cli --test stdlib_process
step "B6 TCP and UDP (default-deny, limits)" cargo test --locked --offline -p guion-cli --test stdlib_net
step "B7 TLS (verification, rejection, grants)" cargo test --locked --offline -p guion-cli --test stdlib_tls
step "B8 HTTP server, WebSockets, SHA-1" cargo test --locked --offline -p guion-cli --test stdlib_http_server
step "artifact format gating (formats 52 to 59)" cargo test --locked --offline -p guion-model
step "three-stage bootstrap" cargo xtask bootstrap --stages 3
step "conformance (stage2, stage3)" cargo xtask conformance --compilers stage2,stage3
step "clippy, warnings denied" cargo clippy --workspace --all-targets --locked --offline -- -D warnings
step "full workspace tests (includes old-format fixtures)" cargo test --workspace --locked --offline --no-fail-fast

{
  echo "# Wave 1 verification"
  echo
  echo "Generated $(date -u +%Y-%m-%dT%H:%M:%SZ) at $(git rev-parse --short HEAD)."
  echo
  echo "| Check | Result | Detail |"
  echo "|---|---|---|"
  printf '%s\n' "${rows[@]}"
} | tee "$out"

exit $failed
