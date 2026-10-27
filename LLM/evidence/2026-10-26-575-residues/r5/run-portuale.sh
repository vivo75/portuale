#!/bin/bash
# Copied from the R5 S0 scratch (/var/tmp/claude-r5/runp2.sh). CELL=<name> picks the scratch cell (default fxp). The argv echo line always says $S/fxp; the W-* captures were rewritten to $S/fxwp with sed.
# runp.sh <name> <args...> : portuale (target/release, HEAD 6303332a) on scratch cell /var/tmp/claude-r5/fxp (copy of pmtest/fixtures)
set -u
S=/var/tmp/claude-r5; SRC=/home/vivo/repo/PORTUALE/pmtest/fixtures
OUT=/home/vivo/repo/PORTUALE/portuale/LLM/evidence/2026-10-26-575-residues/r5/captures
BIN=/home/vivo/repo/PORTUALE/portuale/target/release/portuale
d=$S/${CELL:-fxp}; [ -d $d ] || { mkdir -p $S/ptmp; cp -a "$SRC" "$d"; }
name=$1; shift
B=(--ignore-default-opts --getbinpkg=n --usepkg=n --color=n -pv)
{ echo "### portuale argv: env -u EMERGE_DEFAULT_OPTS PORTAGE_CONFIGROOT=\$S/fxp ROOT=\$S/fxp PYTHONHASHSEED=0 PORTUALE_BT_TRACE=1 portuale emerge ${B[*]} $*"
  env -u EMERGE_DEFAULT_OPTS PORTAGE_CONFIGROOT=$d ROOT=$d PORTAGE_RUNNING_ROOT=$d DISTDIR=$d/distfiles \
    PORTAGE_TMPDIR=$S/ptmp PYTHONHASHSEED=0 PORTUALE_BT_TRACE=1 $BIN emerge "${B[@]}" "$@" 2>&1
  echo "### rc: $?"; } | sed -e "s#$S#\$S#g" -e 's/took [0-9.]* s/took N s/' > "$OUT/$name.txt"
echo "$name: $(grep -ho 'backtrack: [0-9/]*' $OUT/$name.txt | head -1) $(tail -1 $OUT/$name.txt) lines=$(wc -l < $OUT/$name.txt)"
