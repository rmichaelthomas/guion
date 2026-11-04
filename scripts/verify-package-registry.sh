#!/usr/bin/env bash
# Verifies the package system and registry (Track A) end to end on this machine. It runs the
# registry's own tests, the client tests against a locally running registry (wrangler dev --local,
# loopback only), and the old-format fixtures. It prints a pass/fail table and writes
# package-registry-verification.md. Nothing is deployed and no network beyond loopback is used.
set -u
cd "$(dirname "$0")/.."

rows=()
failed=0
record() { # name, status, detail
  rows+=("| $1 | $2 | $3 |")
  [ "$2" = PASS ] || failed=1
}
summarise() { # sums "N passed; M failed" over every `test result` line
  awk '/^test result/ {p+=$4; f+=$6} END {print p " passed, " f " failed"}'
}
run() { # label, command...
  local label="$1"; shift
  local out
  if out=$("$@" 2>&1); then
    record "$label" PASS "$(printf '%s' "$out" | summarise)"
  else
    record "$label" FAIL "$(printf '%s' "$out" | tail -3 | tr '\n' ' ')"
  fi
}

if ! command -v wrangler >/dev/null || ! command -v node >/dev/null; then
  record "tooling present (wrangler, node)" FAIL "install both to run the registry"
else
  record "tooling present (wrangler, node)" PASS "$(wrangler --version | head -1), node $(node --version)"
fi

# S4: never test a stale binary.
run "debug binary builds" cargo build -p guion-cli --locked --offline
run "registry core (node tests)" bash -c 'cd registry && node --test test/*.test.js 2>&1 | awk "/^# tests/ {t=\$3} /^# pass/ {p=\$3} /^# fail/ {f=\$3} END {print p \" passed, \" f \" failed\"; exit (f>0)}"'
run "A1 name rules" cargo test -p guion-cli --locked --offline --bin guion package_name
run "A2 versions and resolver" cargo test -p guion-cli --locked --offline --bin guion package_version
run "A3/A4/A5 library, capabilities, identities" cargo test -p guion-cli --locked --offline --test package_library
run "A4 capability declarations (unit)" cargo test -p guion-cli --locked --offline --bin guion package_capabilities
run "A5 artifact format 60 gating" cargo test -p guion-model --locked --offline package_identities
run "A6/A7 publish, add, yank against the local registry" cargo test -p guion-cli --locked --offline --test registry_end_to_end -- --nocapture
run "A7 tampered index, log and object fail closed" cargo test -p guion-cli --locked --offline --bin guion registry::
run "older locks and native packages still load" cargo test -p guion-cli --locked --offline --test native_packages
run "three-stage bootstrap" cargo xtask bootstrap --stages 3
run "conformance (stage2, stage3)" cargo xtask conformance --compilers stage2,stage3
run "clippy, warnings denied" cargo clippy --workspace --all-targets --locked --offline -- -D warnings
run "full workspace tests" cargo test --workspace --locked --offline --no-fail-fast

# A registry test that skipped itself because wrangler was missing must not count as a pass.
if cargo test -p guion-cli --locked --offline --test registry_end_to_end -- --nocapture 2>&1 | grep -q SKIPPED; then
  record "registry end-to-end ran (not skipped)" FAIL "wrangler missing"
else
  record "registry end-to-end ran (not skipped)" PASS ""
fi
# No token may appear in repository output or files created by the tests.
if grep -rIl "gpt_[0-9a-f]\{64\}" --exclude-dir=target --exclude-dir=node_modules --exclude-dir=.git . >/dev/null 2>&1; then
  record "no publisher token on disk" FAIL "a token-shaped string was found in the tree"
else
  record "no publisher token on disk" PASS ""
fi

{
  echo "# Package and registry verification"
  echo
  echo "Generated $(date -u +%Y-%m-%dT%H:%M:%SZ) at $(git rev-parse --short HEAD)."
  echo
  echo "| Check | Result | Detail |"
  echo "|---|---|---|"
  printf '%s\n' "${rows[@]}"
} | tee package-registry-verification.md
exit $failed
