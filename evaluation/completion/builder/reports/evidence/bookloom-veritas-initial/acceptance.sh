#!/bin/sh
set -u
BASE="/private/tmp/veritas-fresh-builders/bookloom-veritas"
BIN="/Users/rmichaelthomas/Documents/Codex/2026-10-22/referenced-chatgpt-conversation-this-is-an/work/veritas-foundation/target/completion/releases/macos-aarch64/current/bin/veritas"
APP="$BASE/app"
DATA="$BASE/test-data"
LOGS="$BASE/logs"
RELOC_PROJECT="$BASE/relocated/Bookloom — 銀河の蔵書"
RELOC_DATA="$BASE/relocated-data"
mkdir -p "$LOGS"
: > "$LOGS/commands.log"
: > "$LOGS/assertions.log"
failed=0

run_case() {
    case_name=$1
    expected=$2
    target=$3
    data=$4
    shift 4
    {
        printf 'case=%s\nexpected_exit=%s\n' "$case_name" "$expected"
        printf 'veritas run --file-root %s %s' "$data" "$target"
        for argument in "$@"; do printf ' [%s]' "$argument"; done
        printf '\n'
    } >> "$LOGS/commands.log"
    "$BIN" run --file-root "$data" "$target" "$@" > "$LOGS/$case_name.out" 2>&1
    actual=$?
    printf 'actual_exit=%s\n' "$actual" >> "$LOGS/commands.log"
    cat "$LOGS/$case_name.out" >> "$LOGS/commands.log"
    printf '\n' >> "$LOGS/commands.log"
    if [ "$actual" -ne "$expected" ]; then
        printf 'FAIL %s exit=%s expected=%s\n' "$case_name" "$actual" "$expected" | tee -a "$LOGS/assertions.log"
        failed=1
    else
        printf 'PASS %s exit=%s\n' "$case_name" "$actual" | tee -a "$LOGS/assertions.log"
    fi
}

contains() {
    case_name=$1
    needle=$2
    if grep -Fq "$needle" "$LOGS/$case_name.out"; then
        printf 'PASS %s contains: %s\n' "$case_name" "$needle" | tee -a "$LOGS/assertions.log"
    else
        printf 'FAIL %s missing: %s\n' "$case_name" "$needle" | tee -a "$LOGS/assertions.log"
        failed=1
    fi
}

run_case add-second 0 "$APP" "$DATA" add BK-002 "A Psalm for the Wild-Built" "Becky Chambers"
contains add-second "added BK-002"
run_case add-available 0 "$APP" "$DATA" add BK-003 "The Dispossessed" "Ursula K. Le Guin"
run_case checkout-first 0 "$APP" "$DATA" checkout BK-001 "Mira Patel"
run_case persistence-list 0 "$APP" "$DATA" list
contains persistence-list "BK-001 | A Long Way to a Small, Angry Planet | Becky Chambers | checked_out | Mira Patel | due "
contains persistence-list "BK-002 | A Psalm for the Wild-Built | Becky Chambers | available"
run_case duplicate-id 1 "$APP" "$DATA" add BK-001 "Replacement title" "Other Author"
contains duplicate-id "book ID already exists: BK-001"
run_case unknown-checkout 1 "$APP" "$DATA" checkout BK-404 "Mira Patel"
contains unknown-checkout "unknown book ID: BK-404"
run_case already-checked-out 1 "$APP" "$DATA" checkout BK-001 "Another Borrower"
contains already-checked-out "book is already checked out: BK-001"
run_case checkout-second 0 "$APP" "$DATA" checkout BK-002 "Alex Quinn"
run_case available-filter 0 "$APP" "$DATA" list available
contains available-filter "BK-003 | The Dispossessed | Ursula K. Le Guin | available"
run_case checked-out-filter 0 "$APP" "$DATA" list checked-out
contains checked-out-filter "Mira Patel"
contains checked-out-filter "Alex Quinn"
AS_OF=$(date -u '+%Y-%m-%d')
run_case due-soon 0 "$APP" "$DATA" due-soon --as-of "$AS_OF"
contains due-soon "BK-001"
contains due-soon "BK-002"
run_case return-first 0 "$APP" "$DATA" return BK-001
run_case list-after-return 0 "$APP" "$DATA" list
contains list-after-return "BK-001 | A Long Way to a Small, Angry Planet | Becky Chambers | available"
run_case unknown-return 1 "$APP" "$DATA" return BK-404
contains unknown-return "unknown book ID: BK-404"
shasum -a 256 "$DATA/bookloom.sqlite" > "$LOGS/invalid-date.before.sha256"
run_case invalid-date 2 "$APP" "$DATA" due-soon --as-of 2025-02-30
contains invalid-date "invalid date: 2025-02-30"
shasum -a 256 "$DATA/bookloom.sqlite" > "$LOGS/invalid-date.after.sha256"
if cmp -s "$LOGS/invalid-date.before.sha256" "$LOGS/invalid-date.after.sha256"; then
    printf 'PASS invalid-date preserved database SHA-256\n' | tee -a "$LOGS/assertions.log"
else
    printf 'FAIL invalid-date changed database SHA-256\n' | tee -a "$LOGS/assertions.log"
    failed=1
fi

cp -R "$APP" "$RELOC_PROJECT"
mkdir -p "$RELOC_DATA"
run_case relocation-add 0 "$RELOC_PROJECT" "$RELOC_DATA" add UNI-001 "The Left Hand of Darkness" "Ursula K. Le Guin"
run_case relocation-restart 0 "$RELOC_PROJECT" "$RELOC_DATA" list
contains relocation-restart "UNI-001 | The Left Hand of Darkness | Ursula K. Le Guin | available"
if [ -f "$RELOC_DATA/bookloom.sqlite" ] && [ ! -f "$RELOC_PROJECT/bookloom.sqlite" ]; then
    printf 'PASS relocation database is in separate caller data directory\n' | tee -a "$LOGS/assertions.log"
else
    printf 'FAIL relocation database is not isolated from project\n' | tee -a "$LOGS/assertions.log"
    failed=1
fi

{
    printf 'build timing command: /usr/bin/time -p veritas build --json app/src/main.vr\n'
    /usr/bin/time -p "$BIN" build --json "$APP/src/main.vr"
} > "$LOGS/build-timing.txt" 2>&1
{
    printf 'installed launch timing command: /usr/bin/time -p veritas run --file-root %s %s list\n' "$DATA" "$APP"
    /usr/bin/time -p "$BIN" run --file-root "$DATA" "$APP" list
} > "$LOGS/installed-launch-timing.txt" 2>&1
printf 'relocated_project=%s\nrelocated_data=%s\n' "$RELOC_PROJECT" "$RELOC_DATA" > "$LOGS/relocation-paths.txt"
printf 'failed=%s\n' "$failed" >> "$LOGS/assertions.log"
exit "$failed"
