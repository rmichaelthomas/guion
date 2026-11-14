#!/bin/sh
# Contamination scan for a harness transcript.
#
# Usage: ./check.sh <transcript-file>
#
# Grep is a backstop, not proof. It catches a subject that names an evaluation
# artifact; it cannot catch one that read a brief and paraphrased it. That is why
# the prompt makes the subject list every path it opened, and why the runner reads
# that list.

set -eu

TRANSCRIPT=${1:?usage: check.sh <transcript-file>}
STATUS=0

if [ ! -f "$TRANSCRIPT" ]; then
    echo "no such transcript: $TRANSCRIPT" >&2
    exit 2
fi

echo "contamination scan: $TRANSCRIPT"
echo "----------------------------------------"

for marker in "RECOMMENDATION-" "BRIEF-guion-frameworks" "PROBE-sqlite" \
              "VERSIONING" "HARNESS-comprehension" "harness/tasks" "B14" "B13"; do
    if grep -q -- "$marker" "$TRANSCRIPT"; then
        echo "CONTAMINATED: mentions $marker"
        grep -n -- "$marker" "$TRANSCRIPT" | head -3
        STATUS=1
    fi
done

# Paths outside the subject root are the other leak. The runner passes the root in
# as $HX_ROOT when it is set.
if [ -n "${HX_ROOT:-}" ]; then
    OUTSIDE=$(grep -oE '/(Users|home|private|tmp|var)/[^ "'\'')]+' "$TRANSCRIPT" \
              | grep -v "^$HX_ROOT" | sort -u | head -10 || true)
    if [ -n "$OUTSIDE" ]; then
        echo "PATHS OUTSIDE THE ROOT (review each):"
        echo "$OUTSIDE"
        STATUS=1
    fi
fi

if grep -q "paths read" "$TRANSCRIPT"; then
    echo "path list present: yes"
else
    echo "path list present: NO — required by the prompt; ask for it before scoring"
fi

echo "----------------------------------------"
if [ "$STATUS" -eq 0 ]; then
    echo "scan: clean"
else
    echo "scan: review required — a hit invalidates the run unless it is provably unrelated"
fi
exit "$STATUS"
