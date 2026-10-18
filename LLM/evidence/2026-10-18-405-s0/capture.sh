#!/bin/bash
# Portage vs portuale: stderr+rc for each option-error argv (backlog #405).
PL=/home/vivo/repo/PORTUALE/portuale/target/release/portuale
python3 -V
emerge --version
while read -r line; do
  [ -z "$line" ] && continue
  echo "=== argv: $line"
  for who in portage portuale; do
    if [ $who = portage ]; then out=$(emerge $line 2>&1 >/dev/null </dev/null); rc=$?; else out=$($PL emerge $line 2>&1 >/dev/null </dev/null); rc=$?; fi
    echo "--- $who rc=$rc"; echo "$out" | tail -2
  done
done <<'ARGS'
--color
--exclude
--reinstall-atoms
--useoldpkg-atoms
--usepkg-exclude
--usepkg-include
--getbinpkg-exclude
--getbinpkg-include
--buildpkg-exclude
--with-bdeps
--autounmask-backtrack
--autounmask-use
--autounmask-license
--rebuilt-binaries-timestamp
--quickpkg-direct
--quickpkg-direct-root
-X
-jx
--jobs=x
--load-average
--load-average=x
-l
ARGS
