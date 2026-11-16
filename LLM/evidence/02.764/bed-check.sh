#!/bin/bash
# usage: bed-check.sh OUTDIR ; plan 02.764 S0.8: checker-only container run (no package manager runs). Classifies the L0-health bed's
# FAST_world.json (sha prefix 96bc81c1e57d5e49, identical in the four 2026-11-16 runs) against the container's own /var/db/pkg.
# Writes OUTDIR/bed-table.txt and OUTDIR/bed-cycle-line.txt. Run via scoperun.sh.
set -e
O=${1:?outdir}; HERE=$(cd "$(dirname "$0")" && pwd); PM=$HERE/../../../../pmtest
LOG=$(ls -d $PM/differential-test-bed/logs/l0-health-20261116T113020Z)/plans
[ "$(sha256sum < $LOG/FAST_world.json | cut -c1-16)" = 96bc81c1e57d5e49 ] || { echo "STOP-3: bed plan hash differs"; exit 3; }
mkdir -p "$O"
podman run --rm --entrypoint /bin/sh -v "$HERE":/E:ro -v "$PM/pytests-contract-suite":/L:ro -v "$LOG":/P:ro -e PMTEST_CHECKERS=/L \
  localhost/test-portuale:latest -c '
python3 -I /E/classify.py --vdb /var/db/pkg /P/FAST_world.json
echo ---
python3 -I /E/hostcycle.py --vdb /var/db/pkg /P/FAST_world.json /P/FAST_world.stderr' > "$O/bed-out.txt"
sed -n '1,/^---$/p' "$O/bed-out.txt" | sed '$d' > "$O/bed-table.txt"
sed -n '/^---$/,$p' "$O/bed-out.txt" | tail -n +2 > "$O/bed-cycle-line.txt"
rm "$O/bed-out.txt"
