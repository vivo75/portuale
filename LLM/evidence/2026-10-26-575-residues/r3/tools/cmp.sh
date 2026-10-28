#!/bin/bash
# cmp.sh <name>... : diff Portage (A-<name>) vs portuale (P-<name>) captures after removing harness noise
# (ARM/BT probe lines, took N s, the root suffix/` to '<root>'` that PORTAGE_RUNNING_ROOT=ROOT turns off in
# portuale, the package.mask comment block, the host-root installed-masked notice).
C=/var/tmp/claude-r3/r3/captures
norm() { sed -n '/would be merged/,$p' "$1" | sed -E 's/ARM .*bt=(True|False)//' | grep -v '^BT' \
  | sed -E -e 's#\$S/[a-z]+p?\.cfg#$CFG#g; s#\$S/(fx|ic|iq)p?/?#$R#g; s/took N s/took/' \
           -e "s/ for \\\$R//; s/ to '\\\$R'//; s/, installed in '\\\$R'/, installed/" \
  | grep -v '^$' | grep -v '^\$CFG/etc/portage/package.mask:$' | grep -v '^# ' \
  | awk '/^Calculating dependencies \.\.\.$/{getline n; print $0 n; next}1'; }
for n; do d=$(diff <(norm $C/A-$n.txt) <(norm $C/P-$n.txt)); if [ -z "$d" ]; then echo "== $n: SAME (after noise filter)"; else echo "== $n: DIFF"; echo "$d" | grep '^[<>]' | head -${MAXL:-14}; fi; done
