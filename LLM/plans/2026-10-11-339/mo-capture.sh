#!/bin/bash
# usage: mo-capture.sh OUTDIR   (merge-order selection traces for every L0 atom, host tree)
cd /home/vivo/repo/PORTUALE/portuale
T=$1; mkdir -p "$T"; T=$(cd "$T" && pwd)
export PORTUALE_BIN=/var/tmp/mo-bin/emerge  # symlink to target/release/portuale (applet name)
grep -vE '^\s*(#|$)' ../pmtest/differential-test-bed/atomlists/l0-resolve.txt | while read -r a; do
  ../pmtest/differential-test-bed/scripts/mo-trace/ptl-trace.sh "$T/$(printf %s "$a" | tr '/ ' '__')" -- -pv --getbinpkg=n --usepkg=n $a >/dev/null 2>&1
done
echo done > "$T/.done"
