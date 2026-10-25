#!/bin/sh
set -eu
cd /private/tmp/veritas-fresh-builders/field-kit-log-veritas
export VERITAS_FILE_ROOT="$PWD/data"
run() { './Field Kit – café/bin/field-kit-log' operator.toml "$@"; }
run list > evidence/relocated-all.tsv
run list --failed-only > evidence/relocated-failed.tsv
cmp evidence/followup-all.tsv evidence/relocated-all.tsv
cmp evidence/followup-failed.tsv evidence/relocated-failed.tsv
shasum -a 256 data/field-kit.sqlite > evidence/db-after-relocation.sha256
cmp evidence/db-before-errors.sha256 evidence/db-after-relocation.sha256
TZ=Pacific/Kiritimati date '+%Y-%m-%d' > evidence/timezone-expected.txt
TZ=Pacific/Kiritimati run inspect pump-2 fail 'Timezone and relocated write check' > evidence/relocated-write.log
run list > evidence/relocated-restart.tsv
awk -F '\t' -v expected="$(cat evidence/timezone-expected.txt)" 'NR == 3 && ($2 != "pump-2" || $3 != expected || $4 != "fail") { exit 1 } END { if (NR != 3) exit 1 }' evidence/relocated-restart.tsv
run list --failed-only > evidence/relocated-restart-failed.tsv
awk -F '\t' '$4 != "fail" { exit 1 } END { if (NR != 2) exit 1 }' evidence/relocated-restart-failed.tsv
test -f data/field-kit.sqlite
test ! -e './Field Kit – café/share/veritas/apps/field-kit-log/0.1.0/field-kit.sqlite'
printf 'PASS relocated installed app with spaces and Unicode; same separate data directory; lists unchanged; local timezone date (Pacific/Kiritimati); new write survives restart; filter includes new failure\n'
