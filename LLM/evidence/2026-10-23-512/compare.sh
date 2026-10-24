#!/bin/bash
# #512 oracle check: per-shape Installing order + rc for both PMs.
# Usage: compare.sh [captures-dir]
set -u
D=${1:-$(cd "$(dirname "$0")" && pwd)/captures}
summ() { # id, pm
  local f="$D/$1.$2.out"
  [ -f "$f" ] || { echo "$1.$2: MISSING"; return; }
  local rc; rc=$(cat "$D/$1.$2.rc" 2>/dev/null || echo "?")
  local order
  order=$(grep -oE ">>> (Emerging|Installing) \([0-9]+ of [0-9]+\) [^ ]+" "$f" \
    | sed -E 's/>>> (Emerging|Installing) \([0-9]+ of [0-9]+\) ([^:]+).*/\2\1/' \
    | tr '\n' ' ')
  echo "$1.$2 rc=$rc :: $order"
}
for id in 01-scope-deep 02-scope-system 03-scope-toolchain 04-scope-none \
    05-default-features 06-deep-closure 07-system-closure 08-toolchain-sysextra \
    09-cycle 10-pretend 11-bad-scope 12-missing-scope 13-sigint 14-cgroup \
    15-pretend-j1 16-pretend-buildpkgonly 17-mixed-pretend; do
  summ "$id" portage
  summ "$id" portuale
done
