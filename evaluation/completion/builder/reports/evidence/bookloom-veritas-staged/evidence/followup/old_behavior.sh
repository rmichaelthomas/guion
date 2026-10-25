#!/bin/bash
set +e
VERITAS="/Users/rmichaelthomas/Documents/Codex/2026-10-22/referenced-chatgpt-conversation-this-is-an/work/veritas-foundation/target/completion/releases/macos-aarch64/current/bin/veritas"
PROJECT="/private/tmp/veritas-fresh-builders/bookloom-veritas-staged/project"
DATA="/private/tmp/veritas-fresh-builders/bookloom-veritas-staged/followup-data"
LOG="/private/tmp/veritas-fresh-builders/bookloom-veritas-staged/evidence/initial/acceptance.log"
run_ok() {
  printf '\n$ veritas run --file-root %s %s' "$DATA" "$PROJECT" >> "$LOG"
  printf ' %q' "$@" >> "$LOG"
  printf '\n' >> "$LOG"
  "$VERITAS" run --file-root "$DATA" "$PROJECT" "$@" >> "$LOG" 2>&1
  status=$?
  printf 'exit=%s\n' "$status" >> "$LOG"
  if [ "$status" -ne 0 ]; then return 1; fi
}
run_fail() {
  printf '\n$ veritas run --file-root %s %s' "$DATA" "$PROJECT" >> "$LOG"
  printf ' %q' "$@" >> "$LOG"
  printf '\n' >> "$LOG"
  "$VERITAS" run --file-root "$DATA" "$PROJECT" "$@" >> "$LOG" 2>&1
  status=$?
  printf 'exit=%s (expected nonzero)\n' "$status" >> "$LOG"
  if [ "$status" -eq 0 ]; then return 1; fi
}
printf 'UTC %s\n' "$(date -u '+%Y-%m-%dT%H:%M:%SZ')" > "$LOG"
run_ok add B001 "The Left Hand of Darkness" || exit 1
run_ok list available || exit 1
run_fail add B001 "Duplicate must not replace" || exit 1
run_fail checkout UNKNOWN "No One" 2026-10-25 || exit 1
run_ok checkout B001 "A. Reader" 2026-10-25 || exit 1
run_fail checkout B001 "Second Reader" 2026-10-26 || exit 1
run_ok list checked-out || exit 1
run_ok return B001 || exit 1
run_ok list available || exit 1
