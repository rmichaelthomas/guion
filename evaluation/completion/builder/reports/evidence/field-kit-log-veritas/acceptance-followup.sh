#!/bin/sh
set -eu
cd /private/tmp/veritas-fresh-builders/field-kit-log-veritas
export VERITAS_FILE_ROOT="$PWD/data"
run() { package-final/run operator.toml "$@"; }
run list > evidence/followup-all.tsv
run list --failed-only > evidence/followup-failed.tsv
cmp evidence/initial-list.tsv evidence/followup-all.tsv
awk -F '\t' 'NF != 5 || $4 != "fail" || $1 != "2" { exit 1 } END { if (NR != 1) exit 1 }' evidence/followup-failed.tsv
shasum -a 256 data/field-kit.sqlite > evidence/db-after-followup.sha256
cmp evidence/db-before-errors.sha256 evidence/db-after-followup.sha256
set +e
run list --unknown > evidence/unknown-filter.stdout 2> evidence/unknown-filter.stderr
code=$?
set -e
test "$code" -eq 2
shasum -a 256 data/field-kit.sqlite > evidence/db-after-unknown-filter.sha256
cmp evidence/db-before-errors.sha256 evidence/db-after-unknown-filter.sha256
printf 'PASS old unfiltered list unchanged, failed-only selects one fail, unknown flag rejected, database unchanged\n'
