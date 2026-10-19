#!/bin/bash
# usage: resume-cells.sh <emerge>   (needs ./setup.sh first)  -- #415 (d)
S=${S7:-/var/tmp/b415}
export PORTAGE_CONFIGROOT=$S PORTAGE_TMPDIR=$S/tmp ROOT=$S/root DISTDIR=$S/dist
okcmd="cp $S/src/probe-1.tar.gz \"\${DISTDIR}/\${FILE}\""
EM=$1
rm -rf $S/dist $S/root; mkdir -p $S/dist $S/root
echo "=== seed: failing merge saves a resume list"
env FETCHCOMMAND="/bin/false \${FILE}" RESUMECOMMAND="/bin/false \${FILE}" $EM dev-util/fetchprobe </dev/null >/dev/null 2>&1; echo rc=$?
ls $S/root/var/cache/edb/ 2>&1
echo "=== resume -f"
out=$(env FETCHCOMMAND="$okcmd" RESUMECOMMAND="$okcmd" $EM --resume -f </dev/null 2>$S/stderr.txt); echo "$out"; echo rc=$?; echo "--- stderr"; cat $S/stderr.txt
echo "--- dist:"; ls $S/dist
echo "=== resume -pf"
out=$(env FETCHCOMMAND="$okcmd" $EM --resume -pf </dev/null 2>$S/stderr.txt); echo "$out"; echo rc=$?; echo "--- stderr"; cat $S/stderr.txt
echo "=== keep-going -f"
rm -rf $S/dist; mkdir $S/dist
out=$(env FETCHCOMMAND="/bin/false \${FILE}" $EM --keep-going -f dev-util/fetchprobe </dev/null 2>$S/stderr.txt); echo "$out"; echo rc=$?; echo "--- stderr"; cat $S/stderr.txt
