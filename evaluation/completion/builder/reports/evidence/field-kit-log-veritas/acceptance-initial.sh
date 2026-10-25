#!/bin/sh
set -eu
cd /private/tmp/veritas-fresh-builders/field-kit-log-veritas
export VERITAS_FILE_ROOT="$PWD/data"
run() { package-v1/run operator.toml "$@"; }
run add pump-2 'Portable pump' > evidence/add.log
run list > evidence/restart-list.tsv
cmp evidence/initial-list.tsv evidence/restart-list.tsv
awk -F '\t' -v day="$(date '+%Y-%m-%d')" 'NF != 5 || $3 != day { exit 1 } END { if (NR != 2) exit 1 }' evidence/restart-list.tsv
shasum -a 256 data/field-kit.sqlite > evidence/db-before-errors.sha256
reject() {
  name=$1
  shift
  set +e
  run "$@" > "evidence/$name.stdout" 2> "evidence/$name.stderr"
  code=$?
  set -e
  printf '%s exit=%s\n' "$name" "$code" >> evidence/error-exits.log
  test "$code" -ne 0
  shasum -a 256 data/field-kit.sqlite > "evidence/$name.sha256"
  cmp evidence/db-before-errors.sha256 "evidence/$name.sha256"
}
reject duplicate add meter-1 'Replacement name'
reject unknown inspect missing-item pass 'Unknown test'
reject invalid inspect meter-1 maybe 'Invalid test'
reject empty-note inspect meter-1 pass ''
reject wrong-arity inspect meter-1 pass
run list > evidence/after-errors.tsv
cmp evidence/restart-list.tsv evidence/after-errors.tsv
printf 'PASS item creation, pass/fail records, automatic local dates, restart persistence, stable order, rejection exit statuses, byte-identical database after rejected operations\n'
