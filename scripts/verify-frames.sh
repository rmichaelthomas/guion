#!/usr/bin/env bash
# Verifies the format-63 array kernels and the `frames` package end to end and writes a pass/fail
# table to stdout and frames-verification.md. Runs entirely offline.
set -u
cd "$(dirname "$0")/.."
out=frames-verification.md
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
    record "$name" PASS "$(grep -E '^test result' "$log" | awk '{p+=$4; f+=$6} END {print p " passed, " f " failed"}')"
  else
    record "$name" FAIL "see $log"
  fi
}

step "formatting" cargo fmt --all -- --check
step "release binary builds" cargo build --release --locked --offline -p guion-cli
step "model: format-63 gating" cargo test --locked --offline -p guion-model --test arrays
step "kernels: take, factorize, by-group, join" cargo test --locked --offline -p guion-cli --test frames_kernels
step "array suites still pass" cargo test --locked --offline -p guion-cli --test arrays_core --test arrays_reduce --test arrays_edges --test arrays_elementwise
step "self-hosted compiler parity" cargo test --locked --offline -p guion-cli --test arrays_self_host
step "frames: package check and own tests" cargo test --locked --offline -p guion-cli --test frames_package
step "frames: CSV, JSONL and display" cargo test --locked --offline -p guion-cli --test frames_io
step "frames: sort and group-by" cargo test --locked --offline -p guion-cli --test frames_group
step "frames: joins" cargo test --locked --offline -p guion-cli --test frames_join
step "shared catalog, both compilers" cargo test --locked --offline -p guion-cli --test compiler_conformance
step "builtin catalog is valid" cargo xtask catalog --check
step "agent reference is current" cargo xtask agent-reference --check
step "three-stage bootstrap" cargo xtask bootstrap --stages 3
step "conformance (stage2, stage3)" cargo xtask conformance --compilers stage2,stage3
step "clippy, warnings denied" cargo clippy --workspace --all-targets --locked --offline -- -D warnings
step "full workspace tests (includes old-format fixtures)" cargo test --workspace --locked --offline --no-fail-fast

{
  echo "# Data frames verification"
  echo
  echo "Generated $(date -u +%Y-%m-%dT%H:%M:%SZ) at $(git rev-parse --short HEAD)."
  echo
  echo "| Check | Result | Detail |"
  echo "|---|---|---|"
  printf '%s\n' "${rows[@]}"
} | tee "$out"

exit $failed
