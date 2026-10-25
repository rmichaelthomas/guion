#!/bin/bash
set +e
VERITAS="/Users/rmichaelthomas/Documents/Codex/2026-10-22/referenced-chatgpt-conversation-this-is-an/work/veritas-foundation/target/completion/releases/macos-aarch64/current/bin/veritas"
PROJECT="/private/tmp/veritas-fresh-builders/bookloom-veritas-staged/project"
DATA="/private/tmp/veritas-fresh-builders/bookloom-veritas-staged/followup-due-data"
LOG="/private/tmp/veritas-fresh-builders/bookloom-veritas-staged/evidence/followup/due_soon.log"
run_ok() {
  printf '\n$ veritas run --file-root %s %s' "$DATA" "$PROJECT" >> "$LOG"
  printf ' %q' "$@" >> "$LOG"
  printf '\n' >> "$LOG"
  "$VERITAS" run --file-root "$DATA" "$PROJECT" "$@" >> "$LOG" 2>&1
  status=$?
  printf 'exit=%s\n' "$status" >> "$LOG"
  if [ "$status" -ne 0 ]; then return 1; fi
}
printf 'UTC %s\n' "$(date -u '+%Y-%m-%dT%H:%M:%SZ')" > "$LOG"
run_ok add B001 "A Book" || exit 1
run_ok add B002 "Another Book" || exit 1
run_ok add B003 "Overdue Book" || exit 1
run_ok checkout B002 "Reader Two" 2026-10-25 || exit 1
run_ok checkout B001 "Reader One" 2026-10-25 || exit 1
run_ok checkout B003 "Reader Three" 2026-10-01 || exit 1
run_ok due-soon --as-of 2026-10-25 || exit 1
run_ok due-soon --as-of 2026-11-02 || exit 1
run_ok due-soon --as-of 2026-11-09 || exit 1
