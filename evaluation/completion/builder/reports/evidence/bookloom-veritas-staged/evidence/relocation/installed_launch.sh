#!/bin/bash
set +e
APP="/private/tmp/veritas-fresh-builders/bookloom-veritas-staged/installed/bin/bookloom"
DATA="/private/tmp/veritas-fresh-builders/bookloom-veritas-staged/📚 caller data"
LOG="/private/tmp/veritas-fresh-builders/bookloom-veritas-staged/evidence/relocation/installed_launch.log"
cd "$DATA" || exit 10
printf 'UTC %s\n' "$(date -u '+%Y-%m-%dT%H:%M:%SZ')" > "$LOG"
printf '\n$ cd %q && %q list checked-out\n' "$DATA" "$APP" >> "$LOG"
/usr/bin/time -p -o "/private/tmp/veritas-fresh-builders/bookloom-veritas-staged/evidence/relocation/installed_launch.time" "$APP" list checked-out >> "$LOG" 2>&1
status=$?
printf 'exit=%s\n' "$status" >> "$LOG"
if [ "$status" -ne 0 ]; then exit 1; fi
printf '\n$ cd %q && %q due-soon --as-of 2026-10-26\n' "$DATA" "$APP" >> "$LOG"
"$APP" due-soon --as-of 2026-10-26 >> "$LOG" 2>&1
status=$?
printf 'exit=%s\n' "$status" >> "$LOG"
if [ "$status" -ne 0 ]; then exit 1; fi
before=$(shasum -a 256 "$DATA/bookloom.sqlite")
printf '\n$ cd %q && %q due-soon --as-of 2026-02-30\n' "$DATA" "$APP" >> "$LOG"
"$APP" due-soon --as-of 2026-02-30 >> "$LOG" 2>&1
status=$?
after=$(shasum -a 256 "$DATA/bookloom.sqlite")
printf 'exit=%s (expected 2)\ndatabase_sha256_before=%s\ndatabase_sha256_after=%s\n' "$status" "$before" "$after" >> "$LOG"
if [ "$status" -eq 0 ] || [ "$before" != "$after" ]; then exit 1; fi
