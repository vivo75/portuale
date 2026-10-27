#!/bin/bash
# run-portuale.sh <name> <args...> : portuale target/release on scratch cell $S/fxp (same tree as fx)
set -u
S=/var/tmp/claude-r1; OUT=$S/r1/captures
BIN=/home/vivo/repo/PORTUALE/portuale/target/release/portuale
d=$S/${CELL:-fxp}
name=$1; shift
B=(--ignore-default-opts --getbinpkg=n --usepkg=n --color=n -pv)
{ echo "### portuale argv: env -u EMERGE_DEFAULT_OPTS PORTAGE_CONFIGROOT=\$S/${CELL:-fxp} ROOT=\$S/${CELL:-fxp} PORTAGE_RUNNING_ROOT=\$S/${CELL:-fxp} DISTDIR=\$S/${CELL:-fxp}/distfiles PORTAGE_TMPDIR=\$S/ptmp PYTHONHASHSEED=0 PORTUALE_BT_TRACE=1 portuale emerge ${B[*]} $*"
  env -u EMERGE_DEFAULT_OPTS PORTAGE_CONFIGROOT=$d ROOT=$d PORTAGE_RUNNING_ROOT=$d DISTDIR=$d/distfiles \
    PORTAGE_TMPDIR=$S/ptmp PYTHONHASHSEED=0 PORTUALE_BT_TRACE=1 $BIN emerge "${B[@]}" "$@" 2>&1
  echo "### rc: $?"; } | sed -e "s#$S#\$S#g" -e 's/took [0-9.]* s/took N s/' > "$OUT/$name.txt"
echo "$name: $(grep -ho 'backtrack: [0-9/]*' $OUT/$name.txt | head -1) $(tail -1 $OUT/$name.txt) lines=$(wc -l < $OUT/$name.txt)"
