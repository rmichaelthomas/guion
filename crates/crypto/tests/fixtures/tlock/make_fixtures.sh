#!/bin/sh
# Produces the tlock fixtures with Go `tle` (github.com/drand/tlock v1.2.0, `tle --help` prints
# "tlock v1.2.0"), which must be on PATH. Needs network access to https://api.drand.sh/ because
# `tle` fetches the chain info. Run once; the outputs are committed and the tests read them.
#
# Network: drand quicknet, chain hash 52db9ba70e0cc0f6eaf7803dd07447a1f5477735fd3f661792ba94600c84e971
# (the `tle` default). Round 1000 is long past, so it needs --force.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
cd "$here"
tle --help 2>&1 | head -1   # expect: tlock v1.2.0

printf 'guion tlock fixture plaintext\n' > plaintext.txt

# Round 1000: decryptable with the published round 1000 signature.
tle --encrypt --force --armor -r 1000 -o round1000.age plaintext.txt

# A round about one hour ahead of generation time (quicknet: genesis 1692803367, period 3 s).
now=$(date +%s)
future=$(( (now - 1692803367) / 3 + 1 + 1200 ))
tle --encrypt --armor -r "$future" -o future.age plaintext.txt
printf '%s' "$future" > future_round.txt

# A payload longer than one 64 KiB STREAM chunk (70000 bytes) to exercise chunking.
python3 -I -c "import sys; sys.stdout.buffer.write(bytes(i % 251 for i in range(70000)))" > large_plaintext.bin
tle --encrypt --force --armor -r 1000 -o large1000.age large_plaintext.bin
