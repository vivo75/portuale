#!/bin/bash
# usage: cells.sh <emerge> (after setup.sh) -- #421: --resume of an interactive package
S=${S7:-/var/tmp/b421}
mkdir -p $S/log; export EMERGE_LOG_DIR=$S/log PORTAGE_CONFIGROOT=$S PORTAGE_TMPDIR=$S/tmp ROOT=$S/root DISTDIR=$S/dist
okcmd="cp $S/src/probe-1.tar.gz \"\${DISTDIR}/\${FILE}\""
EM=$1
rm -rf $S/dist $S/root; mkdir -p $S/dist $S/root
echo "=== seed"
env FETCHCOMMAND="/bin/false \${FILE}" RESUMECOMMAND="/bin/false \${FILE}" $EM dev-util/intprobe </dev/null >/dev/null 2>&1; echo rc=$?
echo "=== resume -p"
$EM --resume -p </dev/null 2>&1; echo rc=$?
echo "=== resume -j2"
env FETCHCOMMAND="$okcmd" RESUMECOMMAND="$okcmd" $EM --resume -j2 </dev/null 2>&1; echo rc=$?
