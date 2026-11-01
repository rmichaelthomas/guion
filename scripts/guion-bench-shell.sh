#!/bin/sh
# Run a command inside the persistent Guion benchmark container (starts Colima/container if stopped).
set -e
C=guion-core-north-star-sprint
colima status >/dev/null 2>&1 || colima start
[ "$(docker inspect -f '{{.State.Running}}' $C 2>/dev/null)" = true ] || docker start $C >/dev/null
exec docker exec -w "$(pwd)" -e PATH=/usr/local/cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin $C "$@"
