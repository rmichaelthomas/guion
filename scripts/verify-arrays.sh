#!/usr/bin/env bash
# Verifies native arrays (artifact format 62) end to end and writes a pass/fail table to stdout and
# arrays-verification.md. Runs entirely offline.
set -u
cd "$(dirname "$0")/.."
out=arrays-verification.md
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

step "release binary builds" cargo build --release --locked --offline -p guion-cli
step "model: element types, codes, format-62 gating" cargo test --locked --offline -p guion-model --test arrays
step "Rust compiler: array typing and diagnostics" cargo test --locked --offline -p guion-compiler --test arrays_typing
step "catalog matches the model operand table" cargo test --locked --offline -p guion-compiler --lib array_catalog_matches_the_model_operand_table
step "construction, access, shape, bytes" cargo test --locked --offline -p guion-cli --test arrays_core
step "elementwise arithmetic, comparisons, math" cargo test --locked --offline -p guion-cli --test arrays_elementwise
step "reductions, sorting, linear algebra, random" cargo test --locked --offline -p guion-cli --test arrays_reduce
step "self-hosted compiler parity" cargo test --locked --offline -p guion-cli --test arrays_self_host
step "machine code on and off" cargo test --locked --offline -p guion-runtime --test arrays_native
step "shared catalog, both compilers" cargo test --locked --offline -p guion-cli --test compiler_conformance
step "builtin catalog is valid" cargo xtask catalog --check
step "agent reference is current" cargo xtask agent-reference --check
step "three-stage bootstrap" cargo xtask bootstrap --stages 3
step "conformance (stage2, stage3)" cargo xtask conformance --compilers stage2,stage3
step "clippy, warnings denied" cargo clippy --workspace --all-targets --locked --offline -- -D warnings
step "full workspace tests (includes old-format fixtures)" cargo test --workspace --locked --offline --no-fail-fast

{
  echo "# Native arrays verification"
  echo
  echo "Generated $(date -u +%Y-%m-%dT%H:%M:%SZ) at $(git rev-parse --short HEAD)."
  echo
  echo "| Check | Result | Detail |"
  echo "|---|---|---|"
  printf '%s\n' "${rows[@]}"
} | tee "$out"

exit $failed
