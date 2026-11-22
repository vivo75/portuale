#!/bin/bash
# usage: bed-plan.sh BIN_DIR OUT_DIR ; runs the fast plan (mrg --plan-world --json --sort depend) of a given
# binary directory inside the l0-health image, then both checkers on it. Run via scoperun.sh.
set -e
BIN=${1:?bin dir}; O=${2:?out dir}
HERE=$(cd "$(dirname "$0")" && pwd); PM=$HERE/../../../../pmtest
E764=$HERE/../02.764
mkdir -p "$O"
podman run --rm --entrypoint /bin/sh \
  -v "$BIN":/usr/local/bin:ro -v "$O":/out -v "$E764":/E:ro -v "$PM/pytests-contract-suite":/L:ro \
  -e PMTEST_CHECKERS=/L -e PORTAGE_CONFIGROOT=/ -e ROOT=/ -e LC_ALL=C.UTF-8 -e TZ=UTC \
  localhost/test-portuale:latest -c '
printf "%s\n" app-misc/tmux app-portage/eix app-portage/gentoolkit dev-libs/blake3 > /var/lib/portage/world
timeout 1200 /usr/local/bin/mrg --plan-world --json --sort depend > /out/FAST_world.json 2> /out/FAST_world.stderr || true
python3 -I /L/health_checker.py --vdb /var/db/pkg /out/FAST_world.json > /out/literal.txt 2>&1 || true
python3 -I /E/strict_health_checker.py --vdb /var/db/pkg /out/FAST_world.json > /out/strict.txt 2>&1 || true
sha256sum /out/FAST_world.json | cut -c1-16'
