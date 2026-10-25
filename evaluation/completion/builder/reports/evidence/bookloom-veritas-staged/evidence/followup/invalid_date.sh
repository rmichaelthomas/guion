#!/bin/bash
VERITAS="/Users/rmichaelthomas/Documents/Codex/2026-10-22/referenced-chatgpt-conversation-this-is-an/work/veritas-foundation/target/completion/releases/macos-aarch64/current/bin/veritas"
PROJECT="/private/tmp/veritas-fresh-builders/bookloom-veritas-staged/project"
DATA="/private/tmp/veritas-fresh-builders/bookloom-veritas-staged/followup-due-data"
DB="$DATA/bookloom.sqlite"
LOG="/private/tmp/veritas-fresh-builders/bookloom-veritas-staged/evidence/followup/invalid_date.log"
set +e
printf 'UTC %s\n' "$(date -u '+%Y-%m-%dT%H:%M:%SZ')" > "$LOG"
before=$(shasum -a 256 "$DB")
printf '\n$ veritas run --file-root %s %s due-soon --as-of 2026-02-30\n' "$DATA" "$PROJECT" >> "$LOG"
"$VERITAS" run --file-root "$DATA" "$PROJECT" due-soon --as-of 2026-02-30 >> "$LOG" 2>&1
status=$?
printf 'exit=%s (expected 2)\n' "$status" >> "$LOG"
after=$(shasum -a 256 "$DB")
printf 'database_sha256_before=%s\ndatabase_sha256_after=%s\n' "$before" "$after" >> "$LOG"
if [ "$status" -eq 0 ] || [ "$before" != "$after" ]; then exit 1; fi
printf '\n$ veritas run --file-root %s %s list checked-out\n' "$DATA" "$PROJECT" >> "$LOG"
"$VERITAS" run --file-root "$DATA" "$PROJECT" list checked-out >> "$LOG" 2>&1
status=$?
printf 'exit=%s\n' "$status" >> "$LOG"
if [ "$status" -ne 0 ]; then exit 1; fi
