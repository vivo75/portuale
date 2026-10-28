#!/bin/bash
# pair.sh <name> <cell> <args...>: Portage (r6trace.py overlay) on $S/<cell> with cfg $S/<cell>.cfg,
# portuale (PORTUALE_BT_TRACE=1) on its own copy $S/<cell>p (config root = the copy, as the suite does).
set -u
S=/var/tmp/claude-r6; OUT=${OUT:-$S/captures}; BIN=/home/vivo/repo/PORTUALE/portuale/target/release/portuale
name=$1; cell=$2; shift 2
A=(--ignore-default-opts --pretend "$@")
r=$S/$cell; c=$S/$cell.cfg; q=$S/${cell}p
{ echo "portage argv: env -u EMERGE_DEFAULT_OPTS PORTAGE_CONFIGROOT=$c ROOT=$r PORTAGE_RUNNING_ROOT=$r DISTDIR=$r/distfiles PORTAGE_TMPDIR=$S/ptmp PYTHONHASHSEED=0 python3 -I $S/tools/r6trace.py ${A[*]}"
  echo "portuale argv: env -u EMERGE_DEFAULT_OPTS PORTAGE_CONFIGROOT=$q ROOT=$q PORTAGE_RUNNING_ROOT=$q DISTDIR=$q/distfiles PORTAGE_TMPDIR=$S/ptmp PORTUALE_BT_TRACE=1 PORTUALE_PROFILE=1 $BIN emerge ${A[*]}"; } > $OUT/$name.argv
env -u EMERGE_DEFAULT_OPTS PORTAGE_CONFIGROOT=$c ROOT=$r PORTAGE_RUNNING_ROOT=$r DISTDIR=$r/distfiles PORTAGE_TMPDIR=$S/ptmp PYTHONHASHSEED=0 \
  python3 -I $S/tools/r6trace.py "${A[@]}" > $OUT/$name.portage.out 2> $OUT/$name.portage.err; echo $? > $OUT/$name.portage.rc
env -u EMERGE_DEFAULT_OPTS PORTAGE_CONFIGROOT=$q ROOT=$q PORTAGE_RUNNING_ROOT=$q DISTDIR=$q/distfiles PORTAGE_TMPDIR=$S/ptmp PORTUALE_BT_TRACE=1 PORTUALE_PROFILE=1 \
  $BIN emerge "${A[@]}" > $OUT/$name.portuale.out 2> $OUT/$name.portuale.err; echo $? > $OUT/$name.portuale.rc
bt() { grep -ho 'backtrack: [0-9]*/[0-9]*' "$@" | head -1 | sed 's/backtrack: //'; }
pp=$(grep -c '^R6 PASS' $OUT/$name.portage.err)
echo "$name | Portage $(bt $OUT/$name.portage.out $OUT/$name.portage.err) rc $(cat $OUT/$name.portage.rc) passes=$pp arms=$(grep -o 'ARM [a-z-]*' $OUT/$name.portage.err | sed 's/ARM //' | tr '\n' ,)$(grep -q BEST-RUN $OUT/$name.portage.err && echo +best)$(grep -q BREAKAGE $OUT/$name.portage.err && echo +breakage) | portuale $(bt $OUT/$name.portuale.out $OUT/$name.portuale.err) rc $(cat $OUT/$name.portuale.rc) bt-tries=$(grep -c '^BT try=' $OUT/$name.portuale.err) kinds=$(grep -o '^BT try=[0-9]* kind=[A-Za-z:]*' $OUT/$name.portuale.err | sed 's/.*kind=//' | tr '\n' ,)"
